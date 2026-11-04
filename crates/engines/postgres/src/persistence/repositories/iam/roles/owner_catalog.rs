// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The owner of an organization holds the organization's whole permission catalog, and nothing of
//! the platform's.
//!
//! The catalog seed lists, permission by permission, which built-in roles receive it. For the
//! owner that list is not the source of truth: the assignment is derived from the catalog itself,
//! so a permission added later cannot be left out of it by forgetting to name the role, and one
//! that was given to owners earlier does not stay behind.

use sqlx::PgPool;

use super::{super::super::sqlx_err, bump_version};
use crate::{
    domain::iam::ORGANIZATION_BOOTSTRAP_ROLE_PURPOSE,
    shared::{Result, ids::Id},
};

/// Set the owner role to exactly the organization permissions of the catalog, on its built-in row
/// and in every organization where the role already exists. Returns how many roles changed.
///
/// Idempotent, and safe to run from several nodes at once: each statement only touches what
/// differs. An organization whose role changed gets a new policy version, so capability snapshots
/// resolved before the change are not served again.
pub(super) async fn grant_catalog_to_organization_owners(pool: &PgPool) -> Result<u64> {
    let mut tx = sqlx::begin(pool).await.map_err(sqlx_err)?;

    // The keys are aggregated as TEXT: the role column is TEXT[], and since PostgreSQL 14 a
    // COALESCE of a VARCHAR[] and a TEXT[] is VARCHAR[], which `IS DISTINCT FROM` will not compare
    // with it.
    let catalog_changed: i64 = sqlx::query_scalar(
        "WITH catalog AS (
             SELECT COALESCE(
                        array_agg(permission_key::TEXT ORDER BY permission_key),
                        ARRAY[]::TEXT[]
                    ) AS keys
               FROM iam_permissions
              WHERE scope = 'organization'
         ), updated AS (
             UPDATE iam_roles role
                SET permissions = catalog.keys
               FROM catalog
              WHERE role.org_id IS NULL
                AND role.purpose = $1
                AND role.permissions IS DISTINCT FROM catalog.keys
          RETURNING role.id
         )
         SELECT COUNT(*)::BIGINT FROM updated",
    )
    .bind(ORGANIZATION_BOOTSTRAP_ROLE_PURPOSE)
    .fetch_one(&mut *tx)
    .await
    .map_err(sqlx_err)?;

    // Every organization's owner role follows the built-in row.
    let changed_organizations: Vec<String> = sqlx::query_scalar(
        "WITH owner AS (
             SELECT role_key, permissions
               FROM iam_roles
              WHERE org_id IS NULL
                AND purpose = $1
         ), updated AS (
             UPDATE iam_roles role
                SET permissions = owner.permissions
               FROM owner
              WHERE owner.role_key = role.role_key
                AND role.builtin
                AND role.org_id IS NOT NULL
                AND role.permissions IS DISTINCT FROM owner.permissions
          RETURNING role.org_id
         )
         SELECT DISTINCT org_id FROM updated",
    )
    .bind(ORGANIZATION_BOOTSTRAP_ROLE_PURPOSE)
    .fetch_all(&mut *tx)
    .await
    .map_err(sqlx_err)?;

    for organization_id in &changed_organizations {
        bump_version(&mut tx, &Id::from_string(organization_id.clone())).await?;
    }

    // The catalog tells every reader which built-in roles hold a permission. That changed.
    if catalog_changed > 0 {
        sqlx::query(
            "UPDATE iam_catalog_versions
                SET version = version + 1,
                    updated_at = clock_timestamp()
              WHERE catalog_key = 'permissions'",
        )
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
    }

    tx.commit().await.map_err(sqlx_err)?;

    Ok(u64::try_from(catalog_changed).unwrap_or_default() + changed_organizations.len() as u64)
}
