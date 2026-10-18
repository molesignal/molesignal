// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 从事件开始的调查会话。
//!
//! - `GET  /api/v1/agent/investigate/incident/{id}`：发起前要用的东西：事件，以及按升级策略推出的
//!   团队（第一个团队目标，否则第一个值班表的团队；推不出时为空，发起人可以改）。
//! - `POST /api/v1/agent/chat/{id}/rca`：把一条回答写回事件的 RCA，要对这个事件有 `alerts.manage`。
//!
//! 新建会话时带 `subject: {kind: "incident", id}`（[`start`]）：核对能读这个事件，建一条调查记录；
//! 会话之后的回答把步骤、证据、假设与结论记到它上面，「调查」页签列出这些会话。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::viewer::{readable_chat, viewer};
use crate::{
    agent::{
        model::{Investigation, InvestigationStatus},
        scope::Scope,
    },
    api::{
        AppState,
        http::{middleware::authorize_resource, routes::activity_audit},
    },
    app::iam::IamContext,
    domain::{
        alerting::{
            escalation::{EscalationPolicy, EscalationTarget},
            incident::{Incident, IncidentRca},
        },
        iam::{Team, permission},
    },
    infra::persistence::repositories::agent::chats::Chat,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/investigate/incident/{id}", get(incident_subject))
        .route("/agent/chat/{id}/rca", post(write_rca))
}

/// 会话调查的对象。
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SubjectReq {
    /// 目前只有 `incident`。
    pub(crate) kind: String,
    pub(crate) id: String,
}

/// 事件的团队：升级策略里第一个团队目标，否则第一个值班表的团队。
async fn incident_team(state: &AppState, incident: &Incident) -> Option<Team> {
    let policies = state
        .alerting
        .service
        .list_policies(&incident.org_id)
        .await
        .ok()?;

    let policy = EscalationPolicy::resolve(&policies, &incident.escalation_policy_id)?;
    let targets = policy.steps.iter().flat_map(|step| step.targets.iter());
    let mut schedule_team = None;

    for target in targets {
        match target {
            EscalationTarget::Team { team_id } => return state.iam.teams.get(team_id).await.ok(),
            EscalationTarget::Schedule { schedule_id } if schedule_team.is_none() => {
                schedule_team = state
                    .alerting
                    .service
                    .get_schedule(schedule_id)
                    .await
                    .ok()
                    .and_then(|schedule| schedule.team_id);
            }
            _ => {}
        }
    }

    state.iam.teams.get(&schedule_team?).await.ok()
}

#[permission("agent.use")]
async fn incident_subject(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let incident: Incident =
        authorize_resource(&state, &ctx, Id::from_string(id), "alerts.read").await?;

    let viewer = viewer(&state, &ctx).await?;

    // 推出的团队只在发起人能为它开团队会话时给出。
    let team = incident_team(&state, &incident).await.filter(|team| {
        viewer.can_edit(&Scope::Team {
            team_id: team.id.clone(),
        })
    });

    Ok(Json(json!({
        "incident": {
            "id": incident.id.0,
            "summary": incident.summary,
            "severity": incident.severity,
            "status": incident.status,
            "created_at_micros": incident.created_at.0,
        },
        "team": team.map(|team| json!({ "id": team.id.0, "name": team.name })),
    })))
}

/// 新会话从一个事件开始：核对能读它，建调查记录，记在会话上。
pub(crate) async fn start(
    state: &AppState,
    ctx: &IamContext,
    chat: &mut Chat,
    subject: &SubjectReq,
) -> Result<()> {
    if subject.kind != "incident" {
        return Err(Error::invalid(format!(
            "a chat investigates an incident, not a `{}`",
            subject.kind
        )));
    }

    let incident: Incident = authorize_resource(
        state,
        ctx,
        Id::from_string(subject.id.trim()),
        "alerts.read",
    )
    .await?;

    let now = TimestampMicros::now();

    let investigation = state
        .agent
        .repository
        .create_investigation(Investigation {
            id: Id::new(),
            org_id: ctx.org_id.clone(),
            created_by: ctx.user_id.clone(),
            chat_id: Some(chat.id.clone()),
            title: incident.summary.clone(),
            status: InvestigationStatus::Running,
            context: json!({
                "source": "incident",
                "incident_id": incident.id.0,
                "team_id": chat.team_id,
            }),
            summary: None,
            confidence: None,
            current_step: None,
            started_at: Some(now),
            completed_at: None,
            created_at: now,
            updated_at: now,
        })
        .await?;

    if chat.title.trim().is_empty() {
        chat.title = incident.summary.chars().take(200).collect();
    }

    chat.subject_kind = Some("incident".into());
    chat.subject_id = Some(incident.id.0);
    chat.investigation_id = Some(investigation.id.0);
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RcaReq {
    message_id: String,
}

/// 把会话里的一条回答写成事件的 RCA。
#[permission("agent.use")]
async fn write_rca(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(chat_id): Path<String>,
    Json(req): Json<RcaReq>,
) -> Result<Json<Value>> {
    let (chat, _) = readable_chat(&state, &ctx, &Id(chat_id)).await?;

    let incident_id = match (chat.subject_kind.as_deref(), chat.subject_id.as_deref()) {
        (Some("incident"), Some(id)) => Id::from_string(id),
        _ => return Err(Error::invalid("this chat does not investigate an incident")),
    };

    let incident: Incident = authorize_resource(&state, &ctx, incident_id, "alerts.manage").await?;

    let message = state
        .agent
        .chats
        .list_messages(&chat.id)
        .await?
        .into_iter()
        .find(|message| message.id.0 == req.message_id && message.role == "assistant")
        .ok_or_else(|| Error::not_found("the answer is not in this chat"))?;

    let now = TimestampMicros::now();

    let rca = state
        .agent
        .incident_rca
        .upsert(IncidentRca {
            incident_id: incident.id.clone(),
            org_id: ctx.org_id.clone(),
            summary: message.content.clone(),
            provider: Some(chat.provider.clone()),
            model: Some(chat.model.clone()),
            prompt_builtin_key: message.prompt_builtin_key.clone(),
            prompt_hash: message.prompt_hash.clone(),
            prompt_tokens: message.prompt_tokens.unwrap_or_default() as i32,
            completion_tokens: message.completion_tokens.unwrap_or_default() as i32,
            finish_reason: Some("chat".into()),
            created_at: now,
            updated_at: now,
        })
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.incident_rca.written",
        "incident",
        &incident.id.0,
        json!({ "chat_id": chat.id.0, "message_id": message.id.0 }),
    )
    .await;

    Ok(Json(json!({
        "incident_id": rca.incident_id.0,
        "updated_at_micros": rca.updated_at.0,
    })))
}
