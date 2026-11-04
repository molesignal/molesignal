// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 评审与组织的打分设置。
//!
//! - `GET|POST /api/v1/agent/quality/evaluators`、`PUT|DELETE /api/v1/agent/quality/evaluators/{id}`
//! - `GET|PUT  /api/v1/agent/quality/settings`
//!
//! 保存评审的人的界面语言是评审写理由用的语言。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    agent::scoring::{Evaluator, GateMode, ScoringSettings, TargetSource},
    api::{
        AppState,
        http::{interface_language::interface_language, routes::activity_audit},
    },
    app::iam::IamContext,
    domain::{i18n_language::FALLBACK_LANGUAGE_CODE, iam::permission},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/quality/evaluators", get(list).post(create))
        .route(
            "/agent/quality/evaluators/{id}",
            axum::routing::put(update).delete(remove),
        )
        .route("/agent/quality/settings", get(settings).put(put_settings))
}

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EvaluatorInput {
    name: String,
    rubric: String,
    targets: Vec<String>,
    sample_percent: u8,
    #[serde(default)]
    provider_id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default = "yes")]
    enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsInput {
    review_threshold: f64,
    review_sample_percent: u8,
    gate: String,
}

async fn audit(state: &AppState, ctx: &IamContext, action: &str, id: &str, detail: Value) {
    activity_audit::record(state, ctx, action, "agent_evaluator", id, detail).await;
}

/// 把输入整理成评审：对象认得出，模型服务在本组织，写理由用保存人的界面语言。
async fn evaluator_of(
    state: &AppState,
    ctx: &IamContext,
    input: EvaluatorInput,
    existing: Option<Evaluator>,
) -> Result<Evaluator> {
    let targets = input
        .targets
        .iter()
        .map(|target| TargetSource::parse(target))
        .collect::<Result<Vec<_>>>()?;

    let provider_id = input
        .provider_id
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .map(Id);

    if let Some(provider_id) = &provider_id {
        state
            .agent
            .model_providers
            .get(&ctx.org_id, provider_id)
            .await?;
    }

    let language = interface_language(state, ctx)
        .await
        .unwrap_or_else(|_| FALLBACK_LANGUAGE_CODE.to_string());
    let now = TimestampMicros::now();

    Evaluator {
        id: existing
            .as_ref()
            .map_or_else(Id::new, |existing| existing.id.clone()),
        org_id: ctx.org_id.clone(),
        name: input.name,
        rubric: input.rubric,
        targets,
        sample_percent: input.sample_percent,
        provider_id,
        model: input.model,
        temperature: input.temperature.unwrap_or(0.0),
        enabled: input.enabled,
        language,
        created_by: existing.as_ref().map_or_else(
            || ctx.user_id.clone(),
            |existing| existing.created_by.clone(),
        ),
        updated_by: ctx.user_id.clone(),
        created_at: existing
            .as_ref()
            .map_or(now, |existing| existing.created_at),
        updated_at: now,
    }
    .normalized()
}

fn detail(evaluator: &Evaluator) -> Value {
    json!({
        "name": evaluator.name,
        "targets": evaluator.targets,
        "samplePercent": evaluator.sample_percent,
        "providerId": evaluator.provider_id.as_ref().map(|id| &id.0),
        "model": evaluator.model,
        "enabled": evaluator.enabled,
    })
}

#[permission("agent.quality.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let evaluators = state.agent.scoring.list_evaluators(&ctx.org_id).await?;

    Ok(Json(json!({ "evaluators": evaluators })))
}

#[permission("agent.quality.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(input): Json<EvaluatorInput>,
) -> Result<Json<Evaluator>> {
    let evaluator = evaluator_of(&state, &ctx, input, None).await?;
    let saved = state.agent.scoring.create_evaluator(evaluator).await?;

    audit(
        &state,
        &ctx,
        "agent.quality.evaluator_created",
        &saved.id.0,
        detail(&saved),
    )
    .await;
    Ok(Json(saved))
}

#[permission("agent.quality.manage")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(input): Json<EvaluatorInput>,
) -> Result<Json<Evaluator>> {
    let existing = state
        .agent
        .scoring
        .get_evaluator(&ctx.org_id, &Id(id))
        .await?;
    let evaluator = evaluator_of(&state, &ctx, input, Some(existing)).await?;
    let saved = state.agent.scoring.update_evaluator(evaluator).await?;

    audit(
        &state,
        &ctx,
        "agent.quality.evaluator_updated",
        &saved.id.0,
        detail(&saved),
    )
    .await;
    Ok(Json(saved))
}

#[permission("agent.quality.manage")]
async fn remove(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let evaluator = state
        .agent
        .scoring
        .get_evaluator(&ctx.org_id, &Id(id.clone()))
        .await?;

    state
        .agent
        .scoring
        .delete_evaluator(&ctx.org_id, &evaluator.id)
        .await?;

    audit(
        &state,
        &ctx,
        "agent.quality.evaluator_deleted",
        &id,
        json!({ "name": evaluator.name }),
    )
    .await;
    Ok(Json(json!({ "deleted": true })))
}

#[permission("agent.quality.read")]
async fn settings(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<ScoringSettings>> {
    Ok(Json(state.agent.scoring.settings(&ctx.org_id).await?))
}

#[permission("agent.quality.manage")]
async fn put_settings(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(input): Json<SettingsInput>,
) -> Result<Json<ScoringSettings>> {
    let settings = ScoringSettings {
        review_threshold: input.review_threshold,
        review_sample_percent: input.review_sample_percent,
        gate: GateMode::parse(&input.gate)?,
        updated_by: Some(ctx.user_id.clone()),
        updated_at: Some(TimestampMicros::now()),
    }
    .validated()?;

    let saved = state
        .agent
        .scoring
        .put_settings(&ctx.org_id, &settings)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.quality.settings_updated",
        "agent_scoring_settings",
        &ctx.org_id.0,
        json!({
            "reviewThreshold": saved.review_threshold,
            "reviewSamplePercent": saved.review_sample_percent,
            "gate": saved.gate,
        }),
    )
    .await;

    Ok(Json(saved))
}
