// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 事件的处理接口：转派、增派、立即升级、暂缓、改严重度、合并、备注、时间线与批量处理。
//!
//! - `POST   /api/v1/alerts/incidents/{id}/reassign`    转派给成员、团队或升级策略
//! - `POST   /api/v1/alerts/incidents/{id}/responders`  增派成员
//! - `POST   /api/v1/alerts/incidents/{id}/escalate`    立即升级到某一步
//! - `POST   /api/v1/alerts/incidents/{id}/snooze`      暂缓；`DELETE` 取消暂缓
//! - `PUT    /api/v1/alerts/incidents/{id}/severity`    改严重度
//! - `PUT    /api/v1/alerts/incidents/{id}/priority`    改优先级（[`super::incident_importance`]）
//! - `PUT    /api/v1/alerts/incidents/{id}/urgency`     改紧急度（同上）
//! - `POST   /api/v1/alerts/incidents/{id}/merge`       把别的事件并入它
//! - `POST   /api/v1/alerts/incidents/{id}/notes`       写备注
//! - `GET    /api/v1/alerts/incidents/{id}/timeline`    时间线（`alerts.incidents.read`）
//! - `POST   /api/v1/alerts/incidents/bulk`             批量处理
//!
//! 除时间线外都要对每个涉及的事件有权限：转派、增派、改严重度、写备注要 `alerts.incidents.update`，
//! 升级、合并要 `alerts.incidents.escalate`，暂缓、批量处理要 `alerts.incidents.acknowledge`。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{AppState, http::middleware::authorize_resource},
    app::{
        alerting::{
            Actor,
            actions::{BulkAction, BulkResult, MAX_BULK_INCIDENTS, Reassignment},
        },
        iam::IamContext,
    },
    domain::{
        alerting::{
            incident::{Incident, Severity},
            timeline::ActionVia,
        },
        iam::{permission, resource_permission},
    },
    shared::{Error, Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/alerts/incidents/bulk", post(bulk))
        .route("/alerts/incidents/{id}/reassign", post(reassign))
        .route("/alerts/incidents/{id}/responders", post(add_responders))
        .route("/alerts/incidents/{id}/escalate", post(escalate))
        .route(
            "/alerts/incidents/{id}/snooze",
            post(snooze).delete(cancel_snooze),
        )
        .route("/alerts/incidents/{id}/severity", put(set_severity))
        .route("/alerts/incidents/{id}/merge", post(merge))
        .route("/alerts/incidents/{id}/notes", post(add_note))
        .route("/alerts/incidents/{id}/timeline", get(timeline))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UsersReq {
    user_ids: Vec<Id>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct EscalateReq {
    /// 升到第几步（从 0 数）；不给时升到下一步。
    #[serde(default)]
    step: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SnoozeReq {
    duration_secs: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SeverityReq {
    severity: Severity,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MergeReq {
    source_ids: Vec<Id>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoteReq {
    text: String,
}

#[derive(Debug, Deserialize)]
struct BulkReq {
    ids: Vec<Id>,
    #[serde(flatten)]
    action: BulkAction,
}

pub(super) fn actor(ctx: &IamContext) -> Actor {
    Actor::new(&ctx.org_id, &ctx.user_id, ActionVia::Web)
}

pub(super) fn incident_json(incident: Incident) -> Json<Value> {
    Json(serde_json::to_value(incident).unwrap_or(Value::Null))
}

#[resource_permission(
    action = "alerts.incidents.update",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn reassign(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(to): Json<Reassignment>,
) -> Result<Json<Value>> {
    let incident = state
        .alerting
        .actions
        .reassign(&actor(&ctx), &incident.id, to)
        .await?;

    Ok(incident_json(incident))
}

#[resource_permission(
    action = "alerts.incidents.update",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn add_responders(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UsersReq>,
) -> Result<Json<Value>> {
    let incident = state
        .alerting
        .actions
        .add_responders(&actor(&ctx), &incident.id, &req.user_ids)
        .await?;

    Ok(incident_json(incident))
}

#[resource_permission(
    action = "alerts.incidents.escalate",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn escalate(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    body: Option<Json<EscalateReq>>,
) -> Result<Json<Value>> {
    let step = body.map(|Json(req)| req.step).unwrap_or_default();

    let incident = state
        .alerting
        .actions
        .escalate(&actor(&ctx), &incident.id, step)
        .await?;

    Ok(incident_json(incident))
}

#[resource_permission(
    action = "alerts.incidents.acknowledge",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn snooze(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<SnoozeReq>,
) -> Result<Json<Value>> {
    let incident = state
        .alerting
        .actions
        .snooze(&actor(&ctx), &incident.id, req.duration_secs)
        .await?;

    Ok(incident_json(incident))
}

#[resource_permission(
    action = "alerts.incidents.acknowledge",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn cancel_snooze(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let incident = state
        .alerting
        .actions
        .cancel_snooze(&actor(&ctx), &incident.id)
        .await?;

    Ok(incident_json(incident))
}

#[resource_permission(
    action = "alerts.incidents.update",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn set_severity(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<SeverityReq>,
) -> Result<Json<Value>> {
    let incident = state
        .alerting
        .actions
        .set_severity(&actor(&ctx), &incident.id, req.severity)
        .await?;

    Ok(incident_json(incident))
}

#[resource_permission(
    action = "alerts.incidents.escalate",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn merge(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<MergeReq>,
) -> Result<Json<Value>> {
    // 并入的每个事件也要有处理它的权限。
    for source_id in &req.source_ids {
        authorize_resource::<Incident>(
            &state,
            &ctx,
            source_id.clone(),
            "alerts.incidents.escalate",
        )
        .await?;
    }

    let incident = state
        .alerting
        .actions
        .merge(&actor(&ctx), &incident.id, &req.source_ids)
        .await?;

    Ok(incident_json(incident))
}

#[resource_permission(
    action = "alerts.incidents.update",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn add_note(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<NoteReq>,
) -> Result<Json<Value>> {
    let entry = state
        .alerting
        .actions
        .note(&actor(&ctx), &incident.id, &req.text)
        .await?;

    // 关联的工单上开了「备注写成评论」的，另起任务写过去。
    let tickets = state.alerting.tickets.clone();
    let text = req.text.clone();
    tokio::spawn(async move { tickets.on_note(&incident, &text).await });

    Ok(Json(serde_json::to_value(entry).unwrap_or(Value::Null)))
}

#[resource_permission(
    action = "alerts.incidents.read",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn timeline(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let timeline = state
        .alerting
        .actions
        .timeline(&actor(&ctx), &incident.id)
        .await?;

    Ok(Json(serde_json::to_value(timeline).unwrap_or(Value::Null)))
}

#[permission("alerts.incidents.acknowledge")]
async fn bulk(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<BulkReq>,
) -> Result<Json<Value>> {
    let mut ids: Vec<Id> = Vec::new();

    for id in req.ids {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }

    if ids.is_empty() {
        return Err(Error::invalid("name at least one incident"));
    }

    if ids.len() > MAX_BULK_INCIDENTS {
        return Err(Error::invalid(format!(
            "at most {MAX_BULK_INCIDENTS} incidents at a time"
        )));
    }

    let mut allowed = Vec::new();
    let mut results = Vec::new();

    for id in ids {
        match authorize_resource::<Incident>(
            &state,
            &ctx,
            id.clone(),
            "alerts.incidents.acknowledge",
        )
        .await
        {
            Ok(_) => allowed.push(id),
            Err(error) => results.push(BulkResult {
                id,
                error: Some(error.to_string()),
            }),
        }
    }

    results.extend(
        state
            .alerting
            .actions
            .bulk(&actor(&ctx), &allowed, &req.action)
            .await,
    );

    Ok(Json(json!({ "results": results })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bulk_request_names_its_action_beside_the_ids() {
        let reassign: BulkReq = serde_json::from_value(json!({
            "ids": ["inc-1", "inc-2"],
            "action": "reassign",
            "to": { "kind": "team", "teamId": "team-1" },
        }))
        .expect("reassign");

        assert_eq!(reassign.ids.len(), 2);
        assert_eq!(
            reassign.action,
            BulkAction::Reassign {
                to: Reassignment::Team {
                    team_id: Id::from_string("team-1")
                }
            }
        );

        let snooze: BulkReq = serde_json::from_value(json!({
            "ids": ["inc-1"],
            "action": "snooze",
            "durationSecs": 3600,
        }))
        .expect("snooze");

        assert_eq!(
            snooze.action,
            BulkAction::Snooze {
                duration_secs: 3600
            }
        );

        let unknown = serde_json::from_value::<BulkReq>(json!({ "ids": [], "action": "delete" }));
        assert!(unknown.is_err());
    }
}
