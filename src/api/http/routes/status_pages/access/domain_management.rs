// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::Deserialize;

use super::record_access_audit;
use crate::{
    api::AppState,
    app::{
        iam::IamContext,
        status_page::{
            StatusPageDomainCapability, StatusPageDomainInput, StatusPageDomainInstructions,
        },
    },
    domain::iam::permission,
    shared::{Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/status-pages/{page_id}/domain",
            get(get_domain).put(configure_domain).delete(remove_domain),
        )
        .route(
            "/status-pages/{page_id}/domain/capability",
            get(get_domain_capability),
        )
        .route("/status-pages/{page_id}/domain/verify", post(verify_domain))
        .route("/status-pages/{page_id}/domain/retry", post(retry_domain))
}

#[derive(Debug, Deserialize)]
struct DomainRequest {
    hostname: String,
}

#[permission("status_pages.manage")]
async fn get_domain_capability(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
) -> Result<Json<StatusPageDomainCapability>> {
    Ok(Json(
        state
            .status_pages
            .get_custom_domain_capability(&context.org_id, &Id(page_id))
            .await?,
    ))
}

#[permission("status_pages.manage")]
async fn get_domain(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
) -> Result<Json<Option<StatusPageDomainInstructions>>> {
    Ok(Json(
        state
            .status_pages
            .get_custom_domain(&context.org_id, &Id(page_id))
            .await?,
    ))
}

#[permission("status_pages.manage")]
async fn configure_domain(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
    Json(request): Json<DomainRequest>,
) -> Result<Json<StatusPageDomainInstructions>> {
    let page_id = Id(page_id);
    let result = state
        .status_pages
        .configure_custom_domain(
            &context.org_id,
            &page_id,
            StatusPageDomainInput {
                hostname: request.hostname,
            },
        )
        .await?;
    record_access_audit(
        &state,
        &context,
        "domain.configure",
        &page_id,
        serde_json::json!({
            "hostname": result.config.hostname,
            "state": result.config.state,
        }),
    )
    .await;
    Ok(Json(result))
}

#[permission("status_pages.manage")]
async fn verify_domain(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
) -> Result<Json<StatusPageDomainInstructions>> {
    let page_id = Id(page_id);
    let result = state
        .status_pages
        .verify_custom_domain(&context.org_id, &page_id)
        .await?;
    record_access_audit(
        &state,
        &context,
        "domain.verify",
        &page_id,
        serde_json::json!({
            "state": result.config.state,
            "routing_valid": result.config.routing_valid,
        }),
    )
    .await;
    Ok(Json(result))
}

#[permission("status_pages.manage")]
async fn retry_domain(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
) -> Result<Json<StatusPageDomainInstructions>> {
    let page_id = Id(page_id);
    let result = state
        .status_pages
        .retry_custom_domain_tls(&context.org_id, &page_id)
        .await?;
    record_access_audit(
        &state,
        &context,
        "domain.retry",
        &page_id,
        serde_json::json!({
            "state": result.config.state,
        }),
    )
    .await;
    Ok(Json(result))
}

#[permission("status_pages.manage")]
async fn remove_domain(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let page_id = Id(page_id);
    state
        .status_pages
        .remove_custom_domain(&context.org_id, &page_id)
        .await?;
    record_access_audit(
        &state,
        &context,
        "domain.remove",
        &page_id,
        serde_json::json!({}),
    )
    .await;
    Ok(Json(serde_json::json!({"deleted": true})))
}
