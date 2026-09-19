// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Organization-scoped Probe Agent inventory and lifecycle actions.

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{iam::IamContext, synthetics::UpdateAgentConfigurationInput},
    domain::{iam::permission, synthetics::ProbeAgent},
    shared::{Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/synthetics/agents", get(list))
        .route(
            "/synthetics/agents/{agent_id}/configuration",
            axum::routing::put(update_configuration),
        )
        .route(
            "/synthetics/agents/{agent_id}/revoke",
            axum::routing::post(revoke),
        )
}

#[permission("synthetics.read")]
async fn list(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<ProbeAgent>>> {
    Ok(Json(
        state.synthetics.list_all_agents(&context.org_id).await?,
    ))
}

#[permission("synthetics.locations.manage")]
async fn update_configuration(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(agent_id): Path<String>,
    Json(input): Json<UpdateAgentConfigurationInput>,
) -> Result<Json<ProbeAgent>> {
    let agent = state
        .synthetics
        .update_agent_configuration(&context.org_id, &context.user_id, &Id(agent_id), input)
        .await?;
    activity_audit::record(
        &state,
        &context,
        "synthetic.agent.configuration.update",
        "synthetic_agent",
        agent.id.as_str(),
        serde_json::json!({
            "name": agent.name.clone(),
            "label_count": agent.labels.len(),
        }),
    )
    .await;
    Ok(Json(agent))
}

#[permission("synthetics.locations.manage")]
async fn revoke(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(agent_id): Path<String>,
) -> Result<Json<ProbeAgent>> {
    let agent = state
        .synthetics
        .revoke_agent(&context.org_id, &Id(agent_id))
        .await?;
    activity_audit::record(
        &state,
        &context,
        "synthetic.agent.revoke",
        "synthetic_agent",
        agent.id.as_str(),
        serde_json::json!({"location_id": agent.location_id}),
    )
    .await;
    Ok(Json(agent))
}
