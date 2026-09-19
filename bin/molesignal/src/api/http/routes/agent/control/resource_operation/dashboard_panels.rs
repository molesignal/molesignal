// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::{Value, json};

use super::super::OperationOutcome;
use crate::{
    agent::model::ApprovalRequest,
    api::{
        AppState,
        http::{federation::emit_cud, middleware::Permission},
    },
    app::iam::IamContext,
    domain::{
        dashboard::Dashboard,
        federation::{CudAction, ResourceKind},
    },
    shared::{Error, Result, ids::Id},
};

pub(super) async fn execute(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let dashboard_id = Id(approval.target.clone());
    let dashboard = state.dashboard.get(&dashboard_id).await?;
    if dashboard.org_id != ctx.org_id {
        return Err(Error::not_found("dashboard not found"));
    }
    Permission::require_resource(
        state,
        ctx,
        "dashboards.edit",
        &dashboard.org_id,
        "dashboard",
        &dashboard.id.0,
    )
    .await?;
    let saved = match approval.action.as_str() {
        "add_dashboard_panel" => {
            let parameters: AddPanel = parse(&approval.parameters)?;
            state
                .dashboard
                .add_panel(
                    dashboard,
                    approval.requested_by.clone(),
                    parameters.panel,
                    parameters.container_id.as_deref(),
                    parameters.position,
                    parameters.expected_version,
                )
                .await?
        }
        "update_dashboard_panel" => {
            let parameters: UpdatePanel = parse(&approval.parameters)?;
            state
                .dashboard
                .update_panel(
                    dashboard,
                    approval.requested_by.clone(),
                    &parameters.panel_id,
                    parameters.panel,
                    parameters.expected_version,
                )
                .await?
        }
        "move_dashboard_panel" => {
            let parameters: MovePanel = parse(&approval.parameters)?;
            state
                .dashboard
                .move_panel(
                    dashboard,
                    approval.requested_by.clone(),
                    &parameters.panel_id,
                    parameters.container_id.as_deref(),
                    parameters.position,
                    parameters.expected_version,
                )
                .await?
        }
        "delete_dashboard_panel" => {
            let parameters: DeletePanel = parse(&approval.parameters)?;
            state
                .dashboard
                .delete_panel(
                    dashboard,
                    approval.requested_by.clone(),
                    &parameters.panel_id,
                    parameters.expected_version,
                )
                .await?
        }
        _ => unreachable!("Dashboard panel operation received unrelated action"),
    };
    emit_cud(
        state,
        &saved.org_id,
        ResourceKind::Dashboard,
        CudAction::Updated,
        &saved.id.0,
        &saved,
    )
    .await;
    Ok(outcome(approval, saved))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddPanel {
    panel: Value,
    #[serde(default)]
    container_id: Option<String>,
    #[serde(default)]
    position: Option<usize>,
    #[serde(default)]
    expected_version: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdatePanel {
    panel_id: String,
    panel: Value,
    #[serde(default)]
    expected_version: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MovePanel {
    panel_id: String,
    #[serde(default)]
    container_id: Option<String>,
    position: usize,
    #[serde(default)]
    expected_version: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeletePanel {
    panel_id: String,
    #[serde(default)]
    expected_version: Option<u32>,
}

fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("invalid operation parameters: {error}")))
}

fn outcome(approval: &ApprovalRequest, dashboard: Dashboard) -> OperationOutcome {
    OperationOutcome {
        summary: format!("{} completed", approval.action),
        verification: json!({
            "verified": true,
            "dashboard_id": dashboard.id,
            "version": dashboard.version,
            "dashboard": dashboard,
        }),
    }
}
