// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What an API token may still do, read for every request it makes.
//!
//! A token carries no role. Its scope is a permission policy, which it follows as the policy
//! changes, a list fixed when it was issued, or neither, which leaves its principal's access
//! whole. The principal of a service-account token is the account: its role
//! is read here with the token, so a role change reaches the account's tokens at once.

use std::collections::BTreeSet;

use sqlx::{PgPool, Row};

use super::super::sqlx_err;
use crate::{
    domain::iam::access::IamTokenGrant,
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// The grant of a token that may still authenticate: not revoked, not expired, and, for a
/// service-account token, bound to an enabled account that still exists.
pub(super) async fn api_token_grant(
    pool: &PgPool,
    organization_id: &Id,
    token_id: &Id,
    now: TimestampMicros,
) -> Result<Option<IamTokenGrant>> {
    let row = sqlx::query(
        "SELECT token.policy_id,
                token.permissions,
                policy.permissions AS policy_permissions,
                token.service_account_id,
                account.role_id AS service_account_role_id
           FROM api_tokens token
      LEFT JOIN iam_policies policy
             ON policy.id = token.policy_id
      LEFT JOIN service_accounts account
             ON account.org_id = token.org_id
            AND account.id = token.service_account_id
            AND NOT account.disabled
            AND account.deleted_at IS NULL
          WHERE token.org_id = $1
            AND token.id = $2
            AND NOT token.revoked
            AND (token.expires_at IS NULL OR token.expires_at > $3)",
    )
    .bind(&organization_id.0)
    .bind(&token_id.0)
    .bind(now)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?;

    let Some(row) = row else {
        return Ok(None);
    };

    let service_account_id: Option<String> = row.try_get("service_account_id").map_err(sqlx_err)?;
    let service_account_role_id: Option<String> =
        row.try_get("service_account_role_id").map_err(sqlx_err)?;

    if service_account_id.is_some() && service_account_role_id.is_none() {
        return Ok(None);
    }

    let policy: Option<String> = row.try_get("policy_id").map_err(sqlx_err)?;
    let listed: Option<Vec<String>> = row.try_get("permissions").map_err(sqlx_err)?;
    let policy_permissions: Option<Vec<String>> =
        row.try_get("policy_permissions").map_err(sqlx_err)?;

    let ceiling = match (policy, listed) {
        (Some(_), _) => Some(policy_permissions.unwrap_or_default()),
        (None, Some(listed)) => Some(listed),
        (None, None) => None,
    };

    Ok(Some(IamTokenGrant {
        ceiling: ceiling.map(|permissions| permissions.into_iter().collect::<BTreeSet<_>>()),
        service_account_role_id: service_account_role_id.map(Id),
    }))
}
