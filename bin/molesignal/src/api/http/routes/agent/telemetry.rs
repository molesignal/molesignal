// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 模型遥测查询路由。
//!

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
    agent::telemetry::{AGENT_STREAM, AgentStatsQuery, OptionalColumns},
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::permission,
        query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
        stream::StreamType,
    },
    shared::{
        Result,
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

/// 统计查询的时间范围与用量流里写过的可选列。流还不存在时报 404，与直接查询一致。
async fn stats_query(
    state: &AppState,
    ctx: &IamContext,
    params: &AgentQueryParams,
) -> Result<AgentStatsQuery> {
    let (from, to) = resolve_range(params);

    let definition = state
        .telemetry
        .streams
        .get(&ctx.org_id, AGENT_STREAM, StreamType::LOGS)
        .await?;

    let columns = OptionalColumns::from_names(
        definition
            .schema
            .fields
            .iter()
            .map(|field| field.name.as_str()),
    );

    Ok(AgentStatsQuery {
        org_id: ctx.org_id.clone(),
        from_micros: from,
        to_micros: to,
        columns,
    })
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
        // 用量流按日志写入（见 `agent_traces` worker），按同一类型读。
        stream: Some(StreamHint {
            name: AGENT_STREAM.to_string(),
            stream_type: StreamType::LOGS,
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
    let q = stats_query(&state, &ctx, &p).await?;

    let v = run_sql(&state, &ctx, q.overall_sql(), q.from_micros, q.to_micros).await?;
    Ok(Json(v))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn top_models(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(p): Query<AgentQueryParams>,
) -> Result<Json<Value>> {
    let limit = p.limit.unwrap_or(10).clamp(1, 1000);
    let q = stats_query(&state, &ctx, &p).await?;

    let v = run_sql(
        &state,
        &ctx,
        q.top_models_sql(limit),
        q.from_micros,
        q.to_micros,
    )
    .await?;

    Ok(Json(v))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn top_users(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(p): Query<AgentQueryParams>,
) -> Result<Json<Value>> {
    let limit = p.limit.unwrap_or(10).clamp(1, 1000);
    let q = stats_query(&state, &ctx, &p).await?;

    // 还没有记在谁名下的调用：没有可排的人。
    let Some(sql) = q.top_users_sql(limit) else {
        let empty = QueryResult {
            columns: vec!["user_id".into(), "calls".into(), "tokens".into()],
            rows: Vec::new(),
            scanned_rows: 0,
            took_ms: 0,
            federation: None,
        };

        return Ok(Json(serde_json::to_value(empty).unwrap_or(Value::Null)));
    };

    let v = run_sql(&state, &ctx, sql, q.from_micros, q.to_micros).await?;
    Ok(Json(v))
}
