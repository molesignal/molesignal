// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post, put},
};
use domain::{
    iam::{IamContext, permission},
    node_agent::EnrollmentToken,
};
use kernel::{Result, ids::Id};
use serde_json::Value;

use crate::{
    api::AppState,
    app::node_agent::{Actor, CreateEnrollmentTokenInput, EnrollmentTokenIssued, NodeView},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/node-agents/enrollment-tokens",
            get(list_tokens).post(create_token),
        )
        .route(
            "/node-agents/enrollment-tokens/{tokenId}/revoke",
            post(revoke_token),
        )
        .route("/node-agents/nodes", get(list_nodes))
        .route("/node-agents/nodes/{nodeId}", get(get_node))
        .route("/node-agents/nodes/{nodeId}/drain", post(drain))
        .route("/node-agents/nodes/{nodeId}/resume", post(resume))
        .route("/node-agents/nodes/{nodeId}/revoke", post(revoke))
        .route("/node-agents/nodes/{nodeId}/config", put(set_config))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DrainRequest {
    #[serde(default = "default_drain_secs")]
    deadline_seconds: u32,
}

const fn default_drain_secs() -> u32 {
    600
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RevokeRequest {
    reason: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigRequest {
    config: Value,
}

#[permission("node_agents.enroll")]
async fn list_tokens(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<EnrollmentToken>>> {
    Ok(Json(
        state
            .node_agents
            .list_enrollment_tokens(&context.org_id)
            .await?,
    ))
}

#[permission("node_agents.enroll")]
async fn create_token(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(input): Json<CreateEnrollmentTokenInput>,
) -> Result<(StatusCode, Json<EnrollmentTokenIssued>)> {
    let issued = state
        .node_agents
        .create_enrollment_token(&context.org_id, &Actor::from_context(&context), input)
        .await?;

    Ok((StatusCode::CREATED, Json(issued)))
}

#[permission("node_agents.enroll")]
async fn revoke_token(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(token_id): Path<String>,
) -> Result<Json<EnrollmentToken>> {
    Ok(Json(
        state
            .node_agents
            .revoke_enrollment_token(
                &context.org_id,
                &Actor::from_context(&context),
                &Id(token_id),
            )
            .await?,
    ))
}

#[permission("node_agents.read")]
async fn list_nodes(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<NodeView>>> {
    Ok(Json(state.node_agents.list_nodes(&context.org_id).await?))
}

#[permission("node_agents.read")]
async fn get_node(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(node_id): Path<String>,
) -> Result<Json<NodeView>> {
    Ok(Json(
        state
            .node_agents
            .get_node(&context.org_id, &Id(node_id))
            .await?,
    ))
}

#[permission("node_agents.manage")]
async fn drain(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(node_id): Path<String>,
    Json(request): Json<DrainRequest>,
) -> Result<Json<NodeView>> {
    Ok(Json(
        state
            .node_agents
            .drain(
                &context.org_id,
                &Actor::from_context(&context),
                &Id(node_id),
                request.deadline_seconds,
            )
            .await?,
    ))
}

#[permission("node_agents.manage")]
async fn resume(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(node_id): Path<String>,
) -> Result<Json<NodeView>> {
    Ok(Json(
        state
            .node_agents
            .resume(
                &context.org_id,
                &Actor::from_context(&context),
                &Id(node_id),
            )
            .await?,
    ))
}

#[permission("node_agents.revoke")]
async fn revoke(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(node_id): Path<String>,
    Json(request): Json<RevokeRequest>,
) -> Result<Json<NodeView>> {
    Ok(Json(
        state
            .node_agents
            .revoke(
                &context.org_id,
                &Actor::from_context(&context),
                &Id(node_id),
                &request.reason,
            )
            .await?,
    ))
}

#[permission("node_agents.manage")]
async fn set_config(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(node_id): Path<String>,
    Json(request): Json<ConfigRequest>,
) -> Result<Json<NodeView>> {
    Ok(Json(
        state
            .node_agents
            .set_config(
                &context.org_id,
                &Actor::from_context(&context),
                &Id(node_id),
                request.config,
            )
            .await?,
    ))
}
