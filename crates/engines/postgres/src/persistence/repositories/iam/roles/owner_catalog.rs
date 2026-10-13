// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The owner of an organization holds the whole permission catalog.
//!
//! The catalog seed lists, permission by permission, which built-in roles receive it. For the
//! owner that list is not the source of truth: an owner holds every permission there is, platform
//! scope included, so the assignment is derived from the catalog itself. A permission added later
//! cannot be left out of it by forgetting to name the role.

use sqlx::PgPool;

use super::{super::super::sqlx_err, bump_version};
use crate::{
    domain::iam::ORGANIZATION_BOOTSTRAP_ROLE_PURPOSE,
    shared::{Result, ids::Id},
};

/// Give the owner role every permission in the catalog, in the built-in catalog and in every
/// organization where the role already exists. Returns how many assignments were added.
///
/// Idempotent, and safe to run from several nodes at once: each statement only adds what is
/// missing. An organization whose role changed gets a new policy version, so capability snapshots
/// resolved before the change are not served again.
pub(super) async fn grant_catalog_to_organization_owners(pool: &PgPool) -> Result<u64> {
    let mut tx = sqlx::begin(pool).await.map_err(sqlx_err)?;

    let catalog_added = sqlx::query(
        "INSERT INTO iam_builtin_role_permissions (role_key, permission_key)
         SELECT purpose.role_key, permission.permission_key
           FROM iam_builtin_role_purposes purpose
          CROSS JOIN iam_permissions permission
          WHERE purpose.purpose = $1
         ON CONFLICT (role_key, permission_key) DO NOTHING",
    )
    .bind(ORGANIZATION_BOOTSTRAP_ROLE_PURPOSE)
    .execute(&mut *tx)
    .await
    .map_err(sqlx_err)?
    .rows_affected();

    let changed_organizations: Vec<(String, i64)> = sqlx::query_as(
        "WITH inserted AS (
             INSERT INTO iam_role_permissions (role_id, permission_key)
             SELECT role.id, catalog.permission_key
               FROM iam_roles role
               JOIN iam_builtin_role_purposes purpose
                 ON purpose.role_key = role.role_key
                AND purpose.purpose = $1
               JOIN iam_builtin_role_permissions catalog
                 ON catalog.role_key = role.role_key
              WHERE role.builtin
             ON CONFLICT (role_id, permission_key) DO NOTHING
             RETURNING role_id
         )
         SELECT role.org_id, COUNT(*)
           FROM inserted
           JOIN iam_roles role
             ON role.id = inserted.role_id
       GROUP BY role.org_id",
    )
    .bind(ORGANIZATION_BOOTSTRAP_ROLE_PURPOSE)
    .fetch_all(&mut *tx)
    .await
    .map_err(sqlx_err)?;

    for (organization_id, _) in &changed_organizations {
        bump_version(&mut tx, &Id::from_string(organization_id.clone())).await?;
    }

    // The catalog tells every reader which built-in roles hold a permission. That changed.
    if catalog_added > 0 {
        sqlx::query(
            "UPDATE iam_permission_catalog_versions
                SET version = version + 1,
                    updated_at_micros = (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT
              WHERE catalog_key = 'permissions'",
        )
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
    }

    tx.commit().await.map_err(sqlx_err)?;

    let materialized = changed_organizations
        .iter()
        .map(|(_, added)| u64::try_from(*added).unwrap_or_default())
        .sum::<u64>();

    Ok(catalog_added + materialized)
}
