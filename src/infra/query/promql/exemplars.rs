// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Prometheus-native Exemplar queries over the metric stream sidecar rows.

use std::collections::{BTreeMap, BTreeSet};

use arrow::array::BooleanArray;

use super::*;
use crate::{
    domain::metrics::{
        METRIC_NAME_FIELD, MetricLabelSet, PROMETHEUS_EXEMPLAR_LABELS_FIELD,
        PROMETHEUS_EXEMPLAR_MARKER_FIELD, PROMETHEUS_EXEMPLAR_VALUE_FIELD, PrometheusExemplar,
        PrometheusExemplarQueryResult, PrometheusExemplarSeries, is_metric_identity_storage_field,
        is_prometheus_exemplar_storage_field,
    },
    shared::time::TimestampMicros,
};

const MAX_QUERY_EXEMPLARS: usize = 10_000;

type SelectorMatchers<'a> = BTreeMap<String, Vec<&'a Matchers>>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ExemplarKey {
    series_labels: Vec<(String, String)>,
    timestamp_micros: i64,
    value_bits: u64,
    labels: Vec<(String, String)>,
}

impl PromQLEngine {
    pub(super) async fn query_exemplars_inner(
        &self,
        req: QueryRequest,
    ) -> Result<PrometheusExemplarQueryResult> {
        if req.time_range.end < req.time_range.start {
            return Err(Error::invalid("exemplar query end must be >= start"));
        }

        let expr = parser::parse(&req.statement)
            .map_err(|error| Error::invalid(format!("promql parse: {error}")))?;
        self.ensure_metrics_queryable(&req, &expr).await?;

        let mut selectors = SelectorMatchers::new();
        collect_selector_matchers(&expr, &mut selectors)?;
        let limit = req
            .limit
            .unwrap_or(MAX_QUERY_EXEMPLARS)
            .clamp(1, MAX_QUERY_EXEMPLARS);
        let mut grouped: BTreeMap<MetricLabelSet, Vec<PrometheusExemplar>> = BTreeMap::new();
        let mut seen = BTreeSet::new();
        let mut truncated = false;

        'metrics: for (metric, matcher_groups) in selectors {
            let source = self.resolve_metric_source(&req.org_id, &metric).await?;
            let parquet_file_metas = self
                .metric_files(&req.org_id, &source.stream, req.time_range)
                .await?;
            if parquet_file_metas.is_empty() {
                continue;
            }

            let reader = ParquetReader::new(Arc::clone(&self.object_store));
            let projection = source.sample_columns.as_ref().map(|columns| {
                let mut columns = columns.clone();
                columns.extend([
                    PROMETHEUS_EXEMPLAR_MARKER_FIELD.to_string(),
                    PROMETHEUS_EXEMPLAR_VALUE_FIELD.to_string(),
                    PROMETHEUS_EXEMPLAR_LABELS_FIELD.to_string(),
                ]);
                columns.sort();
                columns.dedup();
                columns
            });
            let projection_refs = projection
                .as_ref()
                .map(|columns| columns.iter().map(String::as_str).collect::<Vec<_>>());
            for parquet_file_meta in parquet_file_metas {
                let mut options = ReadOptions::new()
                    .with_time_range(req.time_range.start.0, req.time_range.end.0)
                    .with_known_size(parquet_file_meta.size_bytes);
                if let Some(columns) = projection_refs.as_deref() {
                    options = options.with_columns(columns);
                }
                let batches = reader
                    .read_from_store(
                        self.object_store.clone(),
                        &parquet_file_meta.object_key,
                        options,
                    )
                    .await?;
                for batch in batches {
                    if append_batch_exemplars(
                        &batch,
                        &metric,
                        source.logical_metric.as_deref(),
                        &matcher_groups,
                        req.time_range,
                        limit,
                        &mut grouped,
                        &mut seen,
                    ) {
                        truncated = true;
                        break 'metrics;
                    }
                }
            }
        }

        let series = grouped
            .into_iter()
            .map(|(series_labels, mut exemplars)| {
                exemplars.sort_by(|left, right| {
                    left.timestamp
                        .cmp(&right.timestamp)
                        .then_with(|| left.labels.cmp(&right.labels))
                        .then_with(|| left.value.total_cmp(&right.value))
                });
                PrometheusExemplarSeries {
                    series_labels,
                    exemplars,
                }
            })
            .collect();
        Ok(PrometheusExemplarQueryResult { series, truncated })
    }
}

fn collect_selector_matchers<'a>(expr: &'a Expr, out: &mut SelectorMatchers<'a>) -> Result<()> {
    match expr {
        Expr::VectorSelector(selector) => collect_vector_selector(selector, out)?,
        Expr::MatrixSelector(selector) => collect_vector_selector(&selector.vs, out)?,
        Expr::Paren(paren) => collect_selector_matchers(&paren.expr, out)?,
        Expr::Unary(unary) => collect_selector_matchers(&unary.expr, out)?,
        Expr::Binary(binary) => {
            collect_selector_matchers(&binary.lhs, out)?;
            collect_selector_matchers(&binary.rhs, out)?;
        }
        Expr::Aggregate(aggregate) => {
            collect_selector_matchers(&aggregate.expr, out)?;
            if let Some(parameter) = &aggregate.param {
                collect_selector_matchers(parameter, out)?;
            }
        }
        Expr::Subquery(subquery) => collect_selector_matchers(&subquery.expr, out)?,
        Expr::Call(call) => {
            for argument in &call.args.args {
                collect_selector_matchers(argument, out)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn collect_vector_selector<'a>(
    selector: &'a VectorSelector,
    out: &mut SelectorMatchers<'a>,
) -> Result<()> {
    let metric = selector
        .name
        .as_ref()
        .ok_or_else(|| Error::invalid("exemplar queries require an explicit metric name"))?;
    out.entry(metric.clone())
        .or_default()
        .push(&selector.matchers);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_batch_exemplars(
    batch: &RecordBatch,
    metric: &str,
    logical_metric: Option<&str>,
    matcher_groups: &[&Matchers],
    time_range: TimeRange,
    limit: usize,
    grouped: &mut BTreeMap<MetricLabelSet, Vec<PrometheusExemplar>>,
    seen: &mut BTreeSet<ExemplarKey>,
) -> bool {
    let schema = batch.schema();
    let (Ok(timestamp_index), Ok(marker_index), Ok(value_index), Ok(labels_index)) = (
        schema.index_of("_timestamp"),
        schema.index_of(PROMETHEUS_EXEMPLAR_MARKER_FIELD),
        schema.index_of(PROMETHEUS_EXEMPLAR_VALUE_FIELD),
        schema.index_of(PROMETHEUS_EXEMPLAR_LABELS_FIELD),
    ) else {
        return false;
    };
    let Some(timestamps) = batch
        .column(timestamp_index)
        .as_any()
        .downcast_ref::<TimestampMicrosecondArray>()
    else {
        return false;
    };
    let Some(markers) = batch
        .column(marker_index)
        .as_any()
        .downcast_ref::<BooleanArray>()
    else {
        return false;
    };
    let Ok(values) = arrow::compute::cast(batch.column(value_index), &DataType::Float64) else {
        return false;
    };
    let Some(values) = values.as_any().downcast_ref::<Float64Array>() else {
        return false;
    };
    let Some(exemplar_labels) = batch
        .column(labels_index)
        .as_any()
        .downcast_ref::<StringArray>()
    else {
        return false;
    };
    let series_label_columns = schema
        .fields()
        .iter()
        .enumerate()
        .filter_map(|(index, field)| {
            let name = field.name().as_str();
            if name == "_timestamp"
                || name == "value"
                || (logical_metric.is_some() && is_metric_identity_storage_field(name))
                || is_prometheus_exemplar_storage_field(name)
                || !matches!(field.data_type(), DataType::Utf8)
            {
                return None;
            }
            batch
                .column(index)
                .as_any()
                .downcast_ref::<StringArray>()
                .map(|column| (name, column))
        })
        .collect::<Vec<_>>();
    let metric_names = logical_metric.and_then(|_| {
        schema
            .index_of(METRIC_NAME_FIELD)
            .ok()
            .and_then(|index| batch.column(index).as_any().downcast_ref::<StringArray>())
    });

    for row in 0..batch.num_rows() {
        if markers.is_null(row)
            || !markers.value(row)
            || timestamps.is_null(row)
            || values.is_null(row)
            || exemplar_labels.is_null(row)
        {
            continue;
        }
        if let Some(expected) = logical_metric
            && metric_names.is_none_or(|names| names.is_null(row) || names.value(row) != expected)
        {
            continue;
        }
        let timestamp = TimestampMicros(timestamps.value(row));
        if !time_range.contains(timestamp) {
            continue;
        }
        let mut series_labels = MetricLabelSet::new();
        series_labels.insert("__name__".into(), metric.into());
        for (name, column) in &series_label_columns {
            if !column.is_null(row) {
                series_labels.insert((*name).into(), column.value(row).into());
            }
        }
        if !matcher_groups
            .iter()
            .any(|matchers| matchers_match_labels(matchers, &series_labels))
        {
            continue;
        }
        let Ok(labels) = serde_json::from_str::<MetricLabelSet>(exemplar_labels.value(row)) else {
            continue;
        };
        let value = values.value(row);
        if !value.is_finite() {
            continue;
        }
        let key = ExemplarKey {
            series_labels: series_labels
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
            timestamp_micros: timestamp.0,
            value_bits: value.to_bits(),
            labels: labels
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
        };
        if seen.contains(&key) {
            continue;
        }
        if seen.len() >= limit {
            return true;
        }
        seen.insert(key);
        grouped
            .entry(series_labels)
            .or_default()
            .push(PrometheusExemplar {
                labels,
                value,
                timestamp,
            });
    }
    false
}

fn matchers_match_labels(matchers: &Matchers, labels: &MetricLabelSet) -> bool {
    let matches = |matcher: &Matcher| {
        let actual = labels.get(&matcher.name).map(String::as_str).unwrap_or("");
        matcher_matches_value(matcher, actual)
    };
    if !matchers.matchers.iter().all(matches) {
        return false;
    }
    for group in &matchers.or_matchers {
        if group.iter().all(matches) {
            return true;
        }
    }
    matchers.or_matchers.is_empty() || !matchers.matchers.is_empty()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{
        array::{BooleanArray, Float64Array, StringArray, TimestampMicrosecondArray},
        datatypes::{Field, Schema, TimeUnit},
    };

    use super::*;

    fn exemplar_batch() -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                "_timestamp",
                DataType::Timestamp(TimeUnit::Microsecond, None),
                false,
            ),
            Field::new(PROMETHEUS_EXEMPLAR_MARKER_FIELD, DataType::Boolean, true),
            Field::new(PROMETHEUS_EXEMPLAR_VALUE_FIELD, DataType::Float64, true),
            Field::new(PROMETHEUS_EXEMPLAR_LABELS_FIELD, DataType::Utf8, true),
            Field::new("service", DataType::Utf8, true),
        ]));
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(TimestampMicrosecondArray::from(vec![
                    10_000_000_i64,
                    20_000_000,
                    20_000_000,
                ])),
                Arc::new(BooleanArray::from(vec![true, true, true])),
                Arc::new(Float64Array::from(vec![0.25, 0.5, 0.5])),
                Arc::new(StringArray::from(vec![
                    r#"{"trace_id":"trace-a","span_id":"span-a"}"#,
                    r#"{"trace_id":"trace-b","span_id":"span-b"}"#,
                    r#"{"trace_id":"trace-b","span_id":"span-b"}"#,
                ])),
                Arc::new(StringArray::from(vec!["checkout", "payment", "payment"])),
            ],
        )
        .unwrap()
    }

    fn mixed_sample_and_exemplar_batch() -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                "_timestamp",
                DataType::Timestamp(TimeUnit::Microsecond, None),
                false,
            ),
            Field::new("value", DataType::Float64, true),
            Field::new(PROMETHEUS_EXEMPLAR_MARKER_FIELD, DataType::Boolean, true),
            Field::new(PROMETHEUS_EXEMPLAR_VALUE_FIELD, DataType::Float64, true),
            Field::new(PROMETHEUS_EXEMPLAR_LABELS_FIELD, DataType::Utf8, true),
            Field::new("service", DataType::Utf8, true),
        ]));
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(TimestampMicrosecondArray::from(vec![
                    10_000_000_i64,
                    10_000_000,
                ])),
                Arc::new(Float64Array::from(vec![Some(42.0), None])),
                Arc::new(BooleanArray::from(vec![None, Some(true)])),
                Arc::new(Float64Array::from(vec![None, Some(0.25)])),
                Arc::new(StringArray::from(vec![
                    None,
                    Some(r#"{"trace_id":"trace-a"}"#),
                ])),
                Arc::new(StringArray::from(vec!["checkout", "checkout"])),
            ],
        )
        .unwrap()
    }

    fn selector(query: &str) -> VectorSelector {
        match parser::parse(query).unwrap() {
            Expr::VectorSelector(selector) => selector,
            other => panic!("expected vector selector, got {other:?}"),
        }
    }

    #[test]
    fn batch_extraction_matches_series_labels_and_deduplicates_retries() {
        let selector = selector(r#"http_request_duration_seconds{service="payment"}"#);
        let matcher_groups = [&selector.matchers];
        let mut grouped = BTreeMap::new();
        let mut seen = BTreeSet::new();

        let truncated = append_batch_exemplars(
            &exemplar_batch(),
            "http_request_duration_seconds",
            None,
            &matcher_groups,
            TimeRange::new(TimestampMicros(0), TimestampMicros(30_000_000)),
            10,
            &mut grouped,
            &mut seen,
        );

        assert!(!truncated);
        assert_eq!(seen.len(), 1, "duplicate remote-write retry is collapsed");
        let (labels, exemplars) = grouped.into_iter().next().unwrap();
        assert_eq!(
            labels.get("__name__").map(String::as_str),
            Some("http_request_duration_seconds")
        );
        assert_eq!(labels.get("service").map(String::as_str), Some("payment"));
        assert_eq!(
            exemplars[0].labels.get("trace_id").map(String::as_str),
            Some("trace-b")
        );
    }

    #[test]
    fn nested_promql_collects_every_explicit_metric_selector() {
        let expr = parser::parse(
            r#"sum(rate(http_requests_total{service="checkout"}[5m])) / process_cpu_seconds_total"#,
        )
        .unwrap();
        let mut selectors = SelectorMatchers::new();
        collect_selector_matchers(&expr, &mut selectors).unwrap();
        assert_eq!(
            selectors.keys().cloned().collect::<Vec<_>>(),
            vec![
                "http_requests_total".to_string(),
                "process_cpu_seconds_total".to_string()
            ]
        );
    }

    #[test]
    fn ordinary_promql_ignores_the_exemplar_sidecar_row() {
        let series = batches_to_series(
            &[mixed_sample_and_exemplar_batch()],
            &Matchers::empty(),
            None,
            0,
            20_000_000,
            10,
        )
        .unwrap();
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].samples, vec![(10_000_000, 42.0)]);
    }
}
