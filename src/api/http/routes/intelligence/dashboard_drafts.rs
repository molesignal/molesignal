// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    control::{CreateApprovalRequest, create_agent_approval, dashboard_required_approvals},
    toolsets::resolve_toolsets,
};
use crate::{
    api::{
        AppState,
        http::{middleware::Permission, routes::activity_audit},
    },
    app::iam::IamContext,
    domain::{dashboard::authoring::DashboardDraftStatus, iam::permission},
    intelligence::{FEATURE, tools::BuiltinToolKind},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/intelligence/dashboard-authoring/capabilities",
            get(capabilities),
        )
        .route("/intelligence/dashboard-drafts/{id}", get(get_draft))
        .route("/intelligence/dashboard-drafts/{id}/propose", post(propose))
}

#[permission("intelligence.use")]
async fn capabilities(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    require_feature(&state)?;
    Permission::require_key(&ctx, "dashboards.create")?;
    Ok(Json(
        serde_json::to_value(
            state
                .intelligence
                .dashboard_authoring
                .capabilities()
                .await?,
        )
        .map_err(|error| Error::internal(error.to_string()))?,
    ))
}

#[permission("intelligence.use")]
async fn get_draft(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    require_feature(&state)?;
    let draft = state
        .intelligence
        .dashboard_authoring
        .get_draft_for_preview(&ctx.org_id, &Id(id))
        .await?;
    if draft.created_by != ctx.user_id {
        Permission::require_key(&ctx, "intelligence.approve")?;
    } else {
        Permission::require_key(&ctx, "dashboards.create")?;
    }
    let approval = state
        .intelligence
        .repository
        .list_approvals(&ctx.org_id)
        .await?
        .into_iter()
        .find(|approval| approval.action == "create_dashboard" && approval.target == draft.id.0);
    let operation = approval.map(|approval| {
        let review_count = approval
            .reviews
            .as_array()
            .into_iter()
            .flatten()
            .filter(|review| review["decision"] == "approved")
            .count();
        json!({
            "approval_id": approval.id,
            "status": approval.status,
            "required_approvals": approval.required_approvals,
            "approved_reviews": review_count,
            "expires_at": approval.expires_at,
        })
    });
    Ok(Json(json!({
        "draft_id": draft.id,
        "model_hash": draft.model_hash,
        "folder_id": draft.folder_id,
        "status": draft.status,
        "created_at": draft.created_at,
        "expires_at": draft.expires_at,
        "compiled_model": draft.compiled_model,
        "warnings": draft.warnings,
        "preflight": draft.preflight,
        "operation": operation,
        "dashboard_id": draft.dashboard_id,
        "dashboard_route": draft.dashboard_id.map(|id| format!("/dashboards/{}", id.0)),
        "can_propose": draft.status == DashboardDraftStatus::Ready
            && draft.expires_at > TimestampMicros::now(),
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposeRequest {
    expected_hash: String,
    reason: String,
    impact: String,
}

#[permission("intelligence.use")]
async fn propose(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<ProposeRequest>,
) -> Result<Json<Value>> {
    require_feature(&state)?;
    Permission::require_key(&ctx, "dashboards.create")?;
    let resolution = resolve_toolsets(&state, &ctx.org_id).await?;
    let tool_name = BuiltinToolKind::ProposeDashboardCreation.name();
    if !resolution.builtin_enabled(tool_name) {
        return Err(Error::forbidden(
            "Dashboard creation proposals are disabled by the active Agent Profile or Toolset",
        ));
    }
    let required_approvals =
        dashboard_required_approvals(resolution.execution_mode_for_builtin(tool_name))?;
    let approval = create_agent_approval(
        &state,
        &ctx,
        CreateApprovalRequest {
            investigation_id: None,
            action: "create_dashboard".into(),
            target: id,
            parameters: json!({"expected_hash": request.expected_hash}),
            reason: request.reason,
            impact: request.impact,
            expires_at_micros: None,
            required_approvals_override: Some(required_approvals),
        },
    )
    .await?;
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.approval.requested",
        "intelligence_approval",
        &approval.id.0,
        json!({
            "action": "create_dashboard",
            "target": approval.target,
            "risk": approval.risk,
            "required_approvals": approval.required_approvals,
        }),
    )
    .await;
    Ok(Json(json!({"approval": approval})))
}

fn require_feature(state: &AppState) -> Result<()> {
    if state.platform.license.has_feature(FEATURE) {
        Ok(())
    } else {
        Err(Error::forbidden(format!("{FEATURE} feature not licensed")))
    }
}
