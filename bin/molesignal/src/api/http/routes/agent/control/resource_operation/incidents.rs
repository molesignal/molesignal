// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 提议、人批准之后手动建事件：以批准执行的人的身份建，参数同
//! `POST /api/v1/alerts/incidents`。

use serde_json::json;

use super::super::OperationOutcome;
use crate::{
    agent::model::ApprovalRequest,
    api::AppState,
    app::{
        alerting::{Actor, actions::NewIncident},
        iam::IamContext,
    },
    domain::alerting::timeline::ActionVia,
    shared::{Error, Result},
};

pub(super) async fn create(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let new: NewIncident = serde_json::from_value(approval.parameters.clone())
        .map_err(|error| Error::invalid(format!("invalid operation parameters: {error}")))?;
    let declare_major = new.declare_major;

    let actor = Actor::new(&ctx.org_id, ctx.principal_id(), ActionVia::Agent);
    let incident = state.alerting.actions.create(&actor, new).await?;

    if declare_major
        && let Err(error) = state
            .alerting
            .response
            .declare(&incident, Some(ctx.principal_id()), None)
            .await
    {
        tracing::warn!(incident_id = %incident.id, %error, "a hand-made incident could not be declared major");
    }

    Ok(OperationOutcome {
        summary: "incident created".into(),
        verification: json!({"verified": true, "incident": incident}),
    })
}
