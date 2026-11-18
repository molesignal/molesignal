// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! API token authentication, including application-bound public RUM tokens.

use std::{collections::BTreeSet, sync::Arc};

use domain::iam::{
    IamContext, IamScope,
    api_token::{ApiTokenKind, ApiTokenRepository, has_token_marker},
};
use kernel::{Error, ids::Id, time::TimestampMicros};
use postgres::repositories::api_tokens::{split_token, verify_secret};

use crate::app::iam::IamService;

#[derive(Debug, Clone)]
pub(crate) struct VerifiedApiTokenIdentity {
    pub context: IamContext,
    pub token_id: Id,
    pub token_kind: ApiTokenKind,
}

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
    if has_token_marker(token) {
        authenticate_api_token_identity(token, iam, repo).await
    } else {
        let context = iam
            .verify_with_current_secrets(token, |iam| iam.verify_token(token))
            .await?;

        iam.ensure_user_access(&context.user_id).await?;
        Ok(context)
    }
}

pub(crate) async fn authenticate_api_token_identity_with_metadata(
    token: &str,
    iam: &IamService,
    repo: Arc<dyn ApiTokenRepository>,
) -> Result<VerifiedApiTokenIdentity, Error> {
    let identity = verify_api_token_with_metadata(token, repo).await?;

    if identity.context.credential_application_id.is_none()
        && identity.context.credential_service_account_id.is_none()
    {
        iam.ensure_user_access(&identity.context.user_id).await?;
    }

    Ok(identity)
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
    let context = verify_api_token_with_metadata(token, repo).await?.context;

    // A public RUM credential is owned by the application, not by the user who
    // originally created it. Revocation and organization state remain enforced.
    if context.credential_application_id.is_none()
        && context.credential_service_account_id.is_none()
    {
        iam.ensure_user_access(&context.user_id).await?;
    }

    Ok(context)
}

/// Verify an API token, whatever its kind.
pub async fn verify_api_token(
    token: &str,
    repo: Arc<dyn ApiTokenRepository>,
) -> Result<IamContext, Error> {
    Ok(verify_api_token_with_metadata(token, repo).await?.context)
}

pub(crate) async fn verify_api_token_with_metadata(
    token: &str,
    repo: Arc<dyn ApiTokenRepository>,
) -> Result<VerifiedApiTokenIdentity, Error> {
    let (prefix, secret) = split_token(token)
        .ok_or_else(|| Error::unauthorized("malformed API token (expected mspat-<36>)"))?;

    // 查不了（数据库忙、连不上）说明不了 token 无效：错误原样交回，按服务端错误应答，客户端
    // 会重试，而不是当成凭据失效。
    let row = repo
        .find_by_prefix(prefix)
        .await?
        .ok_or_else(|| Error::unauthorized("api token not found"))?;

    if row.revoked {
        return Err(Error::unauthorized("api token revoked"));
    }

    let now = TimestampMicros::now();

    if let Some(expires_at) = row.expires_at
        && expires_at.0 <= now.0
    {
        return Err(Error::unauthorized("api token expired"));
    }

    if !verify_secret(secret, &row.secret_hash) {
        return Err(Error::unauthorized("api token secret invalid"));
    }

    let prefix_owned = prefix.to_string();
    let repo_clone = repo.clone();

    signals::trace_context::spawn_with_current_trace_context(async move {
        let _ = repo_clone.touch_last_used(&prefix_owned, now).await;
    });

    let service_account_id = row.service_account_id.clone();
    let token_id = row.id.clone();
    let token_kind = row.token_kind;

    Ok(VerifiedApiTokenIdentity {
        context: IamContext {
            user_id: service_account_id.clone().unwrap_or(Id(row.user_id.0)),
            org_id: Id(row.org_id.0),
            display_role: String::new(),
            roles: Vec::new(),
            credential_token_id: Some(token_id.clone()),
            credential_application_id: row.application_id,
            credential_service_account_id: service_account_id,
            scope: IamScope::ApiToken,
            permissions: BTreeSet::new(),
            policy_version: 0,
        },
        token_id,
        token_kind,
    })
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use domain::iam::api_token::{ApiToken, ManagedApiToken};
    use kernel::Result;
    use postgres::repositories::api_tokens::{assemble_token, generate_token_parts};

    use super::*;

    /// 数据库忙时的 token 仓库：查找答不了。
    struct BusyTokens;

    #[async_trait]
    impl ApiTokenRepository for BusyTokens {
        async fn create(&self, _token: ApiToken) -> Result<ApiToken> {
            Err(Error::internal("unused"))
        }

        async fn find_by_prefix(&self, _prefix: &str) -> Result<Option<ApiToken>> {
            Err(Error::unavailable("metadata database busy"))
        }

        async fn list_by_org(&self, _org_id: &Id) -> Result<Vec<ApiToken>> {
            Err(Error::internal("unused"))
        }

        async fn list_personal(&self, _org_id: &Id, _user_id: &Id) -> Result<Vec<ApiToken>> {
            Err(Error::internal("unused"))
        }

        async fn get(&self, _org_id: &Id, _id: &Id) -> Result<ApiToken> {
            Err(Error::internal("unused"))
        }

        async fn mark_revoked(&self, _org_id: &Id, _id: &Id) -> Result<()> {
            Err(Error::internal("unused"))
        }

        async fn touch_last_used(&self, _prefix: &str, _at: TimestampMicros) -> Result<()> {
            Err(Error::internal("unused"))
        }

        async fn ensure_default(&self, _org_id: &Id, _user_id: &Id) -> Result<ManagedApiToken> {
            Err(Error::internal("unused"))
        }

        async fn ensure_rum_client(
            &self,
            _org_id: &Id,
            _user_id: &Id,
            _application_id: &str,
        ) -> Result<ManagedApiToken> {
            Err(Error::internal("unused"))
        }
    }

    #[tokio::test]
    async fn a_lookup_that_cannot_be_answered_does_not_call_the_token_invalid() {
        let (prefix, secret) = generate_token_parts();
        let token = assemble_token(&prefix, &secret);
        let error = verify_api_token(&token, Arc::new(BusyTokens))
            .await
            .unwrap_err();

        assert!(matches!(error, Error::Unavailable(_)), "{error}");
        assert_eq!(error.http_status_code(), 503);
    }
}
