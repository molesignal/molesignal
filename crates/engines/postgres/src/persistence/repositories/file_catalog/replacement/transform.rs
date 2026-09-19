// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cross-dataset atomic publication, including an optional sealed input base.

use std::collections::{HashMap, HashSet};

use sqlx::{PgPool, Row};

use super::super::{
    rows::{to_i64, to_u64},
    sqlx_err,
    writes::{
        bump_dataset_version, enqueue_object_gc, insert_segment, tombstone_segment_artifacts,
    },
};
use crate::{
    domain::storage::{
        ArtifactState, DatasetTransformResult, GcReason, OrganizationScope,
        PublishDatasetTransform, SegmentState,
    },
    shared::{Error, Result},
};

pub(crate) async fn publish_dataset_transform(
    pool: &PgPool,
    scope: &OrganizationScope,
    command: PublishDatasetTransform,
) -> Result<DatasetTransformResult> {
    validate_command(scope, &command)?;
    let input_ids = command
        .input_segment_ids
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect::<Vec<_>>();
    let dataset_ids = vec![
        command.input_dataset_id.as_str().to_owned(),
        command.output_dataset_id.as_str().to_owned(),
    ];

    let mut tx = sqlx::begin(pool).await.map_err(sqlx_err)?;
    // A stable lock order prevents opposite-direction transforms from deadlocking.
    let dataset_rows = sqlx::query(
        "SELECT id, logical_stream_id, catalog_version FROM physical_datasets \
         WHERE org_id = $1 AND id = ANY($2) ORDER BY id FOR UPDATE",
    )
    .bind(scope.organization_id.as_str())
    .bind(&dataset_ids)
    .fetch_all(&mut *tx)
    .await
    .map_err(sqlx_err)?;
    if dataset_rows.len() != 2 {
        return Err(Error::not_found(
            "one or more datasets in the transform do not exist",
        ));
    }
    let versions = dataset_versions(&dataset_rows)?;
    let input_version = next_version(&versions, command.input_dataset_id.as_str(), "input")?;
    let output_version = next_version(&versions, command.output_dataset_id.as_str(), "output")?;

    let active_manifest = lock_manifest(&mut tx, scope, &command).await?;
    validate_manifest_identity(&command, active_manifest.as_ref())?;
    validate_input_rows(&mut tx, scope, &command, &input_ids).await?;

    if let Some(pointer) = &command.input_manifest {
        sqlx::query(
            "UPDATE partition_manifests SET state = 'retired' WHERE org_id = $1 \
               AND dataset_id = $2 AND partition_start_micros = $3 \
               AND partition_shard = $4 AND state = 'active'",
        )
        .bind(scope.organization_id.as_str())
        .bind(command.input_dataset_id.as_str())
        .bind(command.input_partition.start_micros)
        .bind(command.input_partition.shard as i16)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        enqueue_object_gc(
            &mut tx,
            scope,
            &pointer.object.key,
            Some(&pointer.object.checksum),
            GcReason::Replaced,
            command.gc_not_before_micros,
        )
        .await?;
    }

    sqlx::query(
        "UPDATE data_segments SET state = 'replaced', retired_at_version = $4 \
         WHERE org_id = $1 AND dataset_id = $2 AND id = ANY($3) \
           AND state IN ('active', 'sealed')",
    )
    .bind(scope.organization_id.as_str())
    .bind(command.input_dataset_id.as_str())
    .bind(&input_ids)
    .bind(to_i64(input_version, "retired_at_version")?)
    .execute(&mut *tx)
    .await
    .map_err(sqlx_err)?;
    tombstone_segment_artifacts(
        &mut tx,
        scope,
        &input_ids,
        GcReason::Replaced,
        command.gc_not_before_micros,
    )
    .await?;

    for segment in &command.output_segments {
        insert_segment(&mut tx, scope, segment, output_version).await?;
    }
    bump_dataset_version(&mut tx, scope, &command.input_dataset_id, input_version).await?;
    bump_dataset_version(&mut tx, scope, &command.output_dataset_id, output_version).await?;
    tx.commit().await.map_err(sqlx_err)?;
    Ok(DatasetTransformResult {
        input_catalog_version: input_version,
        output_catalog_version: output_version,
    })
}

fn dataset_versions(rows: &[sqlx::postgres::PgRow]) -> Result<HashMap<String, u64>> {
    let mut versions = HashMap::with_capacity(2);
    let mut logical_stream_id = None;
    for row in rows {
        let dataset_id: String = row.try_get("id").map_err(sqlx_err)?;
        let stream_id: String = row.try_get("logical_stream_id").map_err(sqlx_err)?;
        if logical_stream_id
            .as_ref()
            .is_some_and(|expected| expected != &stream_id)
        {
            return Err(Error::invalid(
                "cross-dataset transform datasets must belong to the same logical stream",
            ));
        }
        logical_stream_id = Some(stream_id);
        versions.insert(
            dataset_id,
            to_u64(
                row.try_get::<i64, _>("catalog_version").map_err(sqlx_err)?,
                "catalog_version",
            )?,
        );
    }
    Ok(versions)
}

fn next_version(versions: &HashMap<String, u64>, dataset_id: &str, role: &str) -> Result<u64> {
    versions
        .get(dataset_id)
        .ok_or_else(|| Error::not_found(format!("physical dataset {dataset_id}")))?
        .checked_add(1)
        .ok_or_else(|| Error::internal(format!("{role} catalog version overflow")))
}

async fn lock_manifest(
    conn: &mut sqlx::PgConnection,
    scope: &OrganizationScope,
    command: &PublishDatasetTransform,
) -> Result<Option<sqlx::postgres::PgRow>> {
    sqlx::query(
        "SELECT generation, object_key, checksum, segment_count FROM partition_manifests \
         WHERE org_id = $1 AND dataset_id = $2 AND partition_start_micros = $3 \
           AND partition_shard = $4 AND state = 'active' FOR UPDATE",
    )
    .bind(scope.organization_id.as_str())
    .bind(command.input_dataset_id.as_str())
    .bind(command.input_partition.start_micros)
    .bind(command.input_partition.shard as i16)
    .fetch_optional(&mut *conn)
    .await
    .map_err(sqlx_err)
}

fn validate_manifest_identity(
    command: &PublishDatasetTransform,
    actual: Option<&sqlx::postgres::PgRow>,
) -> Result<()> {
    match (&command.input_manifest, actual) {
        (None, None) => Ok(()),
        (Some(expected), Some(actual)) => {
            let generation = to_u64(
                actual.try_get::<i64, _>("generation").map_err(sqlx_err)?,
                "manifest generation",
            )?;
            let segment_count = u32::try_from(
                actual
                    .try_get::<i32, _>("segment_count")
                    .map_err(sqlx_err)?,
            )
            .map_err(|_| Error::internal("negative manifest segment_count"))?;
            let identity_matches = generation == expected.generation
                && actual
                    .try_get::<String, _>("object_key")
                    .map_err(sqlx_err)?
                    == expected.object.key.as_str()
                && actual.try_get::<String, _>("checksum").map_err(sqlx_err)?
                    == expected.object.checksum.as_str()
                && segment_count == expected.segment_count;
            if identity_matches {
                Ok(())
            } else {
                Err(Error::conflict(
                    "input manifest generation changed before dataset transform",
                ))
            }
        }
        _ => Err(Error::conflict(
            "input manifest presence changed before dataset transform",
        )),
    }
}

async fn validate_input_rows(
    conn: &mut sqlx::PgConnection,
    scope: &OrganizationScope,
    command: &PublishDatasetTransform,
    input_ids: &[String],
) -> Result<()> {
    let input_rows = sqlx::query(
        "SELECT id, state FROM data_segments WHERE org_id = $1 AND dataset_id = $2 \
           AND id = ANY($3) AND partition_start_micros = $4 AND partition_end_micros = $5 \
           AND partition_shard = $6 FOR UPDATE",
    )
    .bind(scope.organization_id.as_str())
    .bind(command.input_dataset_id.as_str())
    .bind(input_ids)
    .bind(command.input_partition.start_micros)
    .bind(command.input_partition.end_micros)
    .bind(command.input_partition.shard as i16)
    .fetch_all(&mut *conn)
    .await
    .map_err(sqlx_err)?;
    if input_rows.len() != input_ids.len() {
        return Err(Error::conflict(
            "some transform input segments no longer exist in the input dataset",
        ));
    }
    let mut selected_sealed = HashSet::new();
    for row in &input_rows {
        let id: String = row.try_get("id").map_err(sqlx_err)?;
        let state: String = row.try_get("state").map_err(sqlx_err)?;
        match state.parse::<SegmentState>()? {
            SegmentState::Active => {}
            SegmentState::Sealed if command.input_manifest.is_some() => {
                selected_sealed.insert(id);
            }
            _ => {
                return Err(Error::conflict(format!(
                    "transform input segment {id} was already {state}"
                )));
            }
        }
    }
    if let Some(pointer) = &command.input_manifest {
        validate_complete_sealed_base(conn, scope, command, pointer.segment_count, selected_sealed)
            .await?;
    }
    Ok(())
}

async fn validate_complete_sealed_base(
    conn: &mut sqlx::PgConnection,
    scope: &OrganizationScope,
    command: &PublishDatasetTransform,
    expected_count: u32,
    selected: HashSet<String>,
) -> Result<()> {
    let rows = sqlx::query(
        "SELECT id FROM data_segments WHERE org_id = $1 AND dataset_id = $2 \
           AND partition_start_micros = $3 AND partition_end_micros = $4 \
           AND partition_shard = $5 AND state = 'sealed' FOR UPDATE",
    )
    .bind(scope.organization_id.as_str())
    .bind(command.input_dataset_id.as_str())
    .bind(command.input_partition.start_micros)
    .bind(command.input_partition.end_micros)
    .bind(command.input_partition.shard as i16)
    .fetch_all(&mut *conn)
    .await
    .map_err(sqlx_err)?;
    let all = rows
        .iter()
        .map(|row| row.try_get::<String, _>("id").map_err(sqlx_err))
        .collect::<std::result::Result<HashSet<_>, _>>()?;
    if all != selected || all.len() != expected_count as usize {
        return Err(Error::conflict(
            "dataset transform does not cover the complete active manifest base",
        ));
    }
    Ok(())
}

fn validate_command(scope: &OrganizationScope, command: &PublishDatasetTransform) -> Result<()> {
    if command.input_dataset_id == command.output_dataset_id {
        return Err(Error::invalid(
            "cross-dataset transform requires distinct input and output datasets",
        ));
    }
    if command.input_segment_ids.is_empty() || command.output_segments.is_empty() {
        return Err(Error::invalid(
            "cross-dataset transform requires input and output segments",
        ));
    }
    if command.input_partition.start_micros >= command.input_partition.end_micros {
        return Err(Error::invalid(
            "cross-dataset transform input partition is invalid",
        ));
    }
    if let Some(pointer) = &command.input_manifest
        && (pointer.organization_id != scope.organization_id
            || pointer.dataset_id != command.input_dataset_id
            || pointer.partition != command.input_partition
            || pointer.segment_count == 0)
    {
        return Err(Error::invalid(
            "cross-dataset transform manifest identity does not match its input",
        ));
    }
    let unique_inputs = command.input_segment_ids.iter().collect::<HashSet<_>>();
    if unique_inputs.len() != command.input_segment_ids.len() {
        return Err(Error::invalid(
            "cross-dataset transform contains duplicate input segment ids",
        ));
    }
    let mut output_ids = HashSet::with_capacity(command.output_segments.len());
    for segment in &command.output_segments {
        segment.validate()?;
        if segment.organization_id != scope.organization_id
            || segment.dataset_id != command.output_dataset_id
            || segment.state != SegmentState::Active
            || segment.primary.state != ArtifactState::Ready
            || segment.partition != command.input_partition
            || segment.flush_id.is_some()
            || segment.sequence_range.is_some()
            || !output_ids.insert(segment.id.clone())
        {
            return Err(Error::invalid(format!(
                "transform output segment {} has invalid identity, state, or dataset scope",
                segment.id
            )));
        }
    }
    Ok(())
}
