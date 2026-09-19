// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Logical-to-physical metric resolution for container-style system metrics.

use arrow::array::RecordBatch;
use futures::future::try_join_all;

use super::*;
use crate::{
    domain::{
        metrics::{
            METRIC_NAME_FIELD, is_metric_identity_storage_field,
            is_metric_query_semantics_storage_field,
        },
        storage::QueryFile,
        stream::{FieldType, MOLESIGNAL_SYSTEM_STREAM, StreamDefinition},
    },
    shared::{ids::Id, time::TimeRange},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedMetricSource {
    /// Physical metrics stream used for file lookup.
    pub(super) stream: String,
    /// Logical metric stored in the `metric_name` discriminator column.
    pub(super) logical_metric: Option<String>,
    /// Resolved physical stream identity, when a stream repository is present.
    pub(super) stream_id: Option<crate::shared::ids::Id>,
    /// Parquet sample scan projection: timestamp, value, labels and internal
    /// temporality metadata required by the evaluator.
    pub(super) sample_columns: Option<Vec<String>>,
}

pub(super) struct MetricStorageSnapshot {
    pub(super) files: Vec<QueryFile>,
    pub(super) buffered_batches: Vec<RecordBatch>,
}

impl ResolvedMetricSource {
    fn direct(metric: &str) -> Self {
        Self {
            stream: metric.to_string(),
            logical_metric: None,
            stream_id: None,
            sample_columns: None,
        }
    }
}

fn sample_columns(stream: &StreamDefinition, container: bool) -> Vec<String> {
    let mut columns = vec!["_timestamp".to_string(), "value".to_string()];
    columns.extend(stream.schema.fields.iter().filter_map(|field| {
        let query_semantics = is_metric_query_semantics_storage_field(&field.name);
        let logical_discriminator = container && field.name == METRIC_NAME_FIELD;
        let hidden_metadata = is_metric_identity_storage_field(&field.name)
            && !query_semantics
            && !logical_discriminator;
        let supported = matches!(field.data_type, FieldType::Utf8 | FieldType::Json)
            || logical_discriminator
            || query_semantics;
        if field.name == "value"
            || hidden_metadata
            || crate::domain::metrics::is_prometheus_exemplar_storage_field(&field.name)
            || !supported
        {
            return None;
        }
        Some(field.name.clone())
    }));
    columns.sort();
    columns.dedup();
    columns
}

impl PromQLEngine {
    /// 把 metrics 的 hot/raw 与已被 compactor 原子替换的 rollup 合并为
    /// 一个逻辑数据集。两类 QueryFile 并行查询，避免给每个 PromQL
    /// selector 串行增加两次 PG 往返。
    pub(super) async fn metric_files(
        &self,
        org_id: &Id,
        stream: &str,
        time_range: TimeRange,
    ) -> Result<MetricStorageSnapshot> {
        let dataset_types =
            crate::domain::storage::logical_query_dataset_types(StreamType::METRICS)?;
        let (mut files, buffered_batches) =
            if let (Some(source), Some(streams)) = (&self.catalog_source, &self.streams) {
                match streams.get(org_id, stream, StreamType::METRICS).await {
                    Ok(definition) => {
                        let snapshot = source
                            .snapshot_stream(&definition, &dataset_types, time_range)
                            .await?;
                        (snapshot.files(), snapshot.buffered_batches())
                    }
                    Err(Error::NotFound(_)) => (Vec::new(), Vec::new()),
                    Err(error) => return Err(error),
                }
            } else {
                let lookups = dataset_types.into_iter().map(|dataset_type| {
                    self.files.find_dataset(
                        org_id,
                        stream,
                        StreamType::METRICS,
                        dataset_type,
                        time_range,
                    )
                });
                (
                    try_join_all(lookups)
                        .await?
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>(),
                    Vec::new(),
                )
            };
        files.sort_by(|left, right| {
            left.time_range
                .start
                .cmp(&right.time_range.start)
                .then_with(|| left.id.0.cmp(&right.id.0))
        });
        files.dedup_by(|left, right| left.id == right.id);
        Ok(MetricStorageSnapshot {
            files,
            buffered_batches,
        })
    }

    /// Resolve a PromQL metric identifier to its physical stream.
    ///
    /// Ordinary metrics keep the historical one-stream-per-metric behavior.
    /// `_sys` self telemetry is intentionally stored in the protected
    /// `_molesignal` container, so an otherwise missing logical metric falls
    /// back to that stream and is filtered by its `metric_name` column.
    pub(super) async fn resolve_metric_source(
        &self,
        org_id: &crate::shared::ids::Id,
        metric: &str,
    ) -> Result<ResolvedMetricSource> {
        let Some(streams) = &self.streams else {
            return Ok(ResolvedMetricSource::direct(metric));
        };

        if metric == MOLESIGNAL_SYSTEM_STREAM {
            return match streams.get(org_id, metric, StreamType::METRICS).await {
                Ok(definition) => Ok(ResolvedMetricSource {
                    stream: metric.to_string(),
                    logical_metric: None,
                    sample_columns: Some(sample_columns(&definition, false)),
                    stream_id: Some(definition.id),
                }),
                Err(Error::NotFound(_)) => Ok(ResolvedMetricSource::direct(metric)),
                Err(error) => Err(error),
            };
        }

        match streams.get(org_id, metric, StreamType::METRICS).await {
            Ok(definition) => {
                return Ok(ResolvedMetricSource {
                    stream: metric.to_string(),
                    logical_metric: None,
                    sample_columns: Some(sample_columns(&definition, false)),
                    stream_id: Some(definition.id),
                });
            }
            Err(Error::NotFound(_)) => {}
            Err(error) => return Err(error),
        }

        match streams
            .get(org_id, MOLESIGNAL_SYSTEM_STREAM, StreamType::METRICS)
            .await
        {
            Ok(definition) => Ok(ResolvedMetricSource {
                stream: MOLESIGNAL_SYSTEM_STREAM.to_string(),
                logical_metric: Some(metric.to_string()),
                sample_columns: Some(sample_columns(&definition, true)),
                stream_id: Some(definition.id),
            }),
            Err(Error::NotFound(_)) => Ok(ResolvedMetricSource::direct(metric)),
            Err(error) => Err(error),
        }
    }
}
