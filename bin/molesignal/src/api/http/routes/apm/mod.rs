// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Organization-scoped APM HTTP transport.

use std::time::Instant;

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::Deserialize;

use crate::{
    api::AppState,
    app::{
        apm::{
            ApmQueryRequest, ApmTenantHealthResponse, DependencySummary, ErrorDetailResponse,
            ErrorSummary, OverviewResponse, PagedResponse, ServiceDetailResponse, ServiceSummary,
            TransactionDetailResponse, TransactionSummary, VersionCompareResponse,
            record_apm_query_latency,
        },
        iam::IamContext,
    },
    domain::{
        apm::{QueryResolution, SortDirection, TransactionKind},
        iam::permission,
    },
    shared::{Result, time::TimestampMicros},
};

mod cursor;

const DAY_MICROS: i64 = 86_400_000_000;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/apm/overview", get(overview))
        .route("/apm/services", get(services))
        .route("/apm/services/{service}", get(service_detail))
        .route("/apm/transactions", get(transactions))
        .route("/apm/transactions/{transaction}", get(transaction_detail))
        .route("/apm/dependencies", get(dependencies))
        .route("/apm/errors", get(errors))
        .route("/apm/errors/{fingerprint}", get(error_detail))
        .route("/apm/versions/compare", get(version_compare))
        .route("/apm/health", get(health))
}

#[derive(Debug, Clone, Default, Deserialize)]
struct QueryParams {
    from: Option<i64>,
    to: Option<i64>,
    namespace: Option<String>,
    service: Option<String>,
    environment: Option<String>,
    version: Option<String>,
    resolution: Option<QueryResolution>,
    sort: Option<String>,
    direction: Option<SortDirection>,
    limit: Option<usize>,
    cursor: Option<String>,
    baseline: Option<String>,
    candidate: Option<String>,
    kind: Option<TransactionKind>,
}

impl QueryParams {
    fn request(&self) -> ApmQueryRequest {
        let to = self.to.unwrap_or_else(|| TimestampMicros::now().0);
        ApmQueryRequest {
            from: self.from.unwrap_or_else(|| to.saturating_sub(DAY_MICROS)),
            to,
            namespace: self.namespace.clone(),
            service: self.service.clone(),
            environment: self.environment.clone(),
            version: self.version.clone(),
            resolution: self.resolution.unwrap_or_default(),
            sort: self.sort.clone(),
            direction: self.direction,
            limit: self.limit,
            cursor: self.cursor.clone(),
        }
    }
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn overview(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<QueryParams>,
) -> Result<Json<OverviewResponse>> {
    let started = Instant::now();
    let context = state.telemetry.apm_query.context(
        ctx.org_id,
        params.request(),
        &["request_count", "error_rate", "p95", "total_time"],
        "total_time",
    )?;
    let response = state.telemetry.apm_query.overview(&context).await?;
    record_apm_query_latency("overview", started.elapsed());
    Ok(Json(response))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn services(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<QueryParams>,
) -> Result<Json<PagedResponse<ServiceSummary>>> {
    let started = Instant::now();
    let request = cursor::request(&state, &ctx, &params, cursor::SERVICES)?;
    let context = state.telemetry.apm_query.context(
        ctx.org_id.clone(),
        request,
        &["request_count", "error_rate", "p95", "name"],
        "request_count",
    )?;
    let response = state.telemetry.apm_query.services(&context).await?;
    let response = cursor::sign_page(&state, &ctx, cursor::SERVICES, &context, response)?;
    record_apm_query_latency("services", started.elapsed());
    Ok(Json(response))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn service_detail(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(service): Path<String>,
    Query(mut params): Query<QueryParams>,
) -> Result<Json<ServiceDetailResponse>> {
    let started = Instant::now();
    params.service = Some(service);
    let context = state.telemetry.apm_query.context(
        ctx.org_id,
        params.request(),
        &["request_count"],
        "request_count",
    )?;
    let response = state.telemetry.apm_query.service_detail(&context).await?;
    record_apm_query_latency("service_detail", started.elapsed());
    Ok(Json(response))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn transactions(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<QueryParams>,
) -> Result<Json<PagedResponse<TransactionSummary>>> {
    let started = Instant::now();
    let request = cursor::request(&state, &ctx, &params, cursor::TRANSACTIONS)?;
    let context = state.telemetry.apm_query.context(
        ctx.org_id.clone(),
        request,
        &["request_count", "error_rate", "p95", "total_time"],
        "request_count",
    )?;
    let response = state.telemetry.apm_query.transactions(&context).await?;
    let response = cursor::sign_page(&state, &ctx, cursor::TRANSACTIONS, &context, response)?;
    record_apm_query_latency("transactions", started.elapsed());
    Ok(Json(response))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn transaction_detail(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(transaction): Path<String>,
    Query(params): Query<QueryParams>,
) -> Result<Json<TransactionDetailResponse>> {
    let started = Instant::now();
    let context = state.telemetry.apm_query.context(
        ctx.org_id,
        params.request(),
        &["request_count"],
        "request_count",
    )?;
    let response = state
        .telemetry
        .apm_query
        .transaction_detail(&context, &transaction, params.kind)
        .await?;
    record_apm_query_latency("transaction_detail", started.elapsed());
    Ok(Json(response))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn dependencies(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<QueryParams>,
) -> Result<Json<PagedResponse<DependencySummary>>> {
    let started = Instant::now();
    let request = cursor::request(&state, &ctx, &params, cursor::DEPENDENCIES)?;
    let context = state.telemetry.apm_query.context(
        ctx.org_id.clone(),
        request,
        &["request_count", "error_rate", "p95", "total_time"],
        "total_time",
    )?;
    let response = state.telemetry.apm_query.dependencies(&context).await?;
    let response = cursor::sign_page(&state, &ctx, cursor::DEPENDENCIES, &context, response)?;
    record_apm_query_latency("dependencies", started.elapsed());
    Ok(Json(response))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn errors(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<QueryParams>,
) -> Result<Json<PagedResponse<ErrorSummary>>> {
    let started = Instant::now();
    let request = cursor::request(&state, &ctx, &params, cursor::ERRORS)?;
    let context = state.telemetry.apm_query.context(
        ctx.org_id.clone(),
        request,
        &["occurrence_count", "error_rate", "last_seen"],
        "occurrence_count",
    )?;
    let response = state.telemetry.apm_query.errors(&context).await?;
    let response = cursor::sign_page(&state, &ctx, cursor::ERRORS, &context, response)?;
    record_apm_query_latency("errors", started.elapsed());
    Ok(Json(response))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn error_detail(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(fingerprint): Path<String>,
    Query(params): Query<QueryParams>,
) -> Result<Json<ErrorDetailResponse>> {
    let started = Instant::now();
    let context = state.telemetry.apm_query.context(
        ctx.org_id,
        params.request(),
        &["occurrence_count"],
        "occurrence_count",
    )?;
    let response = state
        .telemetry
        .apm_query
        .error_detail(&context, &fingerprint)
        .await?;
    record_apm_query_latency("error_detail", started.elapsed());
    Ok(Json(response))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn version_compare(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<QueryParams>,
) -> Result<Json<VersionCompareResponse>> {
    let started = Instant::now();
    let baseline = params
        .baseline
        .as_deref()
        .ok_or_else(|| crate::shared::Error::invalid("baseline version is required"))?;
    let candidate = params
        .candidate
        .as_deref()
        .ok_or_else(|| crate::shared::Error::invalid("candidate version is required"))?;
    let context = state.telemetry.apm_query.context(
        ctx.org_id,
        params.request(),
        &["request_count"],
        "request_count",
    )?;
    let response = state
        .telemetry
        .apm_query
        .compare_versions(&context, baseline, candidate)
        .await?;
    record_apm_query_latency("version_compare", started.elapsed());
    Ok(Json(response))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn health(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<QueryParams>,
) -> Result<Json<ApmTenantHealthResponse>> {
    let started = Instant::now();
    let context = state.telemetry.apm_query.context(
        ctx.org_id,
        params.request(),
        &["request_count"],
        "request_count",
    )?;
    let response = state
        .telemetry
        .apm_query
        .tenant_health(&context, state.telemetry.apm_runtime.as_deref())
        .await?;
    record_apm_query_latency("health", started.elapsed());
    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_range_defaults_to_last_day_and_preserves_filters() {
        let params = QueryParams {
            service: Some("api".into()),
            environment: Some("prod".into()),
            ..QueryParams::default()
        };
        let request = params.request();
        assert_eq!(request.to - request.from, DAY_MICROS);
        assert_eq!(request.service.as_deref(), Some("api"));
        assert_eq!(request.environment.as_deref(), Some("prod"));
    }
}
