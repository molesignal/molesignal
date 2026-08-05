// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Prometheus HTTP API compatibility for native Exemplar queries.

use std::collections::BTreeMap;

use axum::{
    Extension, Form, Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::permission,
        metrics::{
            PrometheusExemplar as DomainExemplar, PrometheusExemplarSeries as DomainExemplarSeries,
        },
        query::{QueryLanguage, QueryRequest},
    },
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

const MAX_EXEMPLARS_PER_QUERY: usize = 10_000;

pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/prometheus/api/v1/query_exemplars",
        get(query_exemplars_get).post(query_exemplars_post),
    )
}

#[derive(Debug, Clone, Deserialize)]
struct ExemplarQueryParams {
    query: String,
    start: String,
    end: String,
}

#[derive(Debug, Serialize)]
struct PrometheusResponse<T> {
    status: &'static str,
    data: T,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExemplarSeries {
    series_labels: BTreeMap<String, String>,
    exemplars: Vec<Exemplar>,
}

#[derive(Debug, Serialize)]
struct Exemplar {
    labels: BTreeMap<String, String>,
    value: f64,
    /// Prometheus HTTP API 使用 epoch seconds（可带小数）。
    timestamp: f64,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn query_exemplars_get(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<ExemplarQueryParams>,
) -> Result<Json<PrometheusResponse<Vec<ExemplarSeries>>>> {
    query_exemplars(state, ctx, params).await
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn query_exemplars_post(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Form(params): Form<ExemplarQueryParams>,
) -> Result<Json<PrometheusResponse<Vec<ExemplarSeries>>>> {
    query_exemplars(state, ctx, params).await
}

async fn query_exemplars(
    state: AppState,
    ctx: IamContext,
    params: ExemplarQueryParams,
) -> Result<Json<PrometheusResponse<Vec<ExemplarSeries>>>> {
    if params.query.trim().is_empty() {
        return Err(Error::invalid(
            "Prometheus exemplar query must not be empty",
        ));
    }
    let start = parse_prometheus_time(&params.start, "start")?;
    let end = parse_prometheus_time(&params.end, "end")?;
    if end < start {
        return Err(Error::invalid(
            "Prometheus exemplar query end must be >= start",
        ));
    }
    let role_key = ctx.organization_role_key().to_string();
    let result = state
        .query
        .run_exemplars(
            QueryRequest {
                org_id: ctx.org_id,
                language: QueryLanguage::Promql,
                statement: params.query,
                time_range: TimeRange::new(start, end),
                stream: None,
                limit: Some(MAX_EXEMPLARS_PER_QUERY),
                federation_clusters: Vec::new(),
            },
            &role_key,
        )
        .await?;
    let warnings = result
        .truncated
        .then(|| format!("exemplar result truncated at {MAX_EXEMPLARS_PER_QUERY} unique exemplars"))
        .into_iter()
        .collect();
    Ok(Json(PrometheusResponse {
        status: "success",
        data: result
            .series
            .into_iter()
            .map(ExemplarSeries::from)
            .collect(),
        warnings,
    }))
}

fn parse_prometheus_time(value: &str, field: &str) -> Result<TimestampMicros> {
    if let Ok(seconds) = value.parse::<f64>() {
        let micros = seconds * 1_000_000.0;
        if !micros.is_finite() || micros < i64::MIN as f64 || micros > i64::MAX as f64 {
            return Err(Error::invalid(format!(
                "Prometheus exemplar {field} is outside the supported timestamp range"
            )));
        }
        return Ok(TimestampMicros(micros.round() as i64));
    }
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|timestamp| TimestampMicros(timestamp.timestamp_micros()))
        .map_err(|_| {
            Error::invalid(format!(
                "Prometheus exemplar {field} must be epoch seconds or RFC3339"
            ))
        })
}

impl From<DomainExemplarSeries> for ExemplarSeries {
    fn from(value: DomainExemplarSeries) -> Self {
        Self {
            series_labels: value.series_labels,
            exemplars: value.exemplars.into_iter().map(Exemplar::from).collect(),
        }
    }
}

impl From<DomainExemplar> for Exemplar {
    fn from(value: DomainExemplar) -> Self {
        Self {
            labels: value.labels,
            value: value.value,
            timestamp: value.timestamp.0 as f64 / 1_000_000.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::metrics::{MetricLabelSet, PrometheusExemplarSeries};

    #[test]
    fn parses_epoch_seconds_and_rfc3339() {
        assert_eq!(
            parse_prometheus_time("1700000000.125", "start").unwrap(),
            TimestampMicros(1_700_000_000_125_000)
        );
        assert_eq!(
            parse_prometheus_time("2023-11-14T22:13:20.125Z", "end").unwrap(),
            TimestampMicros(1_700_000_000_125_000)
        );
    }

    #[test]
    fn rejects_non_finite_and_invalid_timestamps() {
        assert!(parse_prometheus_time("NaN", "start").is_err());
        assert!(parse_prometheus_time("not-a-time", "end").is_err());
    }

    #[test]
    fn serializes_the_native_prometheus_exemplar_shape() {
        let response = PrometheusResponse {
            status: "success",
            data: vec![ExemplarSeries::from(PrometheusExemplarSeries {
                series_labels: MetricLabelSet::from([
                    ("__name__".into(), "latency_seconds".into()),
                    ("service".into(), "checkout".into()),
                ]),
                exemplars: vec![DomainExemplar {
                    labels: MetricLabelSet::from([("trace_id".into(), "trace-a".into())]),
                    value: 0.5,
                    timestamp: TimestampMicros(1_500_000),
                }],
            })],
            warnings: Vec::new(),
        };

        let value = serde_json::to_value(response).unwrap();
        assert_eq!(value["status"], "success");
        assert_eq!(value["data"][0]["seriesLabels"]["service"], "checkout");
        assert_eq!(value["data"][0]["exemplars"][0]["timestamp"], 1.5);
        assert!(value.get("warnings").is_none());
    }
}
