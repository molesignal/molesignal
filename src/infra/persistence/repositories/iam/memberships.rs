// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeSet;

use async_trait::async_trait;
use sqlx::{PgConnection, PgPool, Row};

use super::super::sqlx_err;
use crate::{
    domain::iam::{IamAssignedRole, IamMembership, IamMembershipRepository},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgIamMembershipRepository {
    pool: PgPool,
}

impl PgIamMembershipRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_membership(row: sqlx::postgres::PgRow) -> Result<IamMembership> {
    Ok(IamMembership {
        user_id: Id::from_string(row.try_get::<String, _>("user_id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        joined_at: TimestampMicros(row.try_get("joined_at_micros").map_err(sqlx_err)?),
    })
}

async fn ensure_builtin_roles(
    tx: &mut PgConnection,
    org_id: &Id,
    now: TimestampMicros,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO iam_roles (
             id, org_id, role_key, name, description, builtin,
             role_type, scope, created_at_micros, updated_at_micros
         )
         SELECT
             gen_random_uuid()::TEXT,
             $1,
             catalog.role_key,
             catalog.name,
             catalog.description,
             TRUE,
             catalog.role_type,
             catalog.scope,
             $2,
             $2
           FROM iam_builtin_roles catalog
          WHERE catalog.role_type = 'organization'
            AND catalog.scope = 'organization'
         ON CONFLICT (org_id, role_key) DO NOTHING",
    )
    .bind(&org_id.0)
    .bind(now.0)
    .execute(&mut *tx)
    .await
    .map_err(sqlx_err)?;
    sqlx::query(
        "INSERT INTO iam_role_permissions (role_id, permission_key)
         SELECT role.id, catalog.permission_key
           FROM iam_roles role
           JOIN iam_builtin_role_permissions catalog
             ON catalog.role_key = role.role_key
          WHERE role.org_id = $1
            AND role.builtin
         ON CONFLICT (role_id, permission_key) DO NOTHING",
    )
    .bind(&org_id.0)
    .execute(&mut *tx)
    .await
    .map_err(sqlx_err)?;
    Ok(())
}

#[async_trait]
impl IamMembershipRepository for PgIamMembershipRepository {
    async fn upsert(
        &self,
        membership: IamMembership,
        role_ids: &[Id],
        actor_id: &Id,
    ) -> Result<()> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let system: bool = sqlx::query("SELECT system FROM organizations WHERE id = $1")
            .bind(&membership.org_id.0)
            .fetch_optional(&mut *tx)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found("organization"))?
            .try_get("system")
            .map_err(sqlx_err)?;
        if system {
            return Err(Error::forbidden(
                "system organization does not accept membership",
            ));
        }

        ensure_builtin_roles(&mut tx, &membership.org_id, membership.joined_at).await?;
        let unique_role_ids = role_ids
            .iter()
            .map(|role_id| role_id.0.clone())
            .collect::<BTreeSet<_>>();
        if !unique_role_ids.is_empty() {
            let role_ids = unique_role_ids.iter().cloned().collect::<Vec<_>>();
            let found: i64 = sqlx::query_scalar(
                "SELECT COUNT(*)
                   FROM iam_roles
                  WHERE org_id = $1
                    AND id = ANY($2::TEXT[])",
            )
            .bind(&membership.org_id.0)
            .bind(&role_ids)
            .fetch_one(&mut *tx)
            .await
            .map_err(sqlx_err)?;
            if found != i64::try_from(role_ids.len()).unwrap_or(i64::MAX) {
                return Err(Error::invalid(
                    "every role_id must reference an IAM role in the target organization",
                ));
            }
        }

        sqlx::query(
            "INSERT INTO iam_memberships (user_id, org_id, joined_at_micros)
             VALUES ($1, $2, $3)
             ON CONFLICT (user_id, org_id) DO UPDATE
             SET joined_at_micros = LEAST(
                 iam_memberships.joined_at_micros,
                 EXCLUDED.joined_at_micros
             )",
        )
        .bind(&membership.user_id.0)
        .bind(&membership.org_id.0)
        .bind(membership.joined_at.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        sqlx::query(
            "DELETE FROM iam_role_bindings
              WHERE organization_id = $1
                AND principal_type = 'user'
                AND principal_id = $2
                AND resource_type IS NULL
                AND resource_id IS NULL",
        )
        .bind(&membership.org_id.0)
        .bind(&membership.user_id.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        for role_id in unique_role_ids {
            sqlx::query(
                "INSERT INTO iam_role_bindings (
                     id, organization_id, role_id, principal_type, principal_id,
                     conditions, created_by, created_at_micros
                 ) VALUES (
                     'binding_' || substr(md5($1 || ':' || $2 || ':' || $3), 1, 32),
                     $1, $3, 'user', $2, '{}'::JSONB, $4, $5
                 )",
            )
            .bind(&membership.org_id.0)
            .bind(&membership.user_id.0)
            .bind(role_id)
            .bind(&actor_id.0)
            .bind(TimestampMicros::now().0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }
        sqlx::query_scalar::<i64>("SELECT bump_iam_policy_version($1)")
            .bind(&membership.org_id.0)
            .fetch_one(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn list_for_user(&self, user_id: &Id) -> Result<Vec<IamMembership>> {
        let rows = sqlx::query(
            "SELECT user_id, org_id, joined_at_micros
               FROM iam_memberships
              WHERE user_id = $1
           ORDER BY joined_at_micros, org_id",
        )
        .bind(&user_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_membership).collect()
    }

    async fn list_for_org(&self, org_id: &Id) -> Result<Vec<IamMembership>> {
        let rows = sqlx::query(
            "SELECT user_id, org_id, joined_at_micros
               FROM iam_memberships
              WHERE org_id = $1
           ORDER BY joined_at_micros, user_id",
        )
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_membership).collect()
    }

    async fn assigned_roles(&self, user_id: &Id, org_id: &Id) -> Result<Vec<IamAssignedRole>> {
        let now = TimestampMicros::now();
        let rows = sqlx::query(
            "SELECT DISTINCT role.id, role.role_key, role.name, role.builtin,
                    COALESCE(catalog.display_priority, 1000) AS display_priority
               FROM iam_role_bindings binding
               JOIN iam_roles role
                 ON role.id = binding.role_id
                AND role.org_id = binding.organization_id
          LEFT JOIN iam_builtin_roles catalog
                 ON catalog.role_key = role.role_key
              WHERE binding.organization_id = $1
                AND binding.principal_type = 'user'
                AND binding.principal_id = $2
                AND binding.resource_type IS NULL
                AND binding.resource_id IS NULL
                AND (binding.starts_at_micros IS NULL OR binding.starts_at_micros <= $3)
                AND (binding.expires_at_micros IS NULL OR binding.expires_at_micros > $3)
           ORDER BY display_priority, role.name, role.id",
        )
        .bind(&org_id.0)
        .bind(&user_id.0)
        .bind(now.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter()
            .map(|row| {
                Ok(IamAssignedRole {
                    id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
                    key: row.try_get("role_key").map_err(sqlx_err)?,
                    name: row.try_get("name").map_err(sqlx_err)?,
                    builtin: row.try_get("builtin").map_err(sqlx_err)?,
                })
            })
            .collect()
    }

    async fn role_id_for_purpose(&self, org_id: &Id, purpose: &str) -> Result<Id> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        ensure_builtin_roles(&mut tx, org_id, TimestampMicros::now()).await?;
        let role_id = sqlx::query_scalar::<String>(
            "SELECT role.id
               FROM iam_builtin_role_purposes purpose
               JOIN iam_roles role
                 ON role.org_id = $1
                AND role.role_key = purpose.role_key
              WHERE purpose.purpose = $2",
        )
        .bind(&org_id.0)
        .bind(purpose)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("IAM role purpose {purpose}")))?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok(Id::from_string(role_id))
    }

    async fn remove(&self, user_id: &Id, org_id: &Id) -> Result<()> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let system: bool = sqlx::query("SELECT system FROM organizations WHERE id = $1")
            .bind(&org_id.0)
            .fetch_optional(&mut *tx)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found("organization"))?
            .try_get("system")
            .map_err(sqlx_err)?;
        if system {
            return Err(Error::forbidden(
                "system organization membership is immutable",
            ));
        }
        sqlx::query("DELETE FROM iam_memberships WHERE user_id = $1 AND org_id = $2")
            .bind(&user_id.0)
            .bind(&org_id.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        sqlx::query(
            "DELETE FROM iam_role_bindings
              WHERE organization_id = $1
                AND principal_type = 'user'
                AND principal_id = $2",
        )
        .bind(&org_id.0)
        .bind(&user_id.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        sqlx::query_scalar::<i64>("SELECT bump_iam_policy_version($1)")
            .bind(&org_id.0)
            .fetch_one(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }
}
