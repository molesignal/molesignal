// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! API token authentication, including application-bound public RUM tokens.

use std::{collections::BTreeSet, sync::Arc};

use crate::{
    app::iam::{IamContext, IamService},
    domain::iam::{
        IamScope,
        api_token::{ApiTokenKind, ApiTokenRepository},
    },
    infra::persistence::repositories::api_tokens::{
        split_rum_token, split_token, verify_rum_secret, verify_secret,
    },
    shared::{Error, ids::Id, time::TimestampMicros},
};

/// Validate a bearer credential and enforce the current organization state.
pub async fn authenticate_bearer(
    token: &str,
    iam: &IamService,
    repo: Arc<dyn ApiTokenRepository>,
) -> Result<IamContext, Error> {
    let context = authenticate_bearer_identity(token, iam, repo).await?;
    iam.ensure_organization_access(&context.org_id).await?;
    Ok(context)
}

pub(super) async fn authenticate_bearer_identity(
    token: &str,
    iam: &IamService,
    repo: Arc<dyn ApiTokenRepository>,
) -> Result<IamContext, Error> {
    if token.starts_with("ms_") || token.starts_with("msrum_") {
        authenticate_api_token_identity(token, iam, repo).await
    } else {
        let context = iam.verify_token(token)?;
        iam.ensure_user_access(&context.user_id).await?;
        Ok(context)
    }
}

/// API-token-only variant used by endpoints that intentionally reject JWTs.
pub async fn authenticate_api_token(
    token: &str,
    iam: &IamService,
    repo: Arc<dyn ApiTokenRepository>,
) -> Result<IamContext, Error> {
    let context = authenticate_api_token_identity(token, iam, repo).await?;
    iam.ensure_organization_access(&context.org_id).await?;
    Ok(context)
}

async fn authenticate_api_token_identity(
    token: &str,
    iam: &IamService,
    repo: Arc<dyn ApiTokenRepository>,
) -> Result<IamContext, Error> {
    let context = verify_api_token(token, repo).await?;
    // A public RUM credential is owned by the application, not by the user who
    // originally created it. Revocation and organization state remain enforced.
    if context.credential_application_id.is_none() {
        iam.ensure_user_access(&context.user_id).await?;
    }
    Ok(context)
}

/// Verify either `ms_<16>_<32>` or application-bound `msrum_<16>_<32>`.
pub async fn verify_api_token(
    token: &str,
    repo: Arc<dyn ApiTokenRepository>,
) -> Result<IamContext, Error> {
    let ((prefix, secret), presented_kind) = if token.starts_with("msrum_") {
        (
            split_rum_token(token).ok_or_else(|| {
                Error::unauthorized("malformed msrum_ token (expected msrum_<16>_<32>)")
            })?,
            ApiTokenKind::RumClient,
        )
    } else {
        (
            split_token(token).ok_or_else(|| {
                Error::unauthorized("malformed ms_ token (expected ms_<16>_<32>)")
            })?,
            ApiTokenKind::Personal,
        )
    };
    let row = repo
        .find_by_prefix(prefix)
        .await
        .map_err(|error| Error::unauthorized(format!("api token lookup: {error}")))?
        .ok_or_else(|| Error::unauthorized("api token not found"))?;
    let kind_matches = match presented_kind {
        ApiTokenKind::RumClient => row.token_kind == ApiTokenKind::RumClient,
        _ => row.token_kind != ApiTokenKind::RumClient,
    };
    if !kind_matches {
        return Err(Error::unauthorized(
            "api token type does not match its prefix",
        ));
    }
    if row.revoked {
        return Err(Error::unauthorized("api token revoked"));
    }
    let now = TimestampMicros::now();
    if let Some(expires_at) = row.expires_at
        && expires_at.0 <= now.0
    {
        return Err(Error::unauthorized("api token expired"));
    }
    let secret_valid = match row.token_kind {
        ApiTokenKind::RumClient => verify_rum_secret(secret, &row.secret_hash),
        _ => verify_secret(secret, &row.secret_hash),
    };
    if !secret_valid {
        return Err(Error::unauthorized("api token secret invalid"));
    }

    let prefix_owned = prefix.to_string();
    let repo_clone = repo.clone();
    crate::shared::trace_context::spawn_with_current_trace_context(async move {
        let _ = repo_clone.touch_last_used(&prefix_owned, now).await;
    });
    Ok(IamContext {
        user_id: Id(row.user_id.0),
        org_id: Id(row.org_id.0),
        display_role: String::new(),
        roles: Vec::new(),
        credential_role_id: Some(row.role_id),
        credential_application_id: row.application_id,
        scope: IamScope::ApiToken,
        permissions: BTreeSet::new(),
        features: BTreeSet::new(),
        policy_version: 0,
    })
}
