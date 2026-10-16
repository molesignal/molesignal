// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Platform permissions held through an organization role.
//!
//! Platform permissions are never assignable to a custom role. They reach an organization only
//! through the built-in catalog, which gives them to the role an organization is bootstrapped
//! with: its owner. This module answers what that amounts to for one user, across every
//! organization they belong to.

use sqlx::PgPool;

use super::super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

/// The platform permissions a user holds as the owner of some organization.
///
/// Only a binding that would also count in the organization itself counts here: it is in force
/// now, it covers the whole organization and not one resource, and it has no conditions. The
/// organization has to be an enabled tenant the user is a member of, so suspending the
/// organization or removing the member ends the platform access with it.
pub(super) async fn platform_permissions(
    pool: &PgPool,
    user_id: &Id,
    now: TimestampMicros,
) -> Result<Vec<String>> {
    sqlx::query_scalar(
        "SELECT DISTINCT permission.permission_key
           FROM iam_role_bindings binding
           JOIN organizations organization
             ON organization.id = binding.organization_id
            AND NOT organization.system
            AND NOT organization.disabled
           JOIN iam_memberships membership
             ON membership.org_id = binding.organization_id
            AND membership.user_id = $1
           JOIN iam_roles role
             ON role.id = binding.role_id
            AND role.org_id = binding.organization_id
            AND role.builtin
           CROSS JOIN LATERAL unnest(role.permissions) AS role_permission(permission_key)
           JOIN iam_permissions permission
             ON permission.permission_key = role_permission.permission_key
            AND permission.scope = 'platform'
          WHERE binding.resource_type IS NULL
            AND binding.resource_id IS NULL
            AND binding.conditions IN ('{}'::JSONB, 'null'::JSONB)
            AND (binding.starts_at_micros IS NULL OR binding.starts_at_micros <= $2)
            AND (binding.expires_at_micros IS NULL OR binding.expires_at_micros > $2)
            AND (
                (binding.principal_type = 'user' AND binding.principal_id = $1)
                OR (
                    binding.principal_type = 'team'
                    AND EXISTS (
                        SELECT 1
                          FROM teams team
                         WHERE team.org_id = binding.organization_id
                           AND team.id = binding.principal_id
                           AND team.member_ids @> jsonb_build_array($1::TEXT)
                    )
                )
            )
       ORDER BY permission.permission_key",
    )
    .bind(&user_id.0)
    .bind(now.0)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)
}
