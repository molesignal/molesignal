// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use sqlx::{PgPool, Row};

use super::{
    rows::{to_i64, to_u64},
    sqlx_err,
    writes::{
        bump_dataset_version, enqueue_object_gc, insert_artifact, lock_dataset_version,
        tombstone_segment_artifacts,
    },
};
use crate::{
    domain::storage::{
        ArtifactReplacement, ArtifactState, GcReason, ObjectChecksum, ObjectKey, OrganizationScope,
        PublishPartitionManifest,
    },
    shared::{Error, Result, time::TimestampMicros},
};

pub(super) async fn publish(
    pool: &PgPool,
    scope: &OrganizationScope,
    command: PublishPartitionManifest,
) -> Result<u64> {
    validate_command(scope, &command)?;
    let mut tx = sqlx::begin(pool).await.map_err(sqlx_err)?;
    let current_version = lock_dataset_version(&mut tx, scope, &command.dataset_id).await?;

    let new_version = current_version
        .checked_add(1)
        .ok_or_else(|| Error::internal("catalog version overflow"))?;

    let active = sqlx::query(
        "SELECT generation, object_key, checksum FROM partition_manifests \
         WHERE org_id = $1 AND dataset_id = $2 AND partition_start = $3 \
           AND partition_end = $4 AND partition_shard = $5 AND state = 'active' \
         FOR UPDATE",
    )
    .bind(scope.organization_id.as_str())
    .bind(command.dataset_id.as_str())
    .bind(TimestampMicros(command.partition.start_micros))
    .bind(TimestampMicros(command.partition.end_micros))
    .bind(command.partition.shard as i16)
    .fetch_optional(&mut *tx)
    .await
    .map_err(sqlx_err)?;

    let active_generation = active
        .as_ref()
        .map(|row| {
            to_u64(
                row.try_get::<i64, _>("generation").map_err(sqlx_err)?,
                "manifest generation",
            )
        })
        .transpose()?;

    if active_generation != command.expected_generation {
        return Err(Error::conflict(format!(
            "manifest generation changed: expected {:?}, found {:?}",
            command.expected_generation, active_generation
        )));
    }

    validate_segment_rows(
        &mut tx,
        scope,
        &command,
        "active",
        &command.seal_segment_ids,
    )
    .await?;

    validate_tombstones(&mut tx, scope, &command).await?;

    for replacement in &command.replace_artifacts {
        replace_sealed_artifact(&mut tx, scope, &command, replacement).await?;
    }

    if let Some(active) = active {
        sqlx::query(
            "UPDATE partition_manifests SET state = 'retired' \
             WHERE org_id = $1 AND dataset_id = $2 AND partition_start = $3 \
               AND partition_end = $4 AND partition_shard = $5 AND state = 'active'",
        )
        .bind(scope.organization_id.as_str())
        .bind(command.dataset_id.as_str())
        .bind(TimestampMicros(command.partition.start_micros))
        .bind(TimestampMicros(command.partition.end_micros))
        .bind(command.partition.shard as i16)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        let retired_key = active
            .try_get::<String, _>("object_key")
            .map_err(sqlx_err)?;

        enqueue_object_gc(
            &mut tx,
            scope,
            &ObjectKey::from_string(retired_key.clone()),
            Some(&ObjectChecksum::from_string(
                active.try_get::<String, _>("checksum").map_err(sqlx_err)?,
            )),
            GcReason::Replaced,
            command.gc_not_before_micros,
        )
        .await?;

        enqueue_object_gc(
            &mut tx,
            scope,
            &bloom_key(&retired_key),
            None,
            GcReason::Replaced,
            command.gc_not_before_micros,
        )
        .await?;
    }

    if let Some(pointer) = &command.new_manifest {
        sqlx::query(
            "INSERT INTO partition_manifests (org_id, dataset_id, partition_start, \
             partition_end, partition_shard, generation, object_key, size_bytes, \
             checksum, etag, segment_count, state, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'active', $12)",
        )
        .bind(scope.organization_id.as_str())
        .bind(command.dataset_id.as_str())
        .bind(TimestampMicros(command.partition.start_micros))
        .bind(TimestampMicros(command.partition.end_micros))
        .bind(command.partition.shard as i16)
        .bind(to_i64(pointer.generation, "manifest generation")?)
        .bind(pointer.object.key.as_str())
        .bind(to_i64(pointer.object.size_bytes, "manifest size_bytes")?)
        .bind(pointer.object.checksum.as_str())
        .bind(pointer.object.etag.as_deref())
        .bind(
            i32::try_from(pointer.segment_count)
                .map_err(|_| Error::invalid("manifest segment_count exceeds INTEGER"))?,
        )
        .bind(TimestampMicros(pointer.created_at_micros))
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
    }

    if !command.seal_segment_ids.is_empty() {
        let ids = id_strings(&command.seal_segment_ids);

        sqlx::query(
            "UPDATE catalog_segments SET state = 'sealed', retired_at_version = $4 \
             WHERE org_id = $1 AND dataset_id = $2 AND id = ANY($3) AND state = 'active'",
        )
        .bind(scope.organization_id.as_str())
        .bind(command.dataset_id.as_str())
        .bind(&ids)
        .bind(to_i64(new_version, "retired_at_version")?)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
    }

    if !command.tombstone_segment_ids.is_empty() {
        let ids = id_strings(&command.tombstone_segment_ids);

        sqlx::query(
            "UPDATE catalog_segments SET state = 'tombstoned', retired_at_version = $4 \
             WHERE org_id = $1 AND dataset_id = $2 AND id = ANY($3) \
               AND state IN ('active', 'sealed')",
        )
        .bind(scope.organization_id.as_str())
        .bind(command.dataset_id.as_str())
        .bind(&ids)
        .bind(to_i64(new_version, "retired_at_version")?)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        tombstone_segment_artifacts(
            &mut tx,
            scope,
            &ids,
            GcReason::Tombstoned,
            command.gc_not_before_micros,
        )
        .await?;
    }

    bump_dataset_version(&mut tx, scope, &command.dataset_id, new_version).await?;
    tx.commit().await.map_err(sqlx_err)?;
    Ok(new_version)
}

/// Swaps an index artifact of a sealed row of this partition for its rebuilt replacement, in the
/// transaction that publishes the manifest naming the replacement: the old row is tombstoned and
/// its object enters delayed GC, the replacement takes the slot.
async fn replace_sealed_artifact(
    conn: &mut sqlx::PgConnection,
    scope: &OrganizationScope,
    command: &PublishPartitionManifest,
    replacement: &ArtifactReplacement,
) -> Result<()> {
    let current = sqlx::query(
        "SELECT a.state, a.role, a.artifact_type, a.object_key, a.checksum, \
                a.source_artifact_id, a.source_checksum \
         FROM catalog_artifacts a \
         JOIN catalog_segments s ON s.org_id = a.org_id AND s.id = a.segment_id \
         WHERE a.org_id = $1 AND a.id = $2 AND a.segment_id = $3 AND s.dataset_id = $4 \
           AND s.state = 'sealed' AND s.partition_start = $5 \
           AND s.partition_end = $6 AND s.partition_shard = $7 \
         FOR UPDATE OF a",
    )
    .bind(scope.organization_id.as_str())
    .bind(replacement.artifact_id.as_str())
    .bind(replacement.segment_id.as_str())
    .bind(command.dataset_id.as_str())
    .bind(TimestampMicros(command.partition.start_micros))
    .bind(TimestampMicros(command.partition.end_micros))
    .bind(command.partition.shard as i16)
    .fetch_optional(&mut *conn)
    .await
    .map_err(sqlx_err)?
    .ok_or_else(|| {
        Error::conflict(format!(
            "artifact {} is not on a sealed row of this partition",
            replacement.artifact_id
        ))
    })?;

    let text = |column: &str| current.try_get::<String, _>(column).map_err(sqlx_err);
    let optional = |column: &str| {
        current
            .try_get::<Option<String>, _>(column)
            .map_err(sqlx_err)
    };
    let rebuilt = &replacement.replacement;

    let same_slot = text("state")? == ArtifactState::Ready.as_str()
        && text("role")? == rebuilt.role.as_str()
        && text("artifact_type")? == rebuilt.artifact_type.as_str()
        && optional("source_artifact_id")?.as_deref()
            == rebuilt.source_artifact_id.as_ref().map(|id| id.as_str())
        && optional("source_checksum")?.as_deref()
            == rebuilt.source_checksum.as_ref().map(ObjectChecksum::as_str);

    if !same_slot || rebuilt.state != ArtifactState::Ready {
        return Err(Error::invalid(
            "a rebuilt artifact must be ready and keep the slot and source of the one it replaces",
        ));
    }

    sqlx::query(
        "UPDATE catalog_artifacts SET state = 'tombstoned', updated_at = $3 \
         WHERE org_id = $1 AND id = $2",
    )
    .bind(scope.organization_id.as_str())
    .bind(replacement.artifact_id.as_str())
    .bind(TimestampMicros::now())
    .execute(&mut *conn)
    .await
    .map_err(sqlx_err)?;

    enqueue_object_gc(
        conn,
        scope,
        &ObjectKey::from_string(text("object_key")?),
        Some(&ObjectChecksum::from_string(text("checksum")?)),
        GcReason::IndexRebuilt,
        command.gc_not_before_micros,
    )
    .await?;

    insert_artifact(conn, scope, &replacement.segment_id, rebuilt).await
}

/// The Bloom filters written beside a manifest. A partition without exact fields has none; the
/// collector treats an object that is already gone as deleted.
pub(super) fn bloom_key(manifest_key: &str) -> ObjectKey {
    ObjectKey::from_string(format!("{manifest_key}.bloom"))
}

fn validate_command(scope: &OrganizationScope, command: &PublishPartitionManifest) -> Result<()> {
    let seals: HashSet<_> = command.seal_segment_ids.iter().collect();

    if seals.len() != command.seal_segment_ids.len()
        || command
            .tombstone_segment_ids
            .iter()
            .any(|id| seals.contains(id))
    {
        return Err(Error::invalid(
            "manifest seal/tombstone segment ids must be unique and disjoint",
        ));
    }

    match &command.new_manifest {
        Some(pointer) => {
            let expected_generation = command
                .expected_generation
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| Error::invalid("manifest generation overflow"))?;

            if pointer.organization_id != scope.organization_id
                || pointer.dataset_id != command.dataset_id
                || pointer.partition != command.partition
                || pointer.generation != expected_generation
            {
                return Err(Error::invalid(
                    "new manifest pointer identity or generation does not match command",
                ));
            }
        }
        None if command.expected_generation.is_none()
            && command.tombstone_segment_ids.is_empty() =>
        {
            return Err(Error::invalid(
                "manifest command has no pointer switch or retention work",
            ));
        }
        None => {}
    }

    Ok(())
}

async fn validate_segment_rows(
    conn: &mut sqlx::PgConnection,
    scope: &OrganizationScope,
    command: &PublishPartitionManifest,
    state: &str,
    ids: &[crate::domain::storage::SegmentId],
) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }

    let rows = sqlx::query(
        "SELECT id FROM catalog_segments WHERE org_id = $1 AND dataset_id = $2 AND id = ANY($3) \
         AND partition_start = $4 AND partition_end = $5 \
         AND partition_shard = $6 AND state = $7 FOR UPDATE",
    )
    .bind(scope.organization_id.as_str())
    .bind(command.dataset_id.as_str())
    .bind(id_strings(ids))
    .bind(TimestampMicros(command.partition.start_micros))
    .bind(TimestampMicros(command.partition.end_micros))
    .bind(command.partition.shard as i16)
    .bind(state)
    .fetch_all(conn)
    .await
    .map_err(sqlx_err)?;

    if rows.len() != ids.len() {
        return Err(Error::conflict(
            "manifest input segments changed before generation switch",
        ));
    }

    Ok(())
}

async fn validate_tombstones(
    conn: &mut sqlx::PgConnection,
    scope: &OrganizationScope,
    command: &PublishPartitionManifest,
) -> Result<()> {
    let ids = &command.tombstone_segment_ids;

    if ids.is_empty() {
        return Ok(());
    }

    let rows = sqlx::query(
        "SELECT id FROM catalog_segments WHERE org_id = $1 AND dataset_id = $2 AND id = ANY($3) \
         AND partition_start = $4 AND partition_end = $5 \
         AND partition_shard = $6 AND state IN ('active', 'sealed') FOR UPDATE",
    )
    .bind(scope.organization_id.as_str())
    .bind(command.dataset_id.as_str())
    .bind(id_strings(ids))
    .bind(TimestampMicros(command.partition.start_micros))
    .bind(TimestampMicros(command.partition.end_micros))
    .bind(command.partition.shard as i16)
    .fetch_all(conn)
    .await
    .map_err(sqlx_err)?;

    if rows.len() != ids.len() {
        return Err(Error::conflict(
            "manifest retention inputs changed before generation switch",
        ));
    }

    Ok(())
}

fn id_strings(ids: &[crate::domain::storage::SegmentId]) -> Vec<String> {
    ids.iter().map(|id| id.as_str().to_owned()).collect()
}
