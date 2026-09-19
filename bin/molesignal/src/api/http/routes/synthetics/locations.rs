// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{iam::IamContext, synthetics::CreateLocationInput},
    domain::{
        iam::permission,
        synthetics::{LocationLifecycle, ProbeAgent, ProbeLocation, ProbeRegisterInstructions},
    },
    shared::{Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/synthetics/locations", get(list).post(create))
        .route(
            "/synthetics/locations/{location_id}/register-tokens",
            axum::routing::post(create_register_token),
        )
        .route(
            "/synthetics/locations/{location_id}/agents",
            get(list_agents),
        )
        .route(
            "/synthetics/locations/{location_id}/lifecycle",
            axum::routing::put(set_lifecycle),
        )
}

#[derive(Debug, serde::Deserialize)]
struct RegisterTokenRequest {
    #[serde(default = "default_register_ttl")]
    ttl_minutes: u32,
}

#[derive(Debug, serde::Deserialize)]
struct LocationLifecycleRequest {
    lifecycle: LocationLifecycle,
}

const fn default_register_ttl() -> u32 {
    15
}

#[permission("synthetics.read")]
async fn list(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<ProbeLocation>>> {
    Ok(Json(
        state.synthetics.list_locations(&context.org_id).await?,
    ))
}

#[permission("synthetics.locations.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(input): Json<CreateLocationInput>,
) -> Result<(StatusCode, Json<ProbeLocation>)> {
    let location = state
        .synthetics
        .create_location(&context.org_id, input)
        .await?;
    activity_audit::record(
        &state,
        &context,
        "synthetic.location.create",
        "synthetic_location",
        location.id.as_str(),
        serde_json::json!({"execution": location.execution, "scope": location.scope}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(location)))
}

#[permission("synthetics.locations.manage")]
async fn create_register_token(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(location_id): Path<String>,
    Json(request): Json<RegisterTokenRequest>,
) -> Result<(StatusCode, Json<ProbeRegisterInstructions>)> {
    let instructions = state
        .synthetics
        .create_register_token(
            &context.org_id,
            &context.user_id,
            &Id(location_id),
            request.ttl_minutes,
        )
        .await?;
    activity_audit::record(
        &state,
        &context,
        "synthetic.location.register_token.create",
        "synthetic_location",
        instructions.token.location_id.as_str(),
        serde_json::json!({
            "token_id": instructions.token.id,
            "expires_at": instructions.token.expires_at,
        }),
    )
    .await;
    Ok((StatusCode::CREATED, Json(instructions)))
}

#[permission("synthetics.read")]
async fn list_agents(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(location_id): Path<String>,
) -> Result<Json<Vec<ProbeAgent>>> {
    Ok(Json(
        state
            .synthetics
            .list_agents(&context.org_id, &Id(location_id))
            .await?,
    ))
}

#[permission("synthetics.locations.manage")]
async fn set_lifecycle(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(location_id): Path<String>,
    Json(request): Json<LocationLifecycleRequest>,
) -> Result<Json<ProbeLocation>> {
    Ok(Json(
        state
            .synthetics
            .set_location_lifecycle(&context.org_id, &Id(location_id), request.lifecycle)
            .await?,
    ))
}
