// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Which streams a selector reads. A metrics stream holds the metrics of one source, told apart
//! by `metric_name`; the metric directory says which streams hold a metric and in which hours.

use std::{collections::BTreeMap, time::Duration};

use arrow::array::RecordBatch;
use futures::future::try_join_all;
use moka::future::Cache;

use super::*;
use crate::{
    domain::{
        metrics::{
            METRIC_NAME_FIELD, MetricDirectory, MetricDirectoryEntry,
            is_metric_identity_storage_field, is_metric_query_semantics_storage_field,
            is_prometheus_exemplar_storage_field,
        },
        storage::QueryFile,
        stream::{FieldType, StreamDefinition, StreamSettings},
    },
    shared::{
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

/// How long a directory answer and a stream's settings are reused.
const REUSE_FOR: Duration = Duration::from_secs(10);

/// A metric's last hour is taken to last one more: a reused directory answer may not know yet
/// that the metric reached the next hour.
const LAST_HOUR_SLACK_US: i64 = 3_600 * 1_000_000;

/// Which metrics a selector reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MetricFilter {
    /// One metric: `up`, `{"up"}` or `{__name__="up"}`.
    Name(String),
    /// Every metric its `__name__` matchers accept, none meaning every metric: `{job="api"}` or
    /// `{__name__=~"http_.*"}`. Its series carry `__name__`.
    Matching(Vec<Matcher>),
}

impl MetricFilter {
    pub(super) fn of(vs: &VectorSelector) -> Self {
        if let Some(name) = &vs.name {
            return Self::Name(name.clone());
        }

        let names = vs
            .matchers
            .matchers
            .iter()
            .filter(|matcher| matcher.name == "__name__")
            .cloned()
            .collect::<Vec<_>>();

        match names.as_slice() {
            [matcher] if matcher.op == MatchOp::Equal => Self::Name(matcher.value.clone()),
            _ => Self::Matching(names),
        }
    }

    pub(super) fn name(&self) -> Option<&str> {
        match self {
            Self::Name(name) => Some(name),
            Self::Matching(_) => None,
        }
    }

    pub(super) fn accepts(&self, metric: &str) -> bool {
        match self {
            Self::Name(name) => name == metric,
            Self::Matching(matchers) => matchers
                .iter()
                .all(|matcher| matcher_matches_value(matcher, metric)),
        }
    }
}

/// The directory of an engine, with its answers and the settings of the streams it names reused
/// for a few seconds: one query asks for the same metric several times.
pub(super) struct MetricSources {
    directory: Arc<dyn MetricDirectory>,
    /// By organization and metric; no metric holds every metric of the organization.
    entries: Cache<(String, Option<String>), Arc<Vec<MetricDirectoryEntry>>>,
    settings: Cache<String, StreamSettings>,
}

impl MetricSources {
    pub(super) fn new(directory: Arc<dyn MetricDirectory>) -> Self {
        Self {
            directory,
            entries: Cache::builder()
                .max_capacity(100_000)
                .time_to_live(REUSE_FOR)
                .build(),
            settings: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(REUSE_FOR)
                .build(),
        }
    }

    /// An empty answer is not kept, so a metric is readable as soon as it is recorded.
    async fn entries(
        &self,
        org_id: &Id,
        filter: &MetricFilter,
    ) -> Result<Arc<Vec<MetricDirectoryEntry>>> {
        let key = (org_id.0.clone(), filter.name().map(str::to_string));

        if let Some(entries) = self.entries.get(&key).await {
            return Ok(entries);
        }

        let entries = Arc::new(match filter.name() {
            Some(name) => self.directory.find(org_id, name).await?,
            None => self.directory.list(org_id).await?,
        });

        if !entries.is_empty() {
            self.entries.insert(key, entries.clone()).await;
        }

        Ok(entries)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedMetricSource {
    /// Metrics stream to list the files of.
    pub(super) stream: String,
    /// Parquet sample scan projection: timestamp, value, labels, `metric_name` and the
    /// temporality metadata the evaluator needs. `None` reads every column.
    pub(super) sample_columns: Option<Vec<String>>,
}

pub(super) struct MetricStorageSnapshot {
    pub(super) files: Vec<QueryFile>,
    pub(super) buffered_batches: Vec<RecordBatch>,
}

fn sample_columns(stream: &StreamDefinition) -> Vec<String> {
    let mut columns = vec!["_timestamp".to_string(), "value".to_string()];

    columns.extend(stream.schema.fields.iter().filter_map(|field| {
        let kept =
            field.name == METRIC_NAME_FIELD || is_metric_query_semantics_storage_field(&field.name);

        let hidden = field.name == "value"
            || is_prometheus_exemplar_storage_field(&field.name)
            || (is_metric_identity_storage_field(&field.name) && !kept);

        let label = matches!(field.data_type, FieldType::Utf8 | FieldType::Json);

        (!hidden && (label || kept)).then(|| field.name.clone())
    }));

    columns.sort();
    columns.dedup();
    columns
}

/// The metric filter of every selector of `expr`, each once.
pub(super) fn selector_filters(expr: &Expr) -> Vec<MetricFilter> {
    let mut filters = Vec::new();
    collect_selector_filters(expr, &mut filters);
    filters
}

fn collect_selector_filters(expr: &Expr, out: &mut Vec<MetricFilter>) {
    match expr {
        Expr::VectorSelector(vs) => push_filter(vs, out),
        Expr::MatrixSelector(ms) => push_filter(&ms.vs, out),
        Expr::Paren(p) => collect_selector_filters(&p.expr, out),
        Expr::Unary(u) => collect_selector_filters(&u.expr, out),
        Expr::Binary(b) => {
            collect_selector_filters(&b.lhs, out);
            collect_selector_filters(&b.rhs, out);
        }
        Expr::Aggregate(a) => {
            collect_selector_filters(&a.expr, out);

            if let Some(param) = &a.param {
                collect_selector_filters(param, out);
            }
        }
        Expr::Subquery(sq) => collect_selector_filters(&sq.expr, out),
        Expr::Call(c) => {
            for arg in &c.args.args {
                collect_selector_filters(arg, out);
            }
        }
        _ => {}
    }
}

fn push_filter(vs: &VectorSelector, out: &mut Vec<MetricFilter>) {
    let filter = MetricFilter::of(vs);

    if !out.contains(&filter) {
        out.push(filter);
    }
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

    /// The streams that may hold the filter's metrics in `window`, by name.
    async fn holding_streams(
        &self,
        org_id: &Id,
        filter: &MetricFilter,
        window: TimeRange,
    ) -> Result<BTreeMap<String, Id>> {
        let Some(sources) = &self.metric_sources else {
            return Ok(BTreeMap::new());
        };

        let window = TimeRange::new(
            TimestampMicros(window.start.0.saturating_sub(LAST_HOUR_SLACK_US)),
            window.end,
        );

        Ok(sources
            .entries(org_id, filter)
            .await?
            .iter()
            .filter(|entry| filter.accepts(&entry.metric_name) && entry.overlaps(window))
            .map(|entry| (entry.stream_name.clone(), entry.stream_id.clone()))
            .collect())
    }

    async fn stream_settings(&self, stream_id: &Id) -> Result<StreamSettings> {
        let Some(streams) = &self.streams else {
            return Ok(StreamSettings::default());
        };

        let Some(sources) = &self.metric_sources else {
            return streams.get_settings(stream_id).await;
        };

        if let Some(settings) = sources.settings.get(&stream_id.0).await {
            return Ok(settings);
        }

        let settings = streams.get_settings(stream_id).await?;

        sources
            .settings
            .insert(stream_id.0.clone(), settings.clone())
            .await;

        Ok(settings)
    }

    /// The queryable streams that may hold the filter's metrics in `window`, by name.
    pub(super) async fn metric_sources(
        &self,
        org_id: &Id,
        filter: &MetricFilter,
        window: TimeRange,
    ) -> Result<Vec<ResolvedMetricSource>> {
        let streams = self.holding_streams(org_id, filter, window).await?;
        let mut sources = Vec::with_capacity(streams.len());

        for (stream, stream_id) in streams {
            let Some(repository) = &self.streams else {
                sources.push(ResolvedMetricSource {
                    stream,
                    sample_columns: None,
                });

                continue;
            };

            if !self.stream_settings(&stream_id).await?.queryable {
                continue;
            }

            match repository.get(org_id, &stream, StreamType::METRICS).await {
                Ok(definition) => sources.push(ResolvedMetricSource {
                    sample_columns: Some(sample_columns(&definition)),
                    stream,
                }),
                Err(Error::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }

        Ok(sources)
    }

    /// Checks the streams every selector of `expr` reads: a selector whose metrics sit only in
    /// streams that may not be queried is refused, and so is a time range longer than a stream
    /// holding them allows. A selector no stream holds reads nothing, as in Prometheus.
    pub(super) async fn ensure_metrics_queryable(
        &self,
        req: &QueryRequest,
        expr: &Expr,
    ) -> Result<()> {
        if self.streams.is_none() {
            return Ok(());
        }

        for filter in selector_filters(expr) {
            let streams = self
                .holding_streams(&req.org_id, &filter, req.time_range)
                .await?;

            let mut queryable = 0;

            for (stream, stream_id) in &streams {
                let settings = self.stream_settings(stream_id).await?;

                if !settings.queryable {
                    continue;
                }

                queryable += 1;

                if let Some(hours) = settings.exceeded_query_range(req.time_range) {
                    return Err(crate::infra::query::planner::query_range_error(
                        stream, hours,
                    ));
                }
            }

            if !streams.is_empty() && queryable == 0 {
                let names = streams.keys().cloned().collect::<Vec<_>>().join(", ");
                return Err(Error::forbidden(format!(
                    "stream is not queryable: {names}"
                )));
            }
        }

        Ok(())
    }
}
