// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! API tokens over HTTP.
//!
//! A token carries no role: its scope narrows what its principal may do. A person's tokens act as
//! that person, a service account's tokens as the account, and an application's RUM client token
//! may only write RUM data for that application.
//!
//! - `/me/api-tokens`: the signed-in user's own tokens, the default intake token included.
//! - `/auth/tokens`: every token of the organization, tokens for service accounts, and the RUM
//!   client token of an application.

use axum::{
    Json, Router,
    http::header::{CACHE_CONTROL, PRAGMA},
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::api_token::{ApiToken, ApiTokenScope, ManagedApiToken},
    shared::{Error, Result},
};

mod issue;
mod organization;
mod personal;

pub(crate) use issue::{
    RequestedScope, TokenSpec, expires_at, issue_scope, new_token, validate_name,
};

pub fn routes() -> Router<AppState> {
    organization::routes().merge(personal::routes())
}

/// What a token may do: a permission policy it follows, a list fixed when it was issued, or,
/// with neither, whatever its principal may do.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScopeResp {
    pub policy_id: Option<String>,
    pub permissions: Option<Vec<String>>,
}

impl From<&ApiTokenScope> for ScopeResp {
    fn from(scope: &ApiTokenScope) -> Self {
        Self {
            policy_id: scope.policy_id().map(|id| id.0.clone()),
            permissions: scope.permissions().map(<[String]>::to_vec),
        }
    }
}

/// A token as listed, never with its secret.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TokenResp {
    pub id: String,
    pub prefix: String,
    pub name: String,
    pub token_kind: String,
    /// Who issued it. A personal token acts as this user.
    pub user_id: String,
    #[serde(flatten)]
    pub scope: ScopeResp,
    pub application_id: Option<String>,
    pub service_account_id: Option<String>,
    pub expires_at: Option<i64>,
    pub last_used_at: Option<i64>,
    pub revoked: bool,
    pub created_at: i64,
}

impl From<ApiToken> for TokenResp {
    fn from(token: ApiToken) -> Self {
        Self {
            scope: ScopeResp::from(&token.scope),
            id: token.id.0,
            prefix: token.prefix,
            name: token.name,
            token_kind: token.token_kind.as_str().to_string(),
            user_id: token.user_id.0,
            application_id: token.application_id,
            service_account_id: token.service_account_id.map(|id| id.0),
            expires_at: token.expires_at.map(|value| value.0),
            last_used_at: token.last_used_at.map(|value| value.0),
            revoked: token.revoked,
            created_at: token.created_at.0,
        }
    }
}

/// A token just issued, with its secret: the one time the secret is shown.
#[derive(Debug, Serialize)]
pub(crate) struct IssuedTokenResp {
    #[serde(flatten)]
    pub meta: TokenResp,
    /// The complete token.
    pub token: String,
}

/// A token whose secret the server keeps sealed and shows again on request: the default intake
/// token and an application's RUM client token.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagedTokenResp {
    id: String,
    prefix: String,
    token: String,
    token_kind: String,
    #[serde(flatten)]
    scope: ScopeResp,
    application_id: Option<String>,
    created_at: i64,
}

impl ManagedTokenResp {
    fn new(token: ManagedApiToken, scope: &ApiTokenScope) -> Self {
        Self {
            id: token.id.0,
            prefix: token.prefix,
            token: token.token,
            token_kind: token.token_kind.as_str().to_string(),
            scope: ScopeResp::from(scope),
            application_id: token.application_id,
            created_at: token.created_at.0,
        }
    }
}

/// A response carrying a secret, which no cache may keep.
pub(crate) fn secret_response(payload: impl Serialize) -> Response {
    (
        [(CACHE_CONTROL, "private, no-store"), (PRAGMA, "no-cache")],
        Json(payload),
    )
        .into_response()
}

fn require_organization_scope(context: &IamContext) -> Result<()> {
    if context.is_system_scope() {
        Err(Error::not_found("API tokens"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::iam::{IamScope, api_token::ApiTokenKind},
        shared::{ids::Id, time::TimestampMicros},
    };

    fn token(scope: ApiTokenScope) -> ApiToken {
        ApiToken {
            id: Id::from_string("token-id"),
            prefix: "0123456789abcdef".into(),
            secret_hash: "hash".into(),
            org_id: Id::from_string("org"),
            user_id: Id::from_string("issuer"),
            scope,
            name: "ci".into(),
            expires_at: None,
            last_used_at: None,
            revoked: false,
            created_at: TimestampMicros(1),
            is_default: false,
            token_kind: ApiTokenKind::Personal,
            application_id: None,
            service_account_id: None,
        }
    }

    #[test]
    fn system_scope_cannot_manage_api_tokens_despite_its_display_role() {
        let context = IamContext {
            user_id: Id::from_string("root"),
            org_id: Id::from_string("_sys"),
            display_role: "Platform Steward".into(),
            roles: vec![crate::domain::iam::IamAssignedRole {
                id: Id::from_string("database-role"),
                key: "platform_steward".into(),
                name: "Platform Steward".into(),
                builtin: false,
            }],
            credential_token_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope: IamScope::System,
            permissions: ["sys.telemetry.read".to_string()].into_iter().collect(),
            policy_version: 1,
        };

        assert!(matches!(
            require_organization_scope(&context),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn a_listed_token_says_what_it_may_do_and_never_its_secret() {
        let principal = serde_json::to_value(TokenResp::from(token(ApiTokenScope::Principal)))
            .expect("token serializes");

        assert_eq!(principal["policyId"], serde_json::Value::Null);
        assert_eq!(principal["permissions"], serde_json::Value::Null);
        assert!(principal.get("secretHash").is_none());

        let policy = serde_json::to_value(TokenResp::from(token(ApiTokenScope::Policy {
            policy_id: Id::from_string("builtin:data_intake"),
        })))
        .expect("token serializes");

        assert_eq!(policy["policyId"], "builtin:data_intake");

        let listed = serde_json::to_value(TokenResp::from(token(ApiTokenScope::Permissions {
            permissions: vec!["streams.query".into()],
        })))
        .expect("token serializes");

        assert_eq!(listed["permissions"], serde_json::json!(["streams.query"]));
    }

    #[test]
    fn plaintext_token_responses_are_not_cacheable() {
        let response = secret_response(IssuedTokenResp {
            meta: TokenResp::from(token(ApiTokenScope::Principal)),
            token: "mspat-0123456789abcdefghjkmnpqrstvwxyz0123".into(),
        });

        assert_eq!(response.headers()[CACHE_CONTROL], "private, no-store");
        assert_eq!(response.headers()[PRAGMA], "no-cache");
    }
}
