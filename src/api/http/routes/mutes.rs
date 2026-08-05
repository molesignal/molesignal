// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警屏蔽 mute 规则 CRUD（/alerts/mutes）。命中 matchers + 时间窗 active 时 dispatcher 暂停派发。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    api::{
        AppState,
        http::federation::{delete_payload, emit_cud},
    },
    app::iam::IamContext,
    domain::{
        alerting::{
            incident::IncidentStatus,
            mute::{INCIDENT_ID_MATCHER_LABEL, MuteRule, MuteWindow},
            semantic_group::{LabelMatcher, MatchOp},
        },
        federation::{CudAction, ResourceKind},
        iam::permission,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/alerts/mutes", get(list).post(create))
        .route(
            "/alerts/mutes/{id}",
            get(get_one).put(update).delete(remove),
        )
        .route("/alerts/incidents/{id}/silence", post(silence_incident))
}

#[derive(Debug, Deserialize)]
struct WriteReq {
    name: String,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    matchers: Vec<LabelMatcher>,
    window: MuteWindow,
    #[serde(default)]
    comment: String,
}

fn default_true() -> bool {
    true
}

const DEFAULT_INCIDENT_SILENCE_SECS: u64 = 60 * 60;
const MIN_INCIDENT_SILENCE_SECS: u64 = 60;
const MAX_INCIDENT_SILENCE_SECS: u64 = 30 * 24 * 60 * 60;

#[derive(Debug, Deserialize)]
struct SilenceIncidentReq {
    #[serde(default = "default_incident_silence_secs")]
    duration_secs: u64,
    #[serde(default)]
    comment: String,
}

fn default_incident_silence_secs() -> u64 {
    DEFAULT_INCIDENT_SILENCE_SECS
}

#[permission("alerts.silence")]
async fn silence_incident(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<SilenceIncidentReq>,
) -> Result<Json<Value>> {
    if !(MIN_INCIDENT_SILENCE_SECS..=MAX_INCIDENT_SILENCE_SECS).contains(&req.duration_secs) {
        return Err(Error::invalid(format!(
            "duration_secs must be between {MIN_INCIDENT_SILENCE_SECS} and {MAX_INCIDENT_SILENCE_SECS}"
        )));
    }

    let incident = state
        .alerting
        .service
        .get_incident(&Id::from_string(id))
        .await?;
    if incident.org_id != ctx.org_id {
        return Err(Error::forbidden("incident belongs to another org"));
    }
    if !matches!(
        incident.status,
        IncidentStatus::Open | IncidentStatus::Acknowledged
    ) {
        return Err(Error::conflict("only active incidents can be silenced"));
    }

    let now = TimestampMicros::now();
    let duration_micros = (req.duration_secs as i64).saturating_mul(1_000_000);
    let rule = MuteRule {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name: format!("incident-{}", incident.id.0),
        enabled: true,
        matchers: vec![LabelMatcher {
            label: INCIDENT_ID_MATCHER_LABEL.to_string(),
            op: MatchOp::Eq,
            value: incident.id.0.clone(),
        }],
        window: MuteWindow::Fixed {
            start: now,
            end: TimestampMicros(now.0.saturating_add(duration_micros)),
        },
        comment: if req.comment.trim().is_empty() {
            format!("Silenced from incident: {}", incident.summary)
        } else {
            req.comment.trim().to_string()
        },
        created_by: Some(ctx.user_id.clone()),
        created_at: now,
        updated_at: now,
    };
    let saved = state.alerting.mute_rules.create(rule).await?;
    emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::MuteRule,
        CudAction::Created,
        &saved.id.0,
        &saved,
    )
    .await;
    Ok(Json(serde_json::to_value(saved).unwrap()))
}

#[permission("alerts.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    Ok(Json(
        serde_json::to_value(state.alerting.mute_rules.list(&ctx.org_id).await?).unwrap(),
    ))
}

#[permission("alerts.silence")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<WriteReq>,
) -> Result<Json<Value>> {
    if req.name.trim().is_empty() {
        return Err(Error::invalid("name cannot be empty"));
    }
    let now = TimestampMicros::now();
    let rule = MuteRule {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name: req.name,
        enabled: req.enabled,
        matchers: req.matchers,
        window: req.window,
        comment: req.comment,
        created_by: Some(ctx.user_id.clone()),
        created_at: now,
        updated_at: now,
    };
    let saved = state.alerting.mute_rules.create(rule).await?;
    emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::MuteRule,
        CudAction::Created,
        &saved.id.0,
        &saved,
    )
    .await;
    Ok(Json(serde_json::to_value(saved).unwrap()))
}

#[permission("alerts.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let rule = state.alerting.mute_rules.get(&Id::from_string(id)).await?;
    if rule.org_id != ctx.org_id {
        return Err(Error::forbidden("mute rule belongs to another org"));
    }
    Ok(Json(serde_json::to_value(rule).unwrap()))
}

#[permission("alerts.silence")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<WriteReq>,
) -> Result<Json<Value>> {
    if req.name.trim().is_empty() {
        return Err(Error::invalid("name cannot be empty"));
    }
    let existing = state.alerting.mute_rules.get(&Id::from_string(id)).await?;
    if existing.org_id != ctx.org_id {
        return Err(Error::forbidden("mute rule belongs to another org"));
    }
    let rule = MuteRule {
        id: existing.id,
        org_id: ctx.org_id.clone(),
        name: req.name,
        enabled: req.enabled,
        matchers: req.matchers,
        window: req.window,
        comment: req.comment,
        created_by: existing.created_by,
        created_at: existing.created_at,
        updated_at: TimestampMicros::now(),
    };
    let saved = state.alerting.mute_rules.update(rule).await?;
    emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::MuteRule,
        CudAction::Updated,
        &saved.id.0,
        &saved,
    )
    .await;
    Ok(Json(serde_json::to_value(saved).unwrap()))
}

#[permission("alerts.silence")]
async fn remove(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let rid = Id::from_string(id);
    let existing = state.alerting.mute_rules.get(&rid).await?;
    if existing.org_id != ctx.org_id {
        return Err(Error::forbidden("mute rule belongs to another org"));
    }
    state.alerting.mute_rules.delete(&rid).await?;
    emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::MuteRule,
        CudAction::Deleted,
        &rid.0,
        &delete_payload(&rid.0),
    )
    .await;
    Ok(Json(serde_json::json!({"deleted": true})))
}
