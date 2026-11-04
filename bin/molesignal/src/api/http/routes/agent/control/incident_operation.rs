// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 提议、人批准之后，对一个事件做的处理：转派、增派、立即升级、暂缓、改严重度、合并与
//! 写备注。参数在审批的 `parameters` 里；以批准执行的人的身份做，时间线上记为 Mole Agent。

use serde::Deserialize;
use serde_json::Value;

use super::approval_execution::OperationOutcome;
use crate::{
    agent::model::ApprovalRequest,
    api::{AppState, http::middleware::authorize_resource},
    app::{
        alerting::{Actor, actions::Reassignment},
        iam::IamContext,
    },
    domain::alerting::{
        incident::{Incident, Severity},
        priority::Priority,
        timeline::ActionVia,
        urgency::Urgency,
    },
    shared::{Error, Result, ids::Id},
};

/// 这些操作由这里执行。
pub(super) const ACTIONS: [&str; 16] = [
    "reassign_alert",
    "add_alert_responders",
    "escalate_alert",
    "snooze_alert",
    "lift_alert_inhibition",
    "set_alert_severity",
    "set_alert_priority",
    "set_alert_urgency",
    "merge_alerts",
    "note_alert",
    "create_incident_ticket",
    "declare_major_incident",
    "assign_incident_role",
    "post_incident_update",
    "add_incident_task",
    "draft_incident_postmortem",
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TicketParams {
    connector_id: Id,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeclareParams {
    #[serde(default)]
    commander_id: Option<Id>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoleParams {
    role: crate::domain::alerting::response::ResponseRole,
    /// 担任这个角色的人；为空时撤下。
    #[serde(default)]
    assignee_id: Option<Id>,
}

#[derive(Debug, Deserialize)]
struct UpdateParams {
    #[serde(default)]
    impact: String,
    #[serde(default)]
    current: String,
    #[serde(default)]
    next: String,
}

#[derive(Debug, Deserialize)]
struct TaskParams {
    title: String,
}

#[derive(Debug, Deserialize)]
struct PostmortemParams {
    #[serde(default)]
    title: String,
    body: String,
}

#[derive(Debug, Deserialize)]
struct ReassignParams {
    to: Reassignment,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsersParams {
    user_ids: Vec<Id>,
}

#[derive(Debug, Default, Deserialize)]
struct EscalateParams {
    #[serde(default)]
    step: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnoozeParams {
    duration_secs: u32,
}

#[derive(Debug, Deserialize)]
struct SeverityParams {
    severity: Severity,
}

#[derive(Debug, Deserialize)]
struct PriorityParams {
    #[serde(default)]
    priority: Option<Priority>,
}

#[derive(Debug, Deserialize)]
struct UrgencyParams {
    urgency: Urgency,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MergeParams {
    source_ids: Vec<Id>,
}

#[derive(Debug, Deserialize)]
struct NoteParams {
    text: String,
}

fn params<T: for<'de> Deserialize<'de>>(approval: &ApprovalRequest) -> Result<T> {
    let parameters = match &approval.parameters {
        Value::Null => Value::Object(Default::default()),
        other => other.clone(),
    };

    serde_json::from_value(parameters).map_err(|error| {
        Error::invalid(format!(
            "the parameters of `{}` are not right: {error}",
            approval.action
        ))
    })
}

pub(super) async fn execute(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
    incident: &Incident,
) -> Result<OperationOutcome> {
    let actions = &state.alerting.actions;
    let actor = Actor::new(&ctx.org_id, ctx.principal_id(), ActionVia::Agent);
    let id = &incident.id;

    let summary = match approval.action.as_str() {
        "reassign_alert" => {
            let ReassignParams { to } = params(approval)?;
            actions.reassign(&actor, id, to).await?;
            "alert reassigned"
        }
        "add_alert_responders" => {
            let UsersParams { user_ids } = params(approval)?;
            actions.add_responders(&actor, id, &user_ids).await?;
            "responders added to the alert"
        }
        "escalate_alert" => {
            let EscalateParams { step } = params(approval)?;
            actions.escalate(&actor, id, step).await?;
            "alert escalated"
        }
        "lift_alert_inhibition" => {
            actions.lift_inhibition(&actor, id).await?;
            "alert inhibition lifted"
        }
        "snooze_alert" => {
            let SnoozeParams { duration_secs } = params(approval)?;
            actions.snooze(&actor, id, duration_secs).await?;
            "alert snoozed"
        }
        "set_alert_severity" => {
            let SeverityParams { severity } = params(approval)?;
            actions.set_severity(&actor, id, severity).await?;
            "alert severity set"
        }
        "set_alert_priority" => {
            let PriorityParams { priority } = params(approval)?;
            actions.set_priority(&actor, id, priority).await?;
            "alert priority set"
        }
        "set_alert_urgency" => {
            let UrgencyParams { urgency } = params(approval)?;
            actions.set_urgency(&actor, id, urgency).await?;
            "alert urgency set"
        }
        "merge_alerts" => {
            let MergeParams { source_ids } = params(approval)?;

            // 并入的每个事件也要有处理它的权限。
            for source_id in &source_ids {
                authorize_resource::<Incident>(
                    state,
                    ctx,
                    source_id.clone(),
                    "alerts.incidents.escalate",
                )
                .await?;
            }

            actions.merge(&actor, id, &source_ids).await?;
            "alerts merged"
        }
        "note_alert" => {
            let NoteParams { text } = params(approval)?;
            actions.note(&actor, id, &text).await?;
            "note written on the alert's timeline"
        }
        "create_incident_ticket" => {
            let TicketParams { connector_id } = params(approval)?;
            state
                .alerting
                .tickets
                .create_with(
                    &ctx.org_id,
                    &connector_id,
                    incident,
                    Some(ctx.principal_id()),
                )
                .await?;
            "ticket created for the alert"
        }
        "declare_major_incident" => {
            let DeclareParams { commander_id } = params(approval)?;
            state
                .alerting
                .response
                .declare(incident, Some(ctx.principal_id()), commander_id)
                .await?;
            "alert declared a major incident"
        }
        "assign_incident_role" => {
            let RoleParams { role, assignee_id } = params(approval)?;
            state
                .alerting
                .response
                .assign_role(incident, role, assignee_id, ctx.principal_id())
                .await?;
            "major incident role assigned"
        }
        "post_incident_update" => {
            let UpdateParams {
                impact,
                current,
                next,
            } = params(approval)?;
            state
                .alerting
                .response
                .status_update(
                    incident,
                    crate::domain::alerting::response::StatusUpdate {
                        impact,
                        current,
                        next,
                    },
                    ctx.principal_id(),
                )
                .await?;
            "major incident update posted"
        }
        "add_incident_task" => {
            let TaskParams { title } = params(approval)?;
            state
                .alerting
                .response
                .add_task(incident, &title, None, ctx.principal_id())
                .await?;
            "task added to the major incident"
        }
        "draft_incident_postmortem" => {
            let PostmortemParams { title, body } = params(approval)?;
            let postmortems = &state.alerting.postmortems;

            match postmortems
                .repository()
                .for_incident(&ctx.org_id, id)
                .await?
            {
                None => {
                    postmortems
                        .create(incident, &title, &body, ctx.principal_id())
                        .await?;
                }
                Some(existing)
                    if existing.status
                        == crate::domain::alerting::postmortem::PostmortemStatus::Draft =>
                {
                    postmortems
                        .update(
                            &ctx.org_id,
                            &existing.id,
                            crate::app::alerting::postmortems::PostmortemPatch {
                                title: (!title.trim().is_empty()).then_some(title),
                                body: Some(body),
                                ..Default::default()
                            },
                        )
                        .await?;
                }
                Some(_) => {
                    return Err(Error::conflict(
                        "the postmortem is past its draft; people revise it from here on",
                    ));
                }
            }

            "postmortem draft saved"
        }
        other => {
            return Err(Error::invalid(format!(
                "operation `{other}` is not registered"
            )));
        }
    };

    Ok(OperationOutcome::verified_summary(summary))
}
