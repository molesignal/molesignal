// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header::CACHE_CONTROL},
    routing::{get, post},
};

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{
        iam::IamContext,
        synthetics::{CreateAgentTokenInput, RotateAgentTokenInput},
    },
    domain::{
        iam::permission,
        synthetics::{ProbeAgentToken, ProbeAgentTokenInstructions},
    },
    shared::{Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/synthetics/agent-tokens", get(list).post(create))
        .route("/synthetics/agent-tokens/{token_id}/rotate", post(rotate))
        .route("/synthetics/agent-tokens/{token_id}/disable", post(disable))
}

#[permission("synthetics.read")]
async fn list(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<ProbeAgentToken>>> {
    Ok(Json(
        state.synthetics.list_agent_tokens(&context.org_id).await?,
    ))
}

#[permission("synthetics.locations.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(input): Json<CreateAgentTokenInput>,
) -> Result<(StatusCode, HeaderMap, Json<ProbeAgentTokenInstructions>)> {
    let instructions = state
        .synthetics
        .create_agent_token(&context.org_id, &context.user_id, input)
        .await?;
    activity_audit::record(
        &state,
        &context,
        "synthetic.agent_token.create",
        "synthetic_agent_token",
        instructions.token.id.as_str(),
        serde_json::json!({
            "location_id": instructions.token.location_id,
            "expires_at": instructions.token.expires_at,
        }),
    )
    .await;
    Ok((StatusCode::CREATED, no_store_headers(), Json(instructions)))
}

#[permission("synthetics.locations.manage")]
async fn rotate(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(token_id): Path<String>,
    Json(input): Json<RotateAgentTokenInput>,
) -> Result<(HeaderMap, Json<ProbeAgentTokenInstructions>)> {
    let instructions = state
        .synthetics
        .rotate_agent_token(&context.org_id, &Id(token_id), input)
        .await?;
    activity_audit::record(
        &state,
        &context,
        "synthetic.agent_token.rotate",
        "synthetic_agent_token",
        instructions.token.id.as_str(),
        serde_json::json!({"expires_at": instructions.token.expires_at}),
    )
    .await;
    Ok((no_store_headers(), Json(instructions)))
}

#[permission("synthetics.locations.manage")]
async fn disable(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(token_id): Path<String>,
) -> Result<Json<ProbeAgentToken>> {
    let token = state
        .synthetics
        .disable_agent_token(&context.org_id, &Id(token_id))
        .await?;
    activity_audit::record(
        &state,
        &context,
        "synthetic.agent_token.disable",
        "synthetic_agent_token",
        token.id.as_str(),
        serde_json::json!({"location_id": token.location_id}),
    )
    .await;
    Ok(Json(token))
}

fn no_store_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers
}
