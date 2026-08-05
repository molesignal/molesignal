// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Value, json};

use super::OperationOutcome;
use crate::{
    api::{
        AppState,
        http::{federation::emit_cud, routes::activity_audit},
    },
    app::iam::IamContext,
    domain::{
        dashboard::authoring::DraftConsumption,
        federation::{CudAction, ResourceKind},
    },
    intelligence::model::ApprovalRequest,
    shared::{Error, Result, ids::Id},
};

pub(super) async fn execute(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let expected_hash = expected_hash(&approval.parameters)?;
    let consumption = state
        .intelligence
        .dashboard_authoring
        .create_from_draft(
            ctx.org_id.clone(),
            approval.requested_by.clone(),
            Id(approval.target.clone()),
            expected_hash.to_string(),
        )
        .await?;
    let dashboard = consumption.dashboard();
    let route = format!("/dashboards/{}", dashboard.id.0);
    let replayed = consumption.replayed();

    if matches!(&consumption, DraftConsumption::Created(_)) {
        emit_cud(
            state,
            &ctx.org_id,
            ResourceKind::Dashboard,
            CudAction::Created,
            &dashboard.id.0,
            dashboard,
        )
        .await;
        activity_audit::record(
            state,
            ctx,
            "dashboard.created_from_ai_draft",
            "dashboard",
            &dashboard.id.0,
            json!({
                "draft_id": approval.target,
                "model_hash": expected_hash,
                "route": route,
                "approval_id": approval.id,
            }),
        )
        .await;
    }

    Ok(OperationOutcome {
        summary: if replayed {
            format!("Dashboard already existed; returning {route}")
        } else {
            format!("Dashboard created successfully at {route}")
        },
        verification: json!({
            "verified": true,
            "dashboard_id": dashboard.id,
            "dashboard_route": route,
            "draft_id": approval.target,
            "draft_consumed": true,
            "replayed": replayed,
            "model_hash": expected_hash,
        }),
    })
}

fn expected_hash(parameters: &Value) -> Result<&str> {
    let values = parameters
        .as_object()
        .ok_or_else(|| Error::invalid("create_dashboard parameters must be an object"))?;
    if values.len() != 1 {
        return Err(Error::invalid(
            "create_dashboard parameters may contain only expected_hash",
        ));
    }
    values
        .get("expected_hash")
        .and_then(Value::as_str)
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| Error::invalid("create_dashboard expected_hash is invalid"))
}
