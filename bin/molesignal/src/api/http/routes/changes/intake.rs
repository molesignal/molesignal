// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `POST /api/v1/intake/changes`: changes from webhooks that cannot be shaped like
//! `POST /apm/deployments`. A tool that cannot set a header puts the API token in `?token=`, so the
//! authentication layer lets the path through and the token is checked here: it must hold
//! `apm.deploy`. The address names the body's format and what the body leaves unsaid
//! ([`IntakeDefaults`]); the body is at most 1 MiB.

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Query, RawQuery, State},
    http::{HeaderMap, StatusCode},
    routing::post,
};
use domain::apm::DeploymentEvent;
use kernel::{Error, Result};
use serde_json::Value;

use crate::{
    api::{
        AppState,
        http::{
            middleware::auth::authenticate_api_token_identity_with_metadata,
            routes::{activity_audit, alerting::presented_token},
        },
    },
    app::changes::{
        ChangeRecorder,
        intake::{IntakeDefaults, read_changes},
    },
};

/// The intake's path.
pub const CHANGE_INTAKE_PATH: &str = "/api/v1/intake/changes";
const MAX_BODY_BYTES: usize = 1024 * 1024;

pub fn public_routes() -> Router<AppState> {
    Router::new().route(
        CHANGE_INTAKE_PATH,
        post(intake).layer(DefaultBodyLimit::max(MAX_BODY_BYTES)),
    )
}

async fn intake(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
    Query(defaults): Query<IntakeDefaults>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<Vec<DeploymentEvent>>)> {
    let token = presented_token(&headers, query.as_deref())
        .ok_or_else(|| Error::unauthorized("send an API token in Authorization or in ?token="))?;

    let identity = authenticate_api_token_identity_with_metadata(
        &token,
        state.iam.service.as_ref(),
        state.iam.api_tokens.clone(),
    )
    .await?;

    let mut ctx = identity.context;
    state.iam.access.enrich_context(&mut ctx).await?;
    state
        .iam
        .service
        .ensure_organization_access(&ctx.org_id)
        .await?;

    if !ctx.has_permission("apm.deploy") {
        return Err(Error::forbidden("reporting changes needs apm.deploy"));
    }

    let body: Value = serde_json::from_slice(&body)
        .map_err(|error| Error::invalid(format!("the body is no JSON: {error}")))?;
    let changes = read_changes(&body, &defaults)?;

    let events = ChangeRecorder::new(state.telemetry.apm_catalog.clone())
        .record(&ctx.org_id, changes, ctx.principal_id())
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "apm.deployment.record",
        "apm_deployment",
        events.first().map_or("", |event| event.id.as_str()),
        serde_json::json!({ "count": events.len(), "format": format!("{:?}", defaults.format) }),
    )
    .await;

    Ok((StatusCode::CREATED, Json(events)))
}
