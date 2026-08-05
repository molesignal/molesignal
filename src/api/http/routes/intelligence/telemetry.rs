// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence 模型遥测查询路由。
//!
//! 仅当 `--features ` 启用且 `license.has_feature("intelligence")` 为 true 时可用：
//! - feature 关：本模块整体不编译，路由也不会注册（见 `routes/mod.rs` 的 cfg 门）。
//! - feature 开 + 无 license：handler 返 403 "intelligence feature not licensed"。
//!
//! `/api/v1/intelligence/telemetry/*`：经 `QueryService::run` 跑 SQL
//! over `intelligence_model_traces` stream。

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::permission,
        query::{QueryLanguage, QueryRequest, StreamHint},
        stream::StreamType,
    },
    intelligence::telemetry::{INTELLIGENCE_FEATURE, INTELLIGENCE_STREAM, IntelligenceStatsQuery},
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/intelligence/telemetry/stats", get(stats))
        .route("/intelligence/telemetry/top-models", get(top_models))
        .route("/intelligence/telemetry/top-users", get(top_users))
}

#[derive(Debug, Deserialize)]
pub struct IntelligenceQueryParams {
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub limit: Option<u32>,
}

fn resolve_range(p: &IntelligenceQueryParams) -> (i64, i64) {
    let to = p.to.unwrap_or_else(|| TimestampMicros::now().0);
    let from = p.from.unwrap_or(to.saturating_sub(60 * 60 * 1_000_000));
    (from, to)
}

fn require_intelligence_license(state: &AppState) -> Result<()> {
    if !state.platform.license.has_feature(INTELLIGENCE_FEATURE) {
        return Err(Error::forbidden(format!(
            "{INTELLIGENCE_FEATURE} feature not licensed"
        )));
    }
    Ok(())
}

async fn run_sql(
    state: &AppState,
    ctx: &IamContext,
    sql: String,
    from: i64,
    to: i64,
) -> Result<Value> {
    let req = QueryRequest {
        org_id: ctx.org_id.clone(),
        language: QueryLanguage::Sql,
        statement: sql,
        time_range: TimeRange::new(TimestampMicros(from), TimestampMicros(to)),
        stream: Some(StreamHint {
            name: INTELLIGENCE_STREAM.to_string(),
            stream_type: StreamType::Traces,
        }),
        limit: None,
        federation_clusters: Vec::new(),
    };
    let out = state.query.run(req).await?;
    Ok(serde_json::to_value(out).unwrap_or(Value::Null))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn stats(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(p): Query<IntelligenceQueryParams>,
) -> Result<Json<Value>> {
    require_intelligence_license(&state)?;
    let (from, to) = resolve_range(&p);
    let q = IntelligenceStatsQuery {
        org_id: ctx.org_id.clone(),
        from_micros: from,
        to_micros: to,
    };
    let v = run_sql(&state, &ctx, q.overall_sql(), from, to).await?;
    Ok(Json(v))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn top_models(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(p): Query<IntelligenceQueryParams>,
) -> Result<Json<Value>> {
    require_intelligence_license(&state)?;
    let (from, to) = resolve_range(&p);
    let limit = p.limit.unwrap_or(10).clamp(1, 1000);
    let q = IntelligenceStatsQuery {
        org_id: ctx.org_id.clone(),
        from_micros: from,
        to_micros: to,
    };
    let v = run_sql(&state, &ctx, q.top_models_sql(limit), from, to).await?;
    Ok(Json(v))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn top_users(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(p): Query<IntelligenceQueryParams>,
) -> Result<Json<Value>> {
    require_intelligence_license(&state)?;
    let (from, to) = resolve_range(&p);
    let limit = p.limit.unwrap_or(10).clamp(1, 1000);
    let q = IntelligenceStatsQuery {
        org_id: ctx.org_id.clone(),
        from_micros: from,
        to_micros: to,
    };
    let v = run_sql(&state, &ctx, q.top_users_sql(limit), from, to).await?;
    Ok(Json(v))
}
