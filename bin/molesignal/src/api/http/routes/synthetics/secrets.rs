// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::Deserialize;

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{iam::IamContext, synthetics::CreateSecretInput},
    domain::{iam::permission, synthetics::SyntheticSecret},
    shared::{Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/synthetics/secrets", get(list).post(create))
        .route("/synthetics/secrets/{secret_id}/rotate", post(rotate))
}

#[derive(Deserialize)]
struct RotateSecretRequest {
    value: String,
}

#[permission("synthetics.read")]
async fn list(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<SyntheticSecret>>> {
    Ok(Json(state.synthetics.list_secrets(&context.org_id).await?))
}

#[permission("synthetics.secrets.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(input): Json<CreateSecretInput>,
) -> Result<(StatusCode, Json<SyntheticSecret>)> {
    let secret = state
        .synthetics
        .create_secret(&context.org_id, &context.user_id, input)
        .await?;
    audit(&state, &context, &secret, "create").await;
    Ok((StatusCode::CREATED, Json(secret)))
}

#[permission("synthetics.secrets.manage")]
async fn rotate(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(secret_id): Path<String>,
    Json(request): Json<RotateSecretRequest>,
) -> Result<Json<SyntheticSecret>> {
    let secret = state
        .synthetics
        .rotate_secret(
            &context.org_id,
            &context.user_id,
            &Id(secret_id),
            request.value,
        )
        .await?;
    audit(&state, &context, &secret, "rotate").await;
    Ok(Json(secret))
}

async fn audit(state: &AppState, context: &IamContext, secret: &SyntheticSecret, verb: &str) {
    activity_audit::record(
        state,
        context,
        &format!("synthetic.secret.{verb}"),
        "synthetic_secret",
        secret.id.as_str(),
        serde_json::json!({"version": secret.current_version}),
    )
    .await;
}
