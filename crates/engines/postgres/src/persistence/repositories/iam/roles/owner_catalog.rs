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

    // The owner entry of the built-in catalog lists every permission there is. The keys are
    // aggregated as TEXT: the role column is TEXT[], and since PostgreSQL 14 a COALESCE of a
    // VARCHAR[] and a TEXT[] is VARCHAR[], for which `TEXT[] @>` has no operator.
    let catalog_added: i64 = sqlx::query_scalar(
        "WITH catalog AS (
             SELECT COALESCE(
                        array_agg(permission_key::TEXT ORDER BY permission_key),
                        ARRAY[]::TEXT[]
                    ) AS keys
               FROM iam_permissions
         ), lacking AS (
             SELECT role.role_key,
                    cardinality(catalog.keys) - cardinality(role.permissions) AS missing
               FROM iam_builtin_roles role
              CROSS JOIN catalog
              WHERE role.purpose = $1
                AND NOT role.permissions @> catalog.keys
         ), updated AS (
             UPDATE iam_builtin_roles role
                SET permissions = catalog.keys
               FROM catalog, lacking
              WHERE role.role_key = lacking.role_key
          RETURNING lacking.missing
         )
         SELECT COALESCE(SUM(missing), 0)::BIGINT FROM updated",
    )
    .bind(ORGANIZATION_BOOTSTRAP_ROLE_PURPOSE)
    .fetch_one(&mut *tx)
    .await
    .map_err(sqlx_err)?;

    // Every organization's owner role gains what it lacks of the catalog entry.
    let changed_organizations: Vec<(String, i64)> = sqlx::query_as(
        "WITH owner AS (
             SELECT role_key, permissions
               FROM iam_builtin_roles
              WHERE purpose = $1
         ), lacking AS (
             SELECT role.id, role.org_id,
                    (SELECT COUNT(*)
                       FROM unnest(owner.permissions) AS listed(permission_key)
                      WHERE NOT listed.permission_key = ANY(role.permissions)) AS missing
               FROM iam_roles role
               JOIN owner
                 ON owner.role_key = role.role_key
              WHERE role.builtin
                AND NOT role.permissions @> owner.permissions
         ), updated AS (
             UPDATE iam_roles role
                SET permissions = ARRAY(
                        SELECT DISTINCT listed.permission_key
                          FROM unnest(role.permissions || owner.permissions)
                               AS listed(permission_key)
                      ORDER BY listed.permission_key
                    )
               FROM owner, lacking
              WHERE lacking.id = role.id
                AND owner.role_key = role.role_key
          RETURNING lacking.org_id, lacking.missing
         )
         SELECT org_id, SUM(missing)::BIGINT
           FROM updated
       GROUP BY org_id",
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
            "UPDATE iam_catalog_versions
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

    Ok(u64::try_from(catalog_added).unwrap_or_default() + materialized)
}
