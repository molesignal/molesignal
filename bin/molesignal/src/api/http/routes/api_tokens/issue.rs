// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What goes into a new token: a scope its issuer may hand out, a secret shown once, and an
//! expiry.

use serde::Deserialize;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::{
        api_token::{ApiToken, ApiTokenKind, ApiTokenScope},
        catalog::IamPermissionScope,
    },
    infra::persistence::repositories::api_tokens::{
        assemble_token, generate_token_parts, hash_secret,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// The longest a token is issued for.
const MAX_LIFETIME_DAYS: i64 = 365 * 5;

/// The scope a caller asks for: a permission policy to follow, a list of permissions, or neither
/// for whatever the token's principal may do.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequestedScope {
    #[serde(default)]
    pub policy_id: Option<String>,
    #[serde(default)]
    pub permissions: Option<Vec<String>>,
}

/// The scope to store for a new token, if its issuer may hand it out: a token never starts with a
/// permission its issuer lacks. On every request it is narrowed again, to what its principal
/// holds then.
pub(crate) async fn issue_scope(
    state: &AppState,
    issuer: &IamContext,
    requested: RequestedScope,
) -> Result<ApiTokenScope> {
    match (requested.policy_id, requested.permissions) {
        (Some(_), Some(_)) => Err(Error::invalid(
            "a token follows a permission policy or a list of permissions, not both",
        )),
        (None, None) => Ok(ApiTokenScope::Principal),
        (Some(policy_id), None) => {
            let policy_id = Id(policy_id.trim().to_string());

            let policy = match state.iam.policies.get(&issuer.org_id, &policy_id).await {
                Ok(entry) => entry.policy,
                Err(Error::NotFound(_)) => {
                    return Err(Error::invalid(format!(
                        "unknown permission policy: {policy_id}"
                    )));
                }
                Err(error) => return Err(error),
            };

            require_held(issuer, &policy.permissions)?;
            Ok(ApiTokenScope::Policy {
                policy_id: policy.id,
            })
        }
        (None, Some(permissions)) => {
            let permissions = state
                .iam
                .access
                .repository()
                .validate_permission_keys(&permissions, IamPermissionScope::Organization)
                .await?;

            if permissions.is_empty() {
                return Err(Error::invalid(
                    "a token's list of permissions cannot be empty",
                ));
            }

            require_held(issuer, &permissions)?;
            Ok(ApiTokenScope::Permissions { permissions })
        }
    }
}

/// Refuses to hand out a permission the issuer does not hold.
pub(crate) fn require_held(issuer: &IamContext, permissions: &[String]) -> Result<()> {
    match permissions
        .iter()
        .find(|permission| !issuer.has_permission(permission))
    {
        Some(missing) => Err(Error::forbidden(format!(
            "a token cannot be given {missing}, which its issuer does not hold"
        ))),
        None => Ok(()),
    }
}

/// When a token issued now for `days` expires: never without a lifetime, at most five years on.
pub(crate) fn expires_at(now: TimestampMicros, days: Option<i64>) -> Option<TimestampMicros> {
    days.map(|days| {
        let micros = days
            .clamp(1, MAX_LIFETIME_DAYS)
            .saturating_mul(86_400 * 1_000_000);

        TimestampMicros(now.0.saturating_add(micros))
    })
}

pub(crate) fn validate_name(name: &str) -> Result<String> {
    let name = name.trim();

    if name.is_empty() || name.chars().count() > 255 {
        return Err(Error::invalid(
            "name must contain between 1 and 255 characters",
        ));
    }

    Ok(name.to_string())
}

/// What a new API token is, all but its secret.
pub(crate) struct TokenSpec {
    pub id: Id,
    pub org_id: Id,
    /// Who issues it. A personal token acts as this user.
    pub user_id: Id,
    pub name: String,
    pub scope: ApiTokenScope,
    pub token_kind: ApiTokenKind,
    pub service_account_id: Option<Id>,
    pub expires_at: Option<TimestampMicros>,
}

/// The row to store for a new API token, and the token itself, which is shown once.
pub(crate) fn new_token(spec: TokenSpec, now: TimestampMicros) -> (ApiToken, String) {
    let (prefix, secret) = generate_token_parts();
    let plaintext = assemble_token(&prefix, &secret);

    let row = ApiToken {
        id: spec.id,
        prefix,
        secret_hash: hash_secret(&secret),
        org_id: spec.org_id,
        user_id: spec.user_id,
        scope: spec.scope,
        name: spec.name,
        expires_at: spec.expires_at,
        last_used_at: None,
        revoked: false,
        created_at: now,
        is_default: false,
        token_kind: spec.token_kind,
        application_id: None,
        service_account_id: spec.service_account_id,
    };

    (row, plaintext)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::domain::iam::IamScope;

    fn issuer(permissions: &[&str]) -> IamContext {
        IamContext {
            user_id: Id::from_string("issuer"),
            org_id: Id::from_string("org"),
            display_role: String::new(),
            roles: Vec::new(),
            credential_token_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope: IamScope::Organization,
            permissions: permissions
                .iter()
                .map(|permission| permission.to_string())
                .collect::<BTreeSet<_>>(),
            policy_version: 1,
        }
    }

    #[test]
    fn a_token_cannot_start_with_a_permission_its_issuer_lacks() {
        let issuer = issuer(&["streams.query", "streams.read"]);

        assert!(require_held(&issuer, &["streams.query".into()]).is_ok());
        assert!(matches!(
            require_held(&issuer, &["streams.query".into(), "logs.write".into()]),
            Err(Error::Forbidden(message)) if message.contains("logs.write")
        ));
    }

    #[test]
    fn a_lifetime_is_at_least_a_day_and_at_most_five_years() {
        let now = TimestampMicros(0);
        let day = 86_400 * 1_000_000;

        assert_eq!(expires_at(now, None), None);
        assert_eq!(expires_at(now, Some(0)), Some(TimestampMicros(day)));
        assert_eq!(
            expires_at(now, Some(100_000)),
            Some(TimestampMicros(day * MAX_LIFETIME_DAYS))
        );
    }

    #[test]
    fn a_name_is_trimmed_and_bounded() {
        assert_eq!(validate_name("  ci  ").expect("valid name"), "ci");
        assert!(validate_name("   ").is_err());
        assert!(validate_name(&"x".repeat(256)).is_err());
    }
}
