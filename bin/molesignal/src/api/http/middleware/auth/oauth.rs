// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};

use super::AuthenticatedCredential;
use crate::{
    agent::inbound_mcp::InboundMcpOAuthTokenKind,
    api::AppState,
    app::iam::IamContext,
    domain::iam::IamScope,
    shared::{Error, Result, time::TimestampMicros},
};

pub(crate) fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Who an OAuth access token acts for, or `None` when the store holds no OAuth token of this value:
/// an API token looks just the same.
pub(crate) async fn authenticate_access_token(
    state: &AppState,
    token: &str,
    expected_resource: &str,
) -> Result<Option<(IamContext, AuthenticatedCredential)>> {
    let Some(record) = state
        .agent
        .inbound_mcp
        .find_oauth_token(&token_hash(token))
        .await?
    else {
        return Ok(None);
    };

    if record.token_kind != InboundMcpOAuthTokenKind::Access
        || record.revoked_at.is_some()
        || record.expires_at.0 <= TimestampMicros::now().0
    {
        return Err(Error::unauthorized(
            "OAuth access token is expired or revoked",
        ));
    }

    if record.resource != expected_resource {
        return Err(Error::unauthorized(
            "OAuth access token audience does not match this MCP resource",
        ));
    }

    if !record
        .scope
        .split_ascii_whitespace()
        .any(|scope| scope == "mcp")
    {
        return Err(Error::forbidden(
            "OAuth access token does not include the mcp scope",
        ));
    }

    state
        .iam
        .service
        .ensure_user_access(&record.user_id)
        .await?;

    let context = IamContext {
        user_id: record.user_id,
        org_id: record.org_id,
        display_role: String::new(),
        roles: Vec::new(),
        credential_token_id: None,
        credential_application_id: None,
        credential_service_account_id: None,
        scope: IamScope::Organization,
        permissions: BTreeSet::new(),
        policy_version: 0,
    };

    Ok(Some((
        context,
        AuthenticatedCredential::OAuthAccess {
            token_id: record.id,
        },
    )))
}
