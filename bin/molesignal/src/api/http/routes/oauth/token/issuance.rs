// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeSet;

use super::{OAuthError, OAuthResult, TokenResponse, random_secret};
use crate::{
    agent::{
        FEATURE,
        inbound_mcp::{InboundMcpOAuthToken, InboundMcpOAuthTokenKind, InboundMcpSettings},
    },
    api::{AppState, http::billing::org_blocked_cached},
    app::iam::IamContext,
    domain::iam::IamScope,
    shared::{ids::Id, time::TimestampMicros},
};

const ACCESS_TOKEN_TTL_MICROS: i64 = 60 * 60 * 1_000_000;
const REFRESH_TOKEN_TTL_MICROS: i64 = 30 * 24 * 60 * 60 * 1_000_000;

pub(super) struct TokenRotation {
    pub(super) token_hash: String,
    pub(super) token_id: Id,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn issue_tokens(
    state: &AppState,
    client_id: String,
    org_id: Id,
    user_id: Id,
    scope: String,
    resource: String,
    family_id: Id,
    rotation: Option<TokenRotation>,
    now: TimestampMicros,
) -> OAuthResult<TokenResponse> {
    let access_plaintext = format!("msoauth_{}", random_secret(32));
    let access = InboundMcpOAuthToken {
        id: Id::new(),
        token_hash: crate::api::http::middleware::auth::oauth::token_hash(&access_plaintext),
        token_kind: InboundMcpOAuthTokenKind::Access,
        family_id: family_id.clone(),
        client_id: client_id.clone(),
        org_id: org_id.clone(),
        user_id: user_id.clone(),
        scope: scope.clone(),
        resource: resource.clone(),
        expires_at: TimestampMicros(now.0.saturating_add(ACCESS_TOKEN_TTL_MICROS)),
        revoked_at: None,
        rotated_from: None,
        created_at: now,
    };
    let refresh_plaintext = scope
        .split_ascii_whitespace()
        .any(|value| value == "offline_access")
        .then(|| format!("msrefresh_{}", random_secret(32)));
    let refresh = refresh_plaintext
        .as_ref()
        .map(|plaintext| InboundMcpOAuthToken {
            id: Id::new(),
            token_hash: crate::api::http::middleware::auth::oauth::token_hash(plaintext),
            token_kind: InboundMcpOAuthTokenKind::Refresh,
            family_id: family_id.clone(),
            client_id: client_id.clone(),
            org_id: org_id.clone(),
            user_id: user_id.clone(),
            scope: scope.clone(),
            resource: resource.clone(),
            expires_at: TimestampMicros(now.0.saturating_add(REFRESH_TOKEN_TTL_MICROS)),
            revoked_at: None,
            rotated_from: rotation.as_ref().map(|value| value.token_id.clone()),
            created_at: now,
        });
    let rotated = if let Some(rotation) = rotation {
        state
            .agent
            .inbound_mcp
            .rotate_oauth_token_pair(&rotation.token_hash, now, access, refresh)
            .await
            .map_err(OAuthError::server)?
    } else {
        state
            .agent
            .inbound_mcp
            .create_oauth_token_pair(access, refresh)
            .await
            .map_err(OAuthError::server)?;
        true
    };
    if !rotated {
        state
            .agent
            .inbound_mcp
            .revoke_oauth_family(&org_id, &family_id, now)
            .await
            .map_err(OAuthError::server)?;
        return Err(OAuthError::invalid_grant(
            "refresh token reuse was detected; the token family was revoked",
        ));
    }
    Ok(TokenResponse {
        access_token: access_plaintext,
        token_type: "Bearer",
        expires_in: ACCESS_TOKEN_TTL_MICROS / 1_000_000,
        scope,
        refresh_token: refresh_plaintext,
    })
}

pub(super) async fn ensure_grant_available(
    state: &AppState,
    org_id: &Id,
    user_id: &Id,
) -> OAuthResult<()> {
    if !state.platform.license.has_feature(FEATURE) {
        return Err(OAuthError::invalid_grant(
            "the organization is not licensed for Inbound MCP",
        ));
    }
    state
        .iam
        .service
        .ensure_user_access(user_id)
        .await
        .map_err(|error| OAuthError::invalid_grant(error.to_string()))?;
    state
        .iam
        .service
        .ensure_organization_access(org_id)
        .await
        .map_err(|error| OAuthError::invalid_grant(error.to_string()))?;
    if org_blocked_cached(state, org_id, TimestampMicros::now().0)
        .await
        .map_err(OAuthError::server)?
    {
        return Err(OAuthError::invalid_grant("organization access is paused"));
    }
    let mut context = IamContext {
        user_id: user_id.clone(),
        org_id: org_id.clone(),
        display_role: String::new(),
        roles: Vec::new(),
        credential_role_id: None,
        credential_application_id: None,
        credential_service_account_id: None,
        scope: IamScope::Organization,
        permissions: BTreeSet::new(),
        features: BTreeSet::new(),
        policy_version: 0,
    };
    state
        .iam
        .access
        .enrich_context(&mut context)
        .await
        .map_err(|error| OAuthError::invalid_grant(error.to_string()))?;
    if !context.has_permission("agent.use") {
        return Err(OAuthError::invalid_grant(
            "the user no longer has permission to use Mole Agent",
        ));
    }
    let settings = state
        .agent
        .inbound_mcp
        .get_settings(org_id)
        .await
        .map_err(OAuthError::server)?
        .unwrap_or_else(|| InboundMcpSettings::defaults(org_id.clone()));
    if !settings.enabled {
        return Err(OAuthError::invalid_grant(
            "Inbound MCP is disabled for the organization",
        ));
    }
    Ok(())
}
