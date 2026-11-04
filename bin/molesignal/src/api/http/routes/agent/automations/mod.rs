// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 自动化的 HTTP 路由。
//!
//! - `GET    /api/v1/agent/automations`                 看得到的规则，带最近一次运行
//! - `POST   /api/v1/agent/automations`                 新建
//! - `GET    /api/v1/agent/automations/{id}`
//! - `PUT    /api/v1/agent/automations/{id}`            修改；保存的人成为执行身份
//! - `DELETE /api/v1/agent/automations/{id}`
//! - `POST   /api/v1/agent/automations/{id}/enabled`    启用或停用
//! - `POST   /api/v1/agent/automations/{id}/run`        手动运行，一分钟一次
//! - `POST   /api/v1/agent/automations/{id}/token`      生成或轮换 API 令牌，只返回这一次
//! - `DELETE /api/v1/agent/automations/{id}/token`      收回令牌
//! - `GET    /api/v1/agent/automations/{id}/runs`       运行记录，新的在前
//! - `GET    /api/v1/agent/automation-templates`        内置模板，读者的语言与时区
//! - `POST   /api/v1/public/automations/{id}/trigger`   API 触发（[`public`]）
//!
//! 谁能看、谁能改见 [`access`]；开始一次运行见 [`start`]，结束见 [`finish`]。

mod access;
mod finish;
mod public;
mod save;
mod start;
mod views;

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
pub(crate) use finish::{AnswerEnd, finished, reconcile};
pub use public::public_routes;
use serde::Deserialize;
use serde_json::{Value, json};
pub(crate) use start::{Start, start};

use self::{
    access::{MANAGE, can_edit, can_moderate, editable, visible},
    save::AutomationInput,
    views::AutomationView,
};
use crate::{
    agent::automations::{AutomationTrigger, MANUAL_RUN_INTERVAL_MICROS, TriggerKind},
    api::{
        AppState,
        http::{
            interface_language::interface_language_pack,
            routes::{activity_audit, agent::viewer::viewer},
        },
    },
    app::{
        automations::{templates, token},
        iam::IamContext,
    },
    domain::iam::permission,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/automations", get(list).post(create))
        .route(
            "/agent/automations/{id}",
            get(get_one).put(update).delete(remove),
        )
        .route("/agent/automations/{id}/enabled", post(set_enabled))
        .route("/agent/automations/{id}/run", post(run_now))
        .route(
            "/agent/automations/{id}/token",
            post(rotate_token).delete(revoke_token),
        )
        .route("/agent/automations/{id}/runs", get(list_runs))
        .route("/agent/automation-templates", get(list_templates))
}

async fn audit(state: &AppState, ctx: &IamContext, action: &str, id: &Id, detail: Value) {
    activity_audit::record(state, ctx, action, "agent_automation", &id.0, detail).await;
}

#[permission("agent.automations.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;

    let automations = state
        .agent
        .automations
        .list(&ctx.org_id)
        .await?
        .into_iter()
        .filter(|automation| viewer.can_see(&automation.scope))
        .map(|automation| AutomationView::of(automation, &ctx, &viewer))
        .collect::<Vec<_>>();

    Ok(Json(json!({
        "automations": automations,
        "canCreate": ctx.has_permission(MANAGE),
    })))
}

#[permission("agent.automations.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<AutomationView>> {
    let (automation, viewer) = visible(&state, &ctx, &Id(id)).await?;
    Ok(Json(AutomationView::of(automation, &ctx, &viewer)))
}

#[permission("agent.automations.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(input): Json<AutomationInput>,
) -> Result<(StatusCode, Json<AutomationView>)> {
    let viewer = viewer(&state, &ctx).await?;
    let scope = save::scope_of(&input, &ctx)?;

    if !can_edit(&ctx, &viewer, &scope) {
        return Err(Error::forbidden(
            "only the people who manage automations in this scope can create one there",
        ));
    }

    let automation = save::create(&state, &ctx, scope, input).await?;

    audit(
        &state,
        &ctx,
        "agent.automation.created",
        &automation.id,
        json!({ "name": automation.name, "trigger": automation.trigger.kind().as_str() }),
    )
    .await;

    Ok((
        StatusCode::CREATED,
        Json(AutomationView::of(automation, &ctx, &viewer)),
    ))
}

#[permission("agent.automations.manage")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(input): Json<AutomationInput>,
) -> Result<Json<AutomationView>> {
    let (existing, viewer) = editable(&state, &ctx, &Id(id)).await?;
    let automation = save::update(&state, &ctx, existing, input).await?;

    audit(
        &state,
        &ctx,
        "agent.automation.updated",
        &automation.id,
        json!({ "name": automation.name, "trigger": automation.trigger.kind().as_str() }),
    )
    .await;

    Ok(Json(AutomationView::of(automation, &ctx, &viewer)))
}

#[permission(any("agent.automations.manage", "agent.manage"))]
async fn remove(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let (automation, viewer) = visible(&state, &ctx, &Id(id)).await?;

    if !can_moderate(&ctx, &viewer, &automation.scope) {
        return Err(Error::forbidden(
            "only the people who manage this automation can delete it",
        ));
    }

    let deleted = state
        .agent
        .automations
        .delete(&ctx.org_id, &automation.id)
        .await?;

    if deleted {
        audit(
            &state,
            &ctx,
            "agent.automation.deleted",
            &automation.id,
            json!({ "name": automation.name }),
        )
        .await;
    }

    Ok(Json(json!({ "deleted": deleted })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnabledReq {
    enabled: bool,
}

/// 停用：能改的人与管理员。启用：只有能改的人，规则以它最后的保存人的身份运行。
#[permission(any("agent.automations.manage", "agent.manage"))]
async fn set_enabled(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<EnabledReq>,
) -> Result<Json<AutomationView>> {
    let (automation, viewer) = visible(&state, &ctx, &Id(id)).await?;

    let allowed = if req.enabled {
        can_edit(&ctx, &viewer, &automation.scope)
    } else {
        can_moderate(&ctx, &viewer, &automation.scope)
    };

    if !allowed {
        return Err(Error::forbidden(
            "only the people who manage this automation can switch it",
        ));
    }

    let now = TimestampMicros::now();
    let updated = state
        .agent
        .automations
        .set_enabled(&ctx.org_id, &automation.id, req.enabled)
        .await?;

    // 重新启用时从现在算起：定时的下一次，事件触发的新事件。
    if req.enabled && !automation.enabled {
        let next = save::next_run(&updated.trigger, true, now)?;
        state
            .agent
            .automations
            .set_next_run(&updated.id, next)
            .await?;

        if updated.trigger.reads_events() {
            state
                .agent
                .automations
                .advance_incident_cursor(&updated.id, updated.incident_cursor, now)
                .await?;
        }
    } else if !req.enabled {
        state
            .agent
            .automations
            .set_next_run(&updated.id, None)
            .await?;
    }

    audit(
        &state,
        &ctx,
        if req.enabled {
            "agent.automation.enabled"
        } else {
            "agent.automation.disabled"
        },
        &updated.id,
        json!({ "name": updated.name }),
    )
    .await;

    let refreshed = state
        .agent
        .automations
        .get(&ctx.org_id, &updated.id)
        .await?;
    Ok(Json(AutomationView::of(refreshed, &ctx, &viewer)))
}

#[permission("agent.automations.run")]
async fn run_now(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<Value>)> {
    let (automation, _) = editable(&state, &ctx, &Id(id)).await?;

    let recently = automation
        .last_run
        .as_ref()
        .is_some_and(|run| TimestampMicros::now().0 - run.at.0 < MANUAL_RUN_INTERVAL_MICROS);

    if recently {
        return Err(Error::ResourceExhausted(
            "an automation runs by hand at most once a minute".into(),
        ));
    }

    let run = start(
        &state,
        &automation,
        Start {
            trigger: TriggerKind::Manual,
            text: None,
            incident: None,
            finding: None,
            started_by: Some(ctx.user_id.clone()),
        },
    )
    .await?;

    audit(
        &state,
        &ctx,
        "agent.automation.run",
        &automation.id,
        json!({ "name": automation.name, "runId": run.id.0, "status": run.status.as_str() }),
    )
    .await;

    Ok((StatusCode::ACCEPTED, Json(json!({ "run": run }))))
}

#[permission("agent.automations.tokens")]
async fn rotate_token(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let (automation, _) = editable(&state, &ctx, &Id(id)).await?;

    if !matches!(automation.trigger, AutomationTrigger::Api) {
        return Err(Error::conflict(
            "only an automation started through its API has a token",
        ));
    }

    let issued = token::generate();

    state
        .agent
        .automations
        .set_api_token(
            &ctx.org_id,
            &automation.id,
            Some((issued.hash, issued.prefix.clone())),
        )
        .await?;

    audit(
        &state,
        &ctx,
        "agent.automation.token_issued",
        &automation.id,
        json!({ "name": automation.name, "prefix": issued.prefix }),
    )
    .await;

    Ok(Json(json!({
        "token": issued.token,
        "prefix": issued.prefix,
        "triggerPath": format!("/api/v1/public/automations/{}/trigger", automation.id.0),
    })))
}

#[permission("agent.automations.tokens")]
async fn revoke_token(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let (automation, _) = editable(&state, &ctx, &Id(id)).await?;

    state
        .agent
        .automations
        .set_api_token(&ctx.org_id, &automation.id, None)
        .await?;

    audit(
        &state,
        &ctx,
        "agent.automation.token_revoked",
        &automation.id,
        json!({ "name": automation.name }),
    )
    .await;

    Ok(Json(json!({ "revoked": true })))
}

#[derive(Debug, Deserialize)]
struct RunsQuery {
    #[serde(default)]
    limit: Option<i64>,
}

#[permission("agent.automations.read")]
async fn list_runs(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Query(query): Query<RunsQuery>,
) -> Result<Json<Value>> {
    let (automation, _) = visible(&state, &ctx, &Id(id)).await?;

    let runs = state
        .agent
        .automations
        .list_runs(
            &ctx.org_id,
            &automation.id,
            query.limit.unwrap_or(50).clamp(1, 100),
        )
        .await?;

    Ok(Json(json!({ "runs": runs })))
}

#[derive(Debug, Deserialize)]
struct TemplatesQuery {
    #[serde(default)]
    timezone: Option<String>,
}

#[permission("agent.automations.read")]
async fn list_templates(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<TemplatesQuery>,
) -> Result<Json<Value>> {
    let timezone = query
        .timezone
        .filter(|zone| crate::app::automations::schedule::parse_timezone(zone).is_ok())
        .unwrap_or_else(|| "UTC".into());

    let pack = interface_language_pack(&state, &ctx, templates::KEY_PREFIX).await;

    Ok(Json(json!({
        "templates": templates::render(&pack, &timezone)?,
    })))
}
