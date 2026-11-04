// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Every token of the organization.
//!
//! Whoever reads API tokens sees all of them. Whoever manages them issues tokens for service
//! accounts, revokes any token, and hands out an application's RUM client token. People issue
//! their own tokens themselves, under `/me/api-tokens`.

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    response::Response,
    routing::{delete, get},
};
use serde::Deserialize;

use super::{
    IssuedTokenResp, ManagedTokenResp, RequestedScope, TokenResp, TokenSpec, expires_at,
    issue::require_held, issue_scope, new_token, require_organization_scope, secret_response,
    validate_name,
};
use crate::{
    api::{AppState, http::middleware::Permission},
    app::iam::IamContext,
    domain::{
        iam::api_token::{ApiTokenKind, ApiTokenScope, RUM_CLIENT_PERMISSION},
        rum::validate_application_id,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/tokens", get(list).post(create))
        .route("/auth/tokens/rum", get(rum_client))
        .route("/auth/tokens/{id}", delete(revoke))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateReq {
    name: String,
    /// The service account the token authenticates.
    service_account_id: String,
    #[serde(flatten)]
    scope: RequestedScope,
    /// Without it the token never expires.
    #[serde(default)]
    expires_in_days: Option<i64>,
}

async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<TokenResp>>> {
    require_organization_scope(&ctx)?;
    Permission::require_key(&ctx, "api_tokens.read")?;
    let tokens = state.iam.api_tokens.list_by_org(&ctx.org_id).await?;
    Ok(Json(tokens.into_iter().map(TokenResp::from).collect()))
}

/// Issues a token for a service account. It acts as the account, within the scope asked for.
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Response> {
    require_organization_scope(&ctx)?;
    Permission::require_key(&ctx, "api_tokens.issue")?;
    let name = validate_name(&req.name)?;

    let account = state
        .iam
        .service_accounts
        .get(&ctx.org_id, &Id::from_string(req.service_account_id.trim()))
        .await?;

    if account.disabled {
        return Err(Error::invalid(
            "an API token cannot be issued for a disabled service account",
        ));
    }

    let scope = issue_scope(&state, &ctx, req.scope).await?;
    let now = TimestampMicros::now();

    let (row, token) = new_token(
        TokenSpec {
            id: Id::new(),
            org_id: ctx.org_id.clone(),
            user_id: ctx.principal_id().clone(),
            name,
            scope,
            token_kind: ApiTokenKind::ServiceAccount,
            service_account_id: Some(account.id),
            expires_at: expires_at(now, req.expires_in_days),
        },
        now,
    );

    let saved = state.iam.api_tokens.create(row).await?;

    Ok(secret_response(IssuedTokenResp {
        meta: saved.into(),
        token,
    }))
}

async fn revoke(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    require_organization_scope(&ctx)?;
    Permission::require_key(&ctx, "api_tokens.revoke")?;
    let id = Id(id);
    state.iam.api_tokens.get(&ctx.org_id, &id).await?;
    state.iam.api_tokens.mark_revoked(&ctx.org_id, &id).await?;
    Ok(Json(serde_json::json!({ "revoked": true })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RumClientParams {
    application_id: String,
}

/// The application-bound, write-only token a RUM SDK embeds. It belongs to the application, not
/// to whoever fetched it, and may write RUM data for that application and nothing else.
async fn rum_client(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<RumClientParams>,
) -> Result<Response> {
    require_organization_scope(&ctx)?;
    Permission::require_key(&ctx, "api_tokens.issue")?;
    let application_id = validate_application_id(&params.application_id)?;

    // Events for an application that does not exist are refused, so a token is only issued for
    // one that does.
    state
        .telemetry
        .rum_applications
        .get(&ctx.org_id, application_id)
        .await?;

    let scope = ApiTokenScope::Permissions {
        permissions: vec![RUM_CLIENT_PERMISSION.to_string()],
    };

    require_held(&ctx, &[RUM_CLIENT_PERMISSION.to_string()])?;

    let token = state
        .iam
        .api_tokens
        .ensure_rum_client(&ctx.org_id, &ctx.user_id, application_id)
        .await?;

    Ok(secret_response(ManagedTokenResp::new(token, &scope)))
}
