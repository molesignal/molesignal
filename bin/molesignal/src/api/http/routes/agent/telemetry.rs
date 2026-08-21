// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 模型遥测查询路由。
//!
//! 模块无条件编译；未获得 `agent` License 的调用由 handler 返回 403。
//!
//! `/api/v1/agent/telemetry/*`：经 `QueryService::run` 跑 SQL
//! over `agent_model_traces` stream。

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    agent::telemetry::{AGENT_FEATURE, AGENT_STREAM, AgentStatsQuery},
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::permission,
        query::{QueryLanguage, QueryRequest, StreamHint},
        stream::StreamType,
    },
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/telemetry/stats", get(stats))
        .route("/agent/telemetry/top-models", get(top_models))
        .route("/agent/telemetry/top-users", get(top_users))
}

#[derive(Debug, Deserialize)]
pub struct AgentQueryParams {
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub limit: Option<u32>,
}

fn resolve_range(p: &AgentQueryParams) -> (i64, i64) {
    let to = p.to.unwrap_or_else(|| TimestampMicros::now().0);
    let from = p.from.unwrap_or(to.saturating_sub(60 * 60 * 1_000_000));
    (from, to)
}

fn require_agent_license(state: &AppState) -> Result<()> {
    if !state.platform.license.has_feature(AGENT_FEATURE) {
        return Err(Error::forbidden(format!(
            "{AGENT_FEATURE} feature not licensed"
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
            name: AGENT_STREAM.to_string(),
            stream_type: StreamType::TRACES,
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
    Query(p): Query<AgentQueryParams>,
) -> Result<Json<Value>> {
    require_agent_license(&state)?;
    let (from, to) = resolve_range(&p);
    let q = AgentStatsQuery {
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
    Query(p): Query<AgentQueryParams>,
) -> Result<Json<Value>> {
    require_agent_license(&state)?;
    let (from, to) = resolve_range(&p);
    let limit = p.limit.unwrap_or(10).clamp(1, 1000);
    let q = AgentStatsQuery {
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
    Query(p): Query<AgentQueryParams>,
) -> Result<Json<Value>> {
    require_agent_license(&state)?;
    let (from, to) = resolve_range(&p);
    let limit = p.limit.unwrap_or(10).clamp(1, 1000);
    let q = AgentStatsQuery {
        org_id: ctx.org_id.clone(),
        from_micros: from,
        to_micros: to,
    };
    let v = run_sql(&state, &ctx, q.top_users_sql(limit), from, to).await?;
    Ok(Json(v))
}
