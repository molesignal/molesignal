// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Platform permissions held through a role in the platform organization.
//!
//! Nothing in a tenant gives a platform permission: no role of an organization holds one, its owner
//! included. They reach an account only through a binding in the platform organization, which has
//! no members, so the binding itself is the grant. Root needs none.

use std::collections::BTreeSet;

use sqlx::{PgPool, Row};

use super::super::sqlx_err;
use crate::{
    domain::iam::{IamAssignedRole, access::IamPlatformGrant},
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// What a user was granted: every binding in force, across the platform or narrowed to an
/// organization. A binding with conditions gives nothing: nothing about a platform session could
/// satisfy one, and the API does not accept them there.
pub(super) async fn platform_grant(
    pool: &PgPool,
    user_id: &Id,
    now: TimestampMicros,
) -> Result<IamPlatformGrant> {
    let rows = sqlx::query(
        "SELECT role.id AS role_id,
                role.role_key,
                role.name AS role_name,
                role.builtin AS role_builtin,
                binding.resource_id,
                permission.permission_key
           FROM iam_role_bindings binding
           JOIN organizations organization
             ON organization.id = binding.organization_id
            AND organization.system
           JOIN iam_roles role
             ON role.id = binding.role_id
            AND role.org_id = binding.organization_id
           CROSS JOIN LATERAL unnest(iam_role_grants(role.permissions, role.policy_ids))
                AS role_permission(permission_key)
           JOIN iam_permissions permission
             ON permission.permission_key = role_permission.permission_key
            AND permission.scope = 'platform'
          WHERE binding.principal_type = 'user'
            AND binding.principal_id = $1
            AND (binding.resource_type IS NULL OR binding.resource_type = 'organization')
            AND binding.conditions IN ('{}'::JSONB, 'null'::JSONB)
            AND (binding.starts_at IS NULL OR binding.starts_at <= $2)
            AND (binding.expires_at IS NULL OR binding.expires_at > $2)
       ORDER BY role.name, role.id, permission.permission_key",
    )
    .bind(&user_id.0)
    .bind(now)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    let mut grant = IamPlatformGrant::default();
    let mut seen_roles = BTreeSet::new();

    for row in rows {
        let counted = grant.add(
            row.try_get("permission_key").map_err(sqlx_err)?,
            row.try_get("resource_id").map_err(sqlx_err)?,
        );

        let role_id: String = row.try_get("role_id").map_err(sqlx_err)?;

        if counted && seen_roles.insert(role_id.clone()) {
            grant.roles.push(IamAssignedRole {
                id: Id::from_string(role_id),
                key: row.try_get("role_key").map_err(sqlx_err)?,
                name: row.try_get("role_name").map_err(sqlx_err)?,
                builtin: row.try_get("role_builtin").map_err(sqlx_err)?,
            });
        }
    }

    Ok(grant)
}
