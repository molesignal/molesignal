// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::{Value, json};

use super::super::OperationOutcome;
use crate::{
    agent::model::ApprovalRequest,
    api::{
        AppState,
        http::{
            federation::{delete_payload, emit_cud},
            middleware::Permission,
        },
    },
    app::iam::IamContext,
    domain::federation::{CudAction, ResourceKind},
    shared::{Error, Result, ids::Id},
};

pub(super) async fn execute(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    match approval.action.as_str() {
        "create_dashboard_model" => create(state, ctx, approval).await,
        "update_dashboard" => update(state, ctx, approval).await,
        "delete_dashboard" => delete(state, ctx, approval).await,
        _ => unreachable!("dashboard operation received unrelated action"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DashboardWrite {
    model: Value,
    #[serde(default)]
    folder_id: Option<String>,
}

async fn create(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let parameters: DashboardWrite = parse(&approval.parameters)?;
    let folder_id = optional_id(parameters.folder_id);
    if let Some(folder_id) = &folder_id {
        state
            .dashboard
            .folders()
            .get(&ctx.org_id, folder_id)
            .await?;
    }
    let dashboard = state
        .dashboard
        .create(
            ctx.org_id.clone(),
            folder_id,
            approval.requested_by.clone(),
            parameters.model,
        )
        .await?;
    emit_cud(
        state,
        &dashboard.org_id,
        ResourceKind::Dashboard,
        CudAction::Created,
        &dashboard.id.0,
        &dashboard,
    )
    .await;
    Ok(OperationOutcome {
        summary: "dashboard created".into(),
        verification: json!({"verified": true, "dashboard": dashboard}),
    })
}

async fn update(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let parameters: DashboardWrite = parse(&approval.parameters)?;
    let dashboard = load_authorized(state, ctx, &approval.target, "dashboards.edit").await?;
    let folder_id = optional_id(parameters.folder_id);
    if let Some(folder_id) = &folder_id {
        state
            .dashboard
            .folders()
            .get(&dashboard.org_id, folder_id)
            .await?;
    }
    let dashboard = state
        .dashboard
        .update_model(
            dashboard,
            folder_id,
            approval.requested_by.clone(),
            parameters.model,
        )
        .await?;
    emit_cud(
        state,
        &dashboard.org_id,
        ResourceKind::Dashboard,
        CudAction::Updated,
        &dashboard.id.0,
        &dashboard,
    )
    .await;
    Ok(OperationOutcome {
        summary: "dashboard updated".into(),
        verification: json!({"verified": true, "dashboard": dashboard}),
    })
}

async fn delete(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let _: Empty = parse(&approval.parameters)?;
    let dashboard = load_authorized(state, ctx, &approval.target, "dashboards.delete").await?;
    state.dashboard.delete(&dashboard.id).await?;
    emit_cud(
        state,
        &dashboard.org_id,
        ResourceKind::Dashboard,
        CudAction::Deleted,
        &dashboard.id.0,
        &delete_payload(&dashboard.id.0),
    )
    .await;
    Ok(OperationOutcome {
        summary: "dashboard deleted".into(),
        verification: json!({"verified": true, "dashboard_id": dashboard.id, "deleted": true}),
    })
}

async fn load_authorized(
    state: &AppState,
    ctx: &IamContext,
    id: &str,
    permission: &str,
) -> Result<crate::domain::dashboard::Dashboard> {
    let dashboard = state.dashboard.get(&Id(id.to_string())).await?;
    if dashboard.org_id != ctx.org_id {
        return Err(Error::not_found("dashboard not found"));
    }
    Permission::require_resource(
        state,
        ctx,
        permission,
        &dashboard.org_id,
        "dashboard",
        &dashboard.id.0,
    )
    .await?;
    Ok(dashboard)
}

fn optional_id(value: Option<String>) -> Option<Id> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(Id)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("invalid operation parameters: {error}")))
}
