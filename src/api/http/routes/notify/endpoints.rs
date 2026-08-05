// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ensure_user_scope;
use crate::{
    api::AppState,
    app::{
        iam::IamContext,
        notify::{CreateUserNotifyEndpoint, UpdateUserNotifyEndpoint},
    },
    domain::notify::{connector::NotifyMessage, endpoint::UserNotifyEndpoint},
    shared::{Error, Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/users/{user_id}/notify-endpoints", get(list).post(create))
        .route(
            "/users/{user_id}/notify-endpoints/{id}",
            get(get_one).put(update).delete(delete),
        )
        .route(
            "/users/{user_id}/notify-endpoints/{id}/test",
            axum::routing::post(test),
        )
        .route(
            "/users/{user_id}/notify-endpoints/{id}/verify",
            axum::routing::post(verify),
        )
}

#[derive(Debug, Deserialize)]
struct CreateEndpointRequest {
    connector_id: Id,
    external_identity: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default = "empty_object")]
    metadata: Value,
    #[serde(default)]
    verified: bool,
    #[serde(default = "default_true")]
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct UpdateEndpointRequest {
    connector_id: Id,
    external_identity: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default = "empty_object")]
    metadata: Value,
    #[serde(default)]
    verified: Option<bool>,
    #[serde(default = "default_true")]
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct EndpointTestRequest {
    #[serde(default)]
    title: String,
    #[serde(default)]
    text: String,
}

#[derive(Debug, Serialize)]
struct EndpointTestResponse {
    sent: bool,
    tested_at_micros: i64,
    elapsed_ms: u64,
    error: Option<String>,
}

fn empty_object() -> Value {
    serde_json::json!({})
}

fn default_true() -> bool {
    true
}

fn may_attest_verification(context: &IamContext) -> bool {
    context.has_permission("org.members.manage")
}

fn test_message(request: EndpointTestRequest) -> NotifyMessage {
    NotifyMessage {
        title: if request.title.trim().is_empty() {
            "MoleSignal notify endpoint test".into()
        } else {
            request.title
        },
        text: if request.text.trim().is_empty() {
            "Your MoleSignal notify endpoint is configured correctly.".into()
        } else {
            request.text
        },
        markdown: None,
        html: None,
        metadata: Default::default(),
    }
}

async fn list(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(user_id): Path<String>,
) -> Result<Json<Vec<UserNotifyEndpoint>>> {
    let user_id = Id::from_string(user_id);
    ensure_user_scope(&state, &context, &user_id, false).await?;
    Ok(Json(
        state
            .alerting
            .notify
            .list_endpoints(&context.org_id, &user_id)
            .await?,
    ))
}

async fn get_one(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((user_id, id)): Path<(String, String)>,
) -> Result<Json<UserNotifyEndpoint>> {
    let user_id = Id::from_string(user_id);
    ensure_user_scope(&state, &context, &user_id, false).await?;
    Ok(Json(
        state
            .alerting
            .notify
            .get_endpoint(&context.org_id, &user_id, &Id::from_string(id))
            .await?,
    ))
}

async fn create(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(user_id): Path<String>,
    Json(request): Json<CreateEndpointRequest>,
) -> Result<(StatusCode, Json<UserNotifyEndpoint>)> {
    let user_id = Id::from_string(user_id);
    ensure_user_scope(&state, &context, &user_id, true).await?;
    if request.verified && !may_attest_verification(&context) {
        return Err(Error::forbidden(
            "org.members.manage is required to attest endpoint verification",
        ));
    }
    let endpoint = state
        .alerting
        .notify
        .create_endpoint(
            &context.org_id,
            &user_id,
            CreateUserNotifyEndpoint {
                connector_id: request.connector_id,
                external_identity: request.external_identity,
                display_name: request.display_name,
                metadata: request.metadata,
                verified: request.verified,
                enabled: request.enabled,
            },
        )
        .await?;
    Ok((StatusCode::CREATED, Json(endpoint)))
}

async fn update(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((user_id, id)): Path<(String, String)>,
    Json(request): Json<UpdateEndpointRequest>,
) -> Result<Json<UserNotifyEndpoint>> {
    let user_id = Id::from_string(user_id);
    ensure_user_scope(&state, &context, &user_id, true).await?;
    let existing = state
        .alerting
        .notify
        .get_endpoint(&context.org_id, &user_id, &Id::from_string(&id))
        .await?;
    let identity_changed = request.connector_id != existing.connector_id
        || request.external_identity.trim() != existing.external_identity;
    let verified = if identity_changed {
        request.verified.unwrap_or(false)
    } else {
        request.verified.unwrap_or(existing.verified)
    };
    if verified
        && (identity_changed || verified != existing.verified)
        && !may_attest_verification(&context)
    {
        return Err(Error::forbidden(
            "org.members.manage is required to change endpoint verification",
        ));
    }
    Ok(Json(
        state
            .alerting
            .notify
            .update_endpoint(
                &context.org_id,
                &user_id,
                &Id::from_string(id),
                UpdateUserNotifyEndpoint {
                    connector_id: request.connector_id,
                    external_identity: request.external_identity,
                    display_name: request.display_name,
                    metadata: request.metadata,
                    verified,
                    enabled: request.enabled,
                },
            )
            .await?,
    ))
}

async fn delete(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((user_id, id)): Path<(String, String)>,
) -> Result<StatusCode> {
    let user_id = Id::from_string(user_id);
    ensure_user_scope(&state, &context, &user_id, true).await?;
    state
        .alerting
        .notify
        .delete_endpoint(&context.org_id, &user_id, &Id::from_string(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn test(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((user_id, id)): Path<(String, String)>,
    Json(request): Json<EndpointTestRequest>,
) -> Result<Json<EndpointTestResponse>> {
    let user_id = Id::from_string(user_id);
    ensure_user_scope(&state, &context, &user_id, true).await?;
    let outcome = state
        .alerting
        .notify
        .test_endpoint(
            &context.org_id,
            &user_id,
            &Id::from_string(id),
            test_message(request),
        )
        .await?;
    Ok(Json(EndpointTestResponse {
        sent: outcome.sent,
        tested_at_micros: outcome.tested_at.0,
        elapsed_ms: outcome.elapsed_ms,
        error: outcome.error,
    }))
}

async fn verify(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((user_id, id)): Path<(String, String)>,
) -> Result<Json<UserNotifyEndpoint>> {
    if !may_attest_verification(&context) {
        return Err(Error::forbidden(
            "org.members.manage is required to attest endpoint verification",
        ));
    }
    let user_id = Id::from_string(user_id);
    ensure_user_scope(&state, &context, &user_id, true).await?;
    let endpoint = state
        .alerting
        .notify
        .get_endpoint(&context.org_id, &user_id, &Id::from_string(id))
        .await?;
    Ok(Json(
        state
            .alerting
            .notify
            .update_endpoint(
                &context.org_id,
                &user_id,
                &endpoint.id,
                UpdateUserNotifyEndpoint {
                    connector_id: endpoint.connector_id,
                    external_identity: endpoint.external_identity,
                    display_name: endpoint.display_name,
                    metadata: endpoint.metadata,
                    verified: true,
                    enabled: endpoint.enabled,
                },
            )
            .await?,
    ))
}
