// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 手动建事件（`alerts.incidents.create`）：`POST /api/v1/alerts/incidents`。
//!
//! 请求见 [`NewIncident`]：标题、说明、严重度、紧急度、优先级、交给谁（按路由分派、团队、升级策略
//! 或指定成员）、受影响的服务、标签，以及要不要同时宣布为重大事件。返回建好的事件。

use axum::{Extension, Json, extract::State};
use serde_json::{Value, json};

use super::incident_actions::{actor, incident_json};
use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{
        alerting::actions::{NewIncident, NewIncidentAssignment},
        iam::IamContext,
    },
    domain::iam::permission,
    shared::Result,
};

fn assignment_kind(assign: &NewIncidentAssignment) -> &'static str {
    match assign {
        NewIncidentAssignment::Auto => "auto",
        NewIncidentAssignment::Team { .. } => "team",
        NewIncidentAssignment::Policy { .. } => "policy",
        NewIncidentAssignment::Users { .. } => "users",
    }
}

#[permission("alerts.incidents.create")]
pub(super) async fn create_incident(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(new): Json<NewIncident>,
) -> Result<Json<Value>> {
    let declare_major = new.declare_major;
    let assigned = assignment_kind(&new.assign);

    let incident = state.alerting.actions.create(&actor(&ctx), new).await?;

    activity_audit::record(
        &state,
        &ctx,
        "alert.incident.created",
        "incident",
        &incident.id.0,
        json!({ "severity": incident.severity, "assign": assigned }),
    )
    .await;

    // 宣布不成（例如重大事件的设置读不出来）不影响已经建好的事件：记日志，人可以在详情页再宣布。
    if declare_major {
        match state
            .alerting
            .response
            .declare(&incident, Some(ctx.principal_id()), None)
            .await
        {
            Ok(_) => {
                activity_audit::record(
                    &state,
                    &ctx,
                    "incident.declared_major",
                    "incident",
                    &incident.id.0,
                    json!({}),
                )
                .await;
            }
            Err(error) => {
                tracing::warn!(incident_id = %incident.id, %error, "a hand-made incident could not be declared major");
            }
        }
    }

    Ok(incident_json(incident))
}
