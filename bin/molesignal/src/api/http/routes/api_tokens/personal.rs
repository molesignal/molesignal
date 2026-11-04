// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The signed-in user's own tokens.
//!
//! Anyone who may manage their own tokens issues them here; a token can never do more than its
//! owner may do at the time. Only a signed-in session issues or revokes a personal token: a token
//! never mints another, so a leaked token cannot outlive its revocation through a copy.

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    response::Response,
    routing::{delete, get},
};
use serde::{Deserialize, Serialize};

use super::{
    IssuedTokenResp, ManagedTokenResp, RequestedScope, TokenResp, TokenSpec, expires_at,
    issue_scope, new_token, secret_response, validate_name,
};
use crate::{
    api::{AppState, http::middleware::Permission},
    app::iam::IamContext,
    domain::iam::{
        IamScope,
        api_token::ApiTokenKind,
        catalog::{IamPermissionDefinition, IamPermissionScope},
        policy::DATA_INTAKE_POLICY,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// Who may issue, list and revoke tokens of their own.
const PERSONAL_TOKENS_PERMISSION: &str = "personal_tokens.manage";

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/me/api-tokens", get(list).post(create))
        .route("/me/api-tokens/default", get(default_intake))
        .route("/me/api-tokens/scopes", get(scopes))
        .route("/me/api-tokens/{id}", delete(revoke))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateReq {
    name: String,
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
    require_signed_in(&ctx)?;

    let tokens = state
        .iam
        .api_tokens
        .list_personal(&ctx.org_id, &ctx.user_id)
        .await?;

    Ok(Json(tokens.into_iter().map(TokenResp::from).collect()))
}

async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Response> {
    require_signed_in(&ctx)?;
    let name = validate_name(&req.name)?;
    let scope = issue_scope(&state, &ctx, req.scope).await?;
    let now = TimestampMicros::now();

    let (row, token) = new_token(
        TokenSpec {
            id: Id::new(),
            org_id: ctx.org_id.clone(),
            user_id: ctx.user_id.clone(),
            name,
            scope,
            token_kind: ApiTokenKind::Personal,
            service_account_id: None,
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

/// The user's default intake token, created on first use and shown again on request, so a data
/// source page can offer a ready token. It writes data and nothing else, and only for someone who
/// may write data.
async fn default_intake(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Response> {
    require_signed_in(&ctx)?;

    let scope = issue_scope(
        &state,
        &ctx,
        RequestedScope {
            policy_id: Some(DATA_INTAKE_POLICY.to_string()),
            permissions: None,
        },
    )
    .await?;

    let token = state
        .iam
        .api_tokens
        .ensure_default(&ctx.org_id, &ctx.user_id)
        .await?;

    Ok(secret_response(ManagedTokenResp::new(token, &scope)))
}

/// What a token of the organization can be limited to: its permission policies and the
/// organization permissions. A page names a token's scope with them, and offers an issuer the
/// ones it holds.
#[derive(Debug, Serialize)]
struct ScopesResp {
    policies: Vec<PolicyOption>,
    permissions: Vec<IamPermissionDefinition>,
}

#[derive(Debug, Serialize)]
struct PolicyOption {
    id: String,
    key: String,
    name: String,
    description: String,
    builtin: bool,
    permissions: Vec<String>,
}

async fn scopes(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<ScopesResp>> {
    require_member(&ctx)?;

    let policies = state
        .iam
        .policies
        .list(&ctx.org_id)
        .await?
        .into_iter()
        .map(|entry| PolicyOption {
            builtin: entry.policy.builtin(),
            id: entry.policy.id.0,
            key: entry.policy.key,
            name: entry.policy.name,
            description: entry.policy.description,
            permissions: entry.policy.permissions,
        })
        .collect();

    let permissions = state
        .iam
        .access
        .repository()
        .permission_catalog()
        .await?
        .permissions
        .into_iter()
        .filter(|permission| permission.scope == IamPermissionScope::Organization)
        .collect();

    Ok(Json(ScopesResp {
        policies,
        permissions,
    }))
}

async fn revoke(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    require_signed_in(&ctx)?;
    let id = Id(id);
    let token = state.iam.api_tokens.get(&ctx.org_id, &id).await?;

    let own = token.user_id == ctx.user_id
        && matches!(
            token.token_kind,
            ApiTokenKind::Personal | ApiTokenKind::DefaultIntake
        );

    if !own {
        return Err(Error::not_found("API token"));
    }

    state.iam.api_tokens.mark_revoked(&ctx.org_id, &id).await?;
    Ok(Json(serde_json::json!({ "revoked": true })))
}

/// A signed-in member of an organization.
fn require_member(ctx: &IamContext) -> Result<()> {
    match ctx.scope {
        IamScope::System => Err(Error::not_found("API tokens")),
        IamScope::ApiToken => Err(Error::forbidden(
            "personal API tokens are managed from a signed-in session",
        )),
        IamScope::Organization => Ok(()),
    }
}

/// A signed-in member who may manage their own tokens.
fn require_signed_in(ctx: &IamContext) -> Result<()> {
    require_member(ctx)?;
    Permission::require_key(ctx, PERSONAL_TOKENS_PERMISSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(scope: IamScope, permissions: &[&str]) -> IamContext {
        IamContext {
            user_id: Id::from_string("member"),
            org_id: Id::from_string("org"),
            display_role: String::new(),
            roles: Vec::new(),
            credential_token_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope,
            permissions: permissions
                .iter()
                .map(|permission| permission.to_string())
                .collect(),
            policy_version: 1,
        }
    }

    #[test]
    fn a_signed_in_member_with_the_permission_manages_their_tokens() {
        let member = context(IamScope::Organization, &[PERSONAL_TOKENS_PERMISSION]);

        assert!(require_signed_in(&member).is_ok());
    }

    #[test]
    fn a_member_without_the_permission_cannot() {
        let member = context(IamScope::Organization, &["streams.query"]);

        assert!(matches!(
            require_signed_in(&member),
            Err(Error::Forbidden(_))
        ));
    }

    #[test]
    fn a_token_never_mints_or_lists_personal_tokens() {
        let mut token = context(IamScope::ApiToken, &[PERSONAL_TOKENS_PERMISSION]);
        token.credential_token_id = Some(Id::from_string("token"));

        assert!(matches!(
            require_signed_in(&token),
            Err(Error::Forbidden(_))
        ));
    }

    #[test]
    fn the_platform_organization_has_no_personal_tokens() {
        let platform = context(IamScope::System, &[PERSONAL_TOKENS_PERMISSION]);

        assert!(matches!(
            require_signed_in(&platform),
            Err(Error::NotFound(_))
        ));
    }
}
