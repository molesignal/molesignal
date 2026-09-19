// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Metrics raw-to-rollup transformation.
//!
//! One cold partition is read from a repeatable-read Catalog snapshot, including its immutable
//! sealed base and hot overlay. Publication retires that exact manifest generation and every
//! selected raw Segment while making the rollup Segment visible in the same PostgreSQL
//! transaction. A concurrent compaction, late manifest fold, or retention pass therefore turns
//! into a conflict and the unpublished rollup objects are removed.

use std::{collections::HashMap, sync::OnceLock};

use arrow::{compute::concat_batches, record_batch::RecordBatch};
use prometheus::IntCounter;

use super::{Compactor, DAY_MICROS, failures, valid_primary};
use crate::{
    domain::{
        storage::{
            DataSegment, DatasetSelection, DatasetState, DatasetTypeId, OrganizationScope,
            Partition, PartitionManifestPointer, PhysicalDataset, PublishDatasetTransform,
            builtin_registry, primary_dataset_type, type_id,
        },
        stream::{StreamDefinition, StreamType},
    },
    infra::storage::downsample::downsample_batch,
    shared::{
        Error, Result,
        metrics::register_int_counter,
        time::{TimeRange, TimestampMicros},
    },
};

struct PartitionInput {
    manifest: Option<PartitionManifestPointer>,
    segments: Vec<DataSegment>,
}

pub(super) async fn sweep(compactor: &Compactor, stream: &StreamDefinition) -> Result<usize> {
    if compactor.settings.downsample_after_days == 0
        || compactor.settings.downsample_interval_secs == 0
        || stream.stream_type != StreamType::METRICS
    {
        return Ok(0);
    }
    let (raw, rollup) = resolve_datasets(compactor, stream).await?;
    let cutoff = TimestampMicros::now().0.saturating_sub(
        i64::from(compactor.settings.downsample_after_days).saturating_mul(DAY_MICROS),
    );
    let mut partitions = snapshot_partitions(compactor, stream, &raw, cutoff).await?;
    partitions.sort_by_key(|(partition, _)| {
        (
            partition.end_micros,
            partition.start_micros,
            partition.shard,
        )
    });

    let mut completed = 0;
    for (partition, input) in partitions {
        match transform_partition(compactor, stream, &raw, &rollup, partition, input).await {
            Ok(true) => {
                completed += 1;
                downsampled().inc();
            }
            Ok(false) => {}
            Err(error) => {
                failures().with_label_values(&["downsample"]).inc();
                tracing::warn!(
                    stream = %stream.name,
                    raw_dataset_id = %raw.id,
                    rollup_dataset_id = %rollup.id,
                    partition_start_micros = partition.start_micros,
                    %error,
                    "metrics downsample partition failed; will retry next sweep"
                );
            }
        }
    }
    Ok(completed)
}

async fn resolve_datasets(
    compactor: &Compactor,
    stream: &StreamDefinition,
) -> Result<(PhysicalDataset, PhysicalDataset)> {
    let scope = OrganizationScope::new(stream.org_id.clone());
    let raw_type = primary_dataset_type(stream.stream_type)?;
    let rollup_type = DatasetTypeId::builtin(type_id::builtin::DATASET_METRIC_ROLLUP);
    let datasets = compactor
        .catalog
        .list_datasets(&scope, &stream.id)
        .await?
        .into_iter()
        .filter(|dataset| dataset.state == DatasetState::Active)
        .collect::<Vec<_>>();
    let raw = datasets
        .iter()
        .find(|dataset| dataset.dataset_type == raw_type)
        .cloned()
        .ok_or_else(|| Error::not_found(format!("metrics raw dataset for stream {}", stream.id)))?;
    let rollup = match datasets
        .iter()
        .find(|dataset| dataset.dataset_type == rollup_type)
        .cloned()
    {
        Some(dataset) => dataset,
        None => {
            let registry = builtin_registry();
            let spec = registry
                .dataset_type(&stream.stream_type, &rollup_type)?
                .to_spec();
            compactor
                .catalog
                .ensure_datasets(&scope, &stream.id, &[spec])
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| Error::internal("rollup dataset ensure returned no dataset"))?
        }
    };
    Ok((raw, rollup))
}

async fn snapshot_partitions(
    compactor: &Compactor,
    stream: &StreamDefinition,
    raw: &PhysicalDataset,
    cutoff: i64,
) -> Result<Vec<(Partition, PartitionInput)>> {
    let snapshot = compactor
        .catalog
        .snapshot(
            &OrganizationScope::new(stream.org_id.clone()),
            DatasetSelection {
                dataset_ids: vec![raw.id.clone()],
                time_range: TimeRange::new(TimestampMicros(i64::MIN), TimestampMicros(cutoff)),
                partition_shard: None,
            },
        )
        .await?;
    let dataset = snapshot
        .dataset(&raw.id)
        .ok_or_else(|| Error::internal(format!("catalog omitted dataset {}", raw.id)))?;
    let mut work = HashMap::<Partition, PartitionInput>::new();
    for pointer in &dataset.manifests {
        if pointer.partition.end_micros > cutoff {
            continue;
        }
        let manifest = compactor.manifest_reader.load(pointer).await?;
        work.insert(
            pointer.partition,
            PartitionInput {
                manifest: Some(pointer.clone()),
                segments: manifest.segments.clone(),
            },
        );
    }
    for segment in &dataset.segments {
        if segment.partition.end_micros > cutoff {
            continue;
        }
        work.entry(segment.partition)
            .or_insert_with(|| PartitionInput {
                manifest: None,
                segments: Vec::new(),
            })
            .segments
            .push(segment.clone());
    }
    Ok(work.into_iter().collect())
}

async fn transform_partition(
    compactor: &Compactor,
    stream: &StreamDefinition,
    raw: &PhysicalDataset,
    rollup: &PhysicalDataset,
    partition: Partition,
    input: PartitionInput,
) -> Result<bool> {
    if input.segments.is_empty() {
        return Ok(false);
    }
    if let Some(segment) = input
        .segments
        .iter()
        .find(|segment| !valid_primary(segment))
    {
        return Err(Error::internal(format!(
            "downsample input segment {} has no supported ready Parquet primary Artifact",
            segment.id
        )));
    }

    let mut batches = Vec::new();
    for segment in &input.segments {
        compactor
            .object_reader
            .register_segment(&stream.org_id, segment)?;
        batches.extend(
            compactor
                .reader
                .read_all(segment.primary.object.key.as_str())
                .await?,
        );
    }
    if batches.is_empty() {
        return Err(Error::internal(
            "downsample input produced no record batches",
        ));
    }
    let raw_stream = crate::infra::intake::physical_schema::project(stream, &raw.dataset_type);
    let raw_schema = crate::infra::storage::arrow_schema::to_arrow(&raw_stream.schema);
    let aligned = batches
        .iter()
        .map(|batch| {
            crate::infra::storage::arrow_schema::align_batch_to_schema(batch, &raw_schema)
                .map_err(|error| Error::internal(format!("downsample align schema: {error}")))
        })
        .collect::<Result<Vec<RecordBatch>>>()?;
    let merged = concat_batches(&raw_schema, &aligned)
        .map_err(|error| Error::internal(format!("downsample concat batches: {error}")))?;
    let reduced = downsample_batch(merged, compactor.settings.downsample_interval_secs).await?;
    let rollup_stream =
        crate::infra::intake::physical_schema::project(stream, &rollup.dataset_type);
    let outputs = compactor
        .writer
        .write_compaction_catalog(&rollup_stream, rollup, reduced)
        .await?;
    if outputs.is_empty() || outputs.iter().any(|segment| segment.partition != partition) {
        compactor.writer.delete_catalog_outputs(&outputs).await;
        return Err(Error::internal(
            "downsample output escaped its input partition",
        ));
    }

    let command = PublishDatasetTransform {
        input_dataset_id: raw.id.clone(),
        input_partition: partition,
        input_manifest: input.manifest,
        input_segment_ids: input
            .segments
            .iter()
            .map(|segment| segment.id.clone())
            .collect(),
        output_dataset_id: rollup.id.clone(),
        output_segments: outputs.clone(),
        gc_not_before_micros: compactor.gc_not_before(),
    };
    if let Err(error) = compactor
        .catalog
        .publish_dataset_transform(&OrganizationScope::new(stream.org_id.clone()), command)
        .await
    {
        compactor.writer.delete_catalog_outputs(&outputs).await;
        return Err(error);
    }
    Ok(true)
}

fn downsampled() -> &'static IntCounter {
    static METRIC: OnceLock<IntCounter> = OnceLock::new();
    METRIC.get_or_init(|| {
        register_int_counter(
            "compactor_downsampled_partitions_total",
            "metrics partitions atomically transformed from raw to rollup",
        )
    })
}
