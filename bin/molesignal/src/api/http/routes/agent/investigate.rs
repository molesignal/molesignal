// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 从事件、APM 服务、RUM 应用或一条自动发现开始的调查会话。
//!
//! - `GET  /api/v1/agent/investigate/incident/{id}`：发起前要用的东西：事件，以及按升级策略推出的
//!   团队（第一个团队目标，否则第一个值班表的团队；推不出时为空，发起人可以改）。
//! - `POST /api/v1/agent/chat/{id}/rca`：把一条回答写回事件的 RCA，要对这个事件有 `alerts.incidents.rca`。
//!
//! 新建会话时带 `subject: {kind: "incident", id}`、`{kind: "service", id: 服务名}`、
//! `{kind: "rum", id: 应用 id}` 或 `{kind: "finding", id: 发现 id}`（[`start`]）：
//! 核对能读它，建一条调查记录；会话之后的回答把步骤、证据、假设与结论记到它上面，「调查」页签列出
//! 这些会话。事件 RCA 只写回从事件开始的会话。

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
        answer::{Headings, StructuredAnswer},
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
    /// `incident`（`id` 是事件 id）、`service`（`id` 是服务名）或 `rum`（`id` 是 RUM 应用 id）。
    pub(crate) kind: String,
    pub(crate) id: String,
}

/// 事件的团队：升级策略里第一个团队目标，否则第一个值班表的团队。
pub(crate) async fn incident_team(state: &AppState, incident: &Incident) -> Option<Team> {
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
        authorize_resource(&state, &ctx, Id::from_string(id), "alerts.incidents.read").await?;

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
            "createdAt": incident.created_at.0,
        },
        "team": team.map(|team| json!({ "id": team.id.0, "name": team.name })),
    })))
}

/// 新会话从一个事件、一个服务或一个 RUM 应用开始：核对能读它，建调查记录，记在会话上。
pub(crate) async fn start(
    state: &AppState,
    ctx: &IamContext,
    chat: &mut Chat,
    subject: &SubjectReq,
) -> Result<()> {
    let (title, context, subject_id) = match subject.kind.as_str() {
        "incident" => {
            let incident: Incident = authorize_resource(
                state,
                ctx,
                Id::from_string(subject.id.trim()),
                "alerts.incidents.read",
            )
            .await?;

            let context = json!({
                "source": "incident",
                "incidentId": incident.id.0,
                "teamId": chat.team_id,
            });

            (incident.summary, context, incident.id.0)
        }
        // 从 APM 的服务、端点或错误发起：服务名是调查的对象，问题里写明端点、错误与时间。
        "service" => {
            if !ctx.has_permission("apm.services.read") && !ctx.has_permission("sys.telemetry.read")
            {
                return Err(Error::forbidden(
                    "investigating a service needs apm.services.read",
                ));
            }

            let name = subject.id.trim();

            if name.is_empty() || name.chars().count() > 255 {
                return Err(Error::invalid("a service is named in 1 to 255 characters"));
            }

            let context = json!({
                "source": "apm",
                "service": name,
                "teamId": chat.team_id,
            });

            (name.to_owned(), context, name.to_owned())
        }
        // 从 RUM 的应用、问题、页面或会话发起：应用是调查的对象，问题里写明环境、页面与时间。
        "rum" => {
            if !ctx.has_permission("rum.events.read") && !ctx.has_permission("sys.telemetry.read") {
                return Err(Error::forbidden(
                    "investigating a RUM application needs rum.events.read",
                ));
            }

            let application = state
                .telemetry
                .rum_applications
                .get(&ctx.org_id, subject.id.trim())
                .await?;

            let context = json!({
                "source": "rum",
                "application": application.id,
                "teamId": chat.team_id,
            });

            (application.name, context, application.id)
        }
        // 从一条自动发现发起：发现是调查的对象，问题里写明它是什么、从什么时候开始。
        "finding" => {
            if !ctx.has_permission("alerts.discovery.read") {
                return Err(Error::forbidden(
                    "investigating a discovery finding needs alerts.discovery.read",
                ));
            }

            let finding = state
                .alerting
                .discovery
                .get(&ctx.org_id, &Id::from_string(subject.id.trim()))
                .await?
                .ok_or_else(|| Error::not_found("discovery finding"))?;

            let context = json!({
                "source": "discovery",
                "findingId": finding.id.0,
                "kind": finding.kind.as_str(),
                "subject": finding.subject,
                "teamId": chat.team_id,
            });

            (finding.title, context, finding.id.0)
        }
        other => {
            return Err(Error::invalid(format!(
                "a chat investigates an incident, a service, a RUM application or a discovery \
                 finding, not a `{other}`"
            )));
        }
    };

    let now = TimestampMicros::now();

    let investigation = state
        .agent
        .repository
        .create_investigation(Investigation {
            id: Id::new(),
            org_id: ctx.org_id.clone(),
            created_by: ctx.user_id.clone(),
            chat_id: Some(chat.id.clone()),
            title: title.clone(),
            status: InvestigationStatus::Running,
            context,
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
        chat.title = title.chars().take(200).collect();
    }

    chat.subject_kind = Some(subject.kind.clone());
    chat.subject_id = Some(subject_id);
    chat.investigation_id = Some(investigation.id.0);
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RcaReq {
    message_id: String,
}

/// 回答写成事件的 RCA 时的正文：结构化的回答排成 Markdown，标题用 `language`（会话的回答语言，
/// 空时用英文）；其余原样。
pub(crate) async fn answer_as_rca(state: &AppState, language: &str, content: &str) -> String {
    let Some(answer) = StructuredAnswer::parse(content) else {
        return content.trim().to_string();
    };

    let language = if language.trim().is_empty() {
        "en-us"
    } else {
        language
    };

    let pack = state
        .platform
        .i18n_language
        .load_pack("agent.answer.", language)
        .await
        .unwrap_or_default();

    let text = |key: &str, fallback: &str| {
        pack.get(&format!("agent.answer.{key}"))
            .unwrap_or(fallback)
            .to_string()
    };

    answer.to_markdown(&Headings {
        likely_causes: text("likely_causes", "Likely causes"),
        evidence: text("evidence", "Evidence"),
        next_steps: text("next_steps", "Next steps"),
        limitations: text("limitations", "Limitations"),
        related_links: text("related_links", "Related actions"),
        confidence_high: text("confidence.high", "Confidence: High"),
        confidence_medium: text("confidence.medium", "Confidence: Medium"),
        confidence_low: text("confidence.low", "Confidence: Low"),
    })
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

    let incident: Incident =
        authorize_resource(&state, &ctx, incident_id, "alerts.incidents.rca").await?;

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
            summary: answer_as_rca(&state, &chat.reply_language, &message.content).await,
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
        json!({ "chatId": chat.id.0, "messageId": message.id.0 }),
    )
    .await;

    Ok(Json(json!({
        "incidentId": rca.incident_id.0,
        "updatedAt": rca.updated_at.0,
    })))
}
