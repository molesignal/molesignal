// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 「质量」页签的读接口。
//!
//! - `GET /api/v1/agent/quality/overview?days=&source=`：一段时间里的质量与分组
//! - `GET /api/v1/agent/quality/answers?days=&source=&below=&before_at=&before_id=&limit=`
//! - `GET /api/v1/agent/quality/answers/{kind}/{id}`：一个对象的分数、待审与材料
//!
//! 个人对话只在合计里，列表与详情都没有它们。

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::facts::{Kind, gather};
use crate::{
    agent::scoring::{OverviewFilter, ScoreTarget, ScoredFilter, TargetSource},
    api::{AppState, http::interface_language::interface_language_pack},
    app::iam::IamContext,
    domain::iam::permission,
    shared::{Error, Result, time::TimestampMicros},
};

const MICROS_PER_DAY: i64 = 86_400_000_000;
const DEFAULT_DAYS: i64 = 30;
const MAX_DAYS: i64 = 90;
const DEFAULT_PAGE: i64 = 50;
const MAX_PAGE: i64 = 100;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/quality/overview", get(overview))
        .route("/agent/quality/answers", get(answers))
        .route("/agent/quality/answers/{kind}/{id}", get(answer))
}

#[derive(Debug, Deserialize)]
struct OverviewQuery {
    #[serde(default)]
    days: Option<i64>,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnswersQuery {
    #[serde(default)]
    days: Option<i64>,
    #[serde(default)]
    source: Option<String>,
    /// 只要分数低于它的（0 到 1）。
    #[serde(default)]
    below: Option<f64>,
    #[serde(default)]
    before_at: Option<i64>,
    #[serde(default)]
    before_id: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

fn since(days: Option<i64>) -> (i64, TimestampMicros) {
    let days = days.unwrap_or(DEFAULT_DAYS).clamp(1, MAX_DAYS);

    (
        days,
        TimestampMicros(TimestampMicros::now().0 - days * MICROS_PER_DAY),
    )
}

fn source(raw: Option<&str>) -> Result<Option<TargetSource>> {
    raw.filter(|source| !source.is_empty() && *source != "all")
        .map(TargetSource::parse)
        .transpose()
}

/// 评审的 ID 与名称，页面按它写分数属于谁。
async fn evaluators(state: &AppState, ctx: &IamContext) -> Result<Vec<Value>> {
    Ok(state
        .agent
        .scoring
        .list_evaluators(&ctx.org_id)
        .await?
        .into_iter()
        .map(|evaluator| {
            json!({ "id": evaluator.id.0, "name": evaluator.name, "enabled": evaluator.enabled })
        })
        .collect())
}

/// Prompt 的名称：按 ID，内置的也按内置键，内置的用读者的语言。
async fn prompt_names(state: &AppState, ctx: &IamContext) -> Result<Map<String, Value>> {
    let pack = interface_language_pack(state, ctx, "agent.prompt").await;
    let mut names = Map::new();

    for prompt in state.agent.prompts.list(&ctx.org_id, &ctx.user_id).await? {
        let name = prompt
            .builtin_key
            .as_deref()
            .filter(|_| prompt.scope == "builtin")
            .and_then(|key| pack.get(&format!("agent.prompt.{key}.name")))
            .map(str::to_string)
            .unwrap_or(prompt.name);

        if prompt.scope == "builtin"
            && let Some(key) = &prompt.builtin_key
        {
            names.insert(key.clone(), Value::String(name.clone()));
        }

        names.insert(prompt.id.0, Value::String(name));
    }

    Ok(names)
}

#[permission("agent.quality.read")]
async fn overview(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<OverviewQuery>,
) -> Result<Json<Value>> {
    let (days, since) = since(query.days);

    let overview = state
        .agent
        .scoring
        .overview(
            &ctx.org_id,
            &OverviewFilter {
                since,
                source: source(query.source.as_deref())?,
            },
        )
        .await?;

    Ok(Json(json!({
        "days": days,
        "overview": overview,
        "promptNames": prompt_names(&state, &ctx).await?,
        "evaluators": evaluators(&state, &ctx).await?,
    })))
}

#[permission("agent.quality.read")]
async fn answers(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<AnswersQuery>,
) -> Result<Json<Value>> {
    let (days, since) = since(query.days);
    let limit = query.limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);

    let before = match (query.before_at, query.before_id) {
        (Some(at), Some(id)) => Some((TimestampMicros(at), id)),
        _ => None,
    };

    let answers = state
        .agent
        .scoring
        .list_scored(
            &ctx.org_id,
            &ScoredFilter {
                since,
                source: source(query.source.as_deref())?,
                below: query.below.filter(|below| (0.0..=1.0).contains(below)),
                before,
                limit,
            },
        )
        .await?;

    let next = (answers.len() as i64 == limit)
        .then(|| answers.last())
        .flatten()
        .map(|last| {
            json!({
                "beforeAt": last.subject.target_at.0,
                "beforeId": last.subject.target_id,
            })
        });

    Ok(Json(json!({
        "days": days,
        "answers": answers,
        "next": next,
        "evaluators": evaluators(&state, &ctx).await?,
    })))
}

#[permission("agent.quality.read")]
async fn answer(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((kind, id)): Path<(String, String)>,
) -> Result<Json<Value>> {
    let kind = ScoreTarget::parse(&kind)?;
    let not_found = || Error::not_found(format!("scored answer `{id}` not found"));

    let scored = state
        .agent
        .scoring
        .scored(&ctx.org_id, kind, &id)
        .await?
        .filter(|scored| !scored.subject.source.is_private())
        .ok_or_else(not_found)?;

    let facts = gather(
        &state,
        &ctx.org_id,
        kind,
        &id,
        scored.subject.attribution.chat_id.as_ref(),
    )
    .await?;

    // 材料：完整的问题、写下的回答（页面按回答的形状排版），以及那一轮调过的工具；对象不在了时
    // 没有。
    let material = facts.map(|facts| {
        let tools = facts
            .steps
            .iter()
            .flat_map(|step| step.calls.iter())
            .map(|call| json!({ "name": call.name, "failed": call.is_error }))
            .collect::<Vec<_>>();

        let answer = match &facts.kind {
            Kind::Answer { content, .. } => content.clone(),
            Kind::Rca { summary } => summary.clone(),
        };

        json!({ "question": facts.question, "answer": answer, "tools": tools })
    });

    let review = state
        .agent
        .scoring
        .review_of(&ctx.org_id, kind, &id)
        .await?;

    Ok(Json(json!({
        "answer": scored,
        "review": review,
        "material": material,
        "evaluators": evaluators(&state, &ctx).await?,
    })))
}
