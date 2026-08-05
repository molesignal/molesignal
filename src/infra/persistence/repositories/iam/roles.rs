// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Org-scoped IAM role catalog.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use super::super::sqlx_err;
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IamRole {
    pub id: Id,
    pub org_id: Id,
    pub key: String,
    pub name: String,
    pub description: String,
    pub builtin: bool,
    pub role_type: String,
    pub scope: String,
    pub permissions: Vec<String>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoleUsage {
    pub memberships: i64,
    pub api_tokens: i64,
    pub invitations: i64,
    pub bindings: i64,
}

impl RoleUsage {
    pub fn total(&self) -> i64 {
        self.memberships + self.api_tokens + self.invitations + self.bindings
    }
}

#[async_trait]
pub trait IamRoleRepository: Send + Sync {
    async fn ensure_builtin_roles(&self, org_id: &Id) -> Result<()>;
    async fn list(&self, org_id: &Id) -> Result<Vec<IamRole>>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<IamRole>;
    async fn create(&self, role: IamRole) -> Result<IamRole>;
    async fn update(&self, role: IamRole) -> Result<IamRole>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;
    async fn usage_by_key(&self, org_id: &Id, role_key: &str) -> Result<RoleUsage>;
}

pub struct PgIamRoleRepository {
    pool: PgPool,
}

impl PgIamRoleRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, role_key, name, description, builtin, role_type, scope, created_at_micros, updated_at_micros";

fn row_to(row: sqlx::postgres::PgRow, permissions: Vec<String>) -> Result<IamRole> {
    Ok(IamRole {
        id: Id(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        key: row.try_get("role_key").map_err(sqlx_err)?,
        name: row.try_get("name").map_err(sqlx_err)?,
        description: row.try_get("description").map_err(sqlx_err)?,
        builtin: row.try_get("builtin").map_err(sqlx_err)?,
        role_type: row.try_get("role_type").map_err(sqlx_err)?,
        scope: row.try_get("scope").map_err(sqlx_err)?,
        permissions,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl IamRoleRepository for PgIamRoleRepository {
    async fn ensure_builtin_roles(&self, org_id: &Id) -> Result<()> {
        let now = TimestampMicros::now();
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let inserted = sqlx::query(
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
        .map_err(sqlx_err)?
        .rows_affected();
        let removed = sqlx::query(
            "DELETE FROM iam_role_permissions role_permission
              USING iam_roles role
              WHERE role_permission.role_id = role.id
                AND role.org_id = $1
                AND role.builtin
                AND NOT EXISTS (
                    SELECT 1
                      FROM iam_builtin_role_permissions catalog
                     WHERE catalog.role_key = role.role_key
                       AND catalog.permission_key = role_permission.permission_key
                )",
        )
        .bind(&org_id.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?
        .rows_affected();
        let permissions_inserted = sqlx::query(
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
        .map_err(sqlx_err)?
        .rows_affected();
        let changed = inserted > 0 || removed > 0 || permissions_inserted > 0;
        if changed {
            bump_version(&mut tx, org_id).await?;
        }
        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<IamRole>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS}
               FROM iam_roles
              WHERE org_id = $1
              ORDER BY
                CASE WHEN builtin THEN 0 ELSE 1 END,
                COALESCE(
                    (SELECT display_priority
                       FROM iam_builtin_roles catalog
                      WHERE catalog.role_key = iam_roles.role_key),
                    1000
                ),
                name ASC"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get("id").map_err(sqlx_err)?;
            let permissions = self.permissions(&id).await?;
            out.push(row_to(row, permissions)?);
        }
        Ok(out)
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<IamRole> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM iam_roles WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        let permissions = self.permissions(&id.0).await?;
        row_to(row, permissions)
    }

    async fn create(&self, mut role: IamRole) -> Result<IamRole> {
        role.permissions = self.validate_permission_keys(&role.permissions).await?;
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        sqlx::query(
            "INSERT INTO iam_roles
                (id, org_id, role_key, name, description, builtin, role_type, scope,
                 created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(&role.id.0)
        .bind(&role.org_id.0)
        .bind(&role.key)
        .bind(&role.name)
        .bind(&role.description)
        .bind(role.builtin)
        .bind(&role.role_type)
        .bind(&role.scope)
        .bind(role.created_at.0)
        .bind(role.updated_at.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        replace_permissions(&mut tx, &role.id.0, &role.permissions).await?;
        bump_version(&mut tx, &role.org_id).await?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok(role)
    }

    async fn update(&self, mut role: IamRole) -> Result<IamRole> {
        role.permissions = self.validate_permission_keys(&role.permissions).await?;
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let result = sqlx::query(
            "UPDATE iam_roles
                SET name = $3,
                    description = $4,
                    updated_at_micros = $5
              WHERE org_id = $1 AND id = $2",
        )
        .bind(&role.org_id.0)
        .bind(&role.id.0)
        .bind(&role.name)
        .bind(&role.description)
        .bind(role.updated_at.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found("role not found"));
        }
        replace_permissions(&mut tx, &role.id.0, &role.permissions).await?;
        bump_version(&mut tx, &role.org_id).await?;
        tx.commit().await.map_err(sqlx_err)?;
        self.get(&role.org_id, &role.id).await
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        sqlx::query("DELETE FROM iam_role_permissions WHERE role_id = $1")
            .bind(&id.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        let result = sqlx::query("DELETE FROM iam_roles WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found("role not found"));
        }
        bump_version(&mut tx, org_id).await?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn usage_by_key(&self, org_id: &Id, role_key: &str) -> Result<RoleUsage> {
        let row = sqlx::query(
            "SELECT
                (SELECT COUNT(DISTINCT b.principal_id)
                   FROM iam_role_bindings b
                   JOIN iam_roles r ON r.id = b.role_id
                  WHERE b.organization_id = $1
                    AND b.principal_type = 'user'
                    AND b.resource_type IS NULL
                    AND b.resource_id IS NULL
                    AND r.role_key = $2) AS memberships,
                (SELECT COUNT(*)
                   FROM api_tokens token
                   JOIN iam_roles r ON r.id = token.role_id
                  WHERE token.org_id = $1
                    AND r.role_key = $2
                    AND token.revoked = FALSE) AS api_tokens,
                (SELECT COUNT(*)
                   FROM invitations invitation
                   JOIN iam_roles r ON r.id = invitation.role_id
                  WHERE invitation.org_id = $1
                    AND r.role_key = $2
                    AND invitation.status <> 'revoked') AS invitations,
                (SELECT COUNT(*)
                   FROM iam_role_bindings b
                   JOIN iam_roles r ON r.id = b.role_id
                  WHERE b.organization_id = $1
                    AND r.role_key = $2) AS bindings",
        )
        .bind(&org_id.0)
        .bind(role_key)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(RoleUsage {
            memberships: row.try_get("memberships").map_err(sqlx_err)?,
            api_tokens: row.try_get("api_tokens").map_err(sqlx_err)?,
            invitations: row.try_get("invitations").map_err(sqlx_err)?,
            bindings: row.try_get("bindings").map_err(sqlx_err)?,
        })
    }
}

impl PgIamRoleRepository {
    async fn validate_permission_keys(&self, permissions: &[String]) -> Result<Vec<String>> {
        let mut normalized = permissions
            .iter()
            .map(|permission| permission.trim().to_ascii_lowercase())
            .collect::<Vec<_>>();
        normalized.sort();
        normalized.dedup();
        if normalized.is_empty() {
            return Ok(normalized);
        }
        let rows = sqlx::query(
            "SELECT permission_key, scope
               FROM iam_permissions
              WHERE permission_key = ANY($1)
           ORDER BY permission_key",
        )
        .bind(&normalized)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        let mut validated = Vec::with_capacity(rows.len());
        for row in rows {
            let permission_key: String = row.try_get("permission_key").map_err(sqlx_err)?;
            let scope: String = row.try_get("scope").map_err(sqlx_err)?;
            if scope != "organization" {
                return Err(Error::invalid(format!(
                    "permission {permission_key} has {scope} scope, expected organization"
                )));
            }
            validated.push(permission_key);
        }
        if validated.len() != normalized.len() {
            let unknown = normalized
                .iter()
                .find(|permission| !validated.contains(permission))
                .cloned()
                .unwrap_or_default();
            return Err(Error::invalid(format!("unknown permission: {unknown}")));
        }
        Ok(validated)
    }

    async fn permissions(&self, role_id: &str) -> Result<Vec<String>> {
        let rows = sqlx::query(
            "SELECT role_permission.permission_key
               FROM iam_role_permissions role_permission
               JOIN iam_permissions permission
                 ON permission.permission_key = role_permission.permission_key
                AND permission.scope = 'organization'
              WHERE role_permission.role_id = $1
           ORDER BY role_permission.permission_key ASC",
        )
        .bind(role_id)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter()
            .map(|row| row.try_get("permission_key").map_err(sqlx_err))
            .collect()
    }
}

async fn replace_permissions(
    tx: &mut sqlx::PgConnection,
    role_id: &str,
    permissions: &[String],
) -> Result<()> {
    sqlx::query("DELETE FROM iam_role_permissions WHERE role_id = $1")
        .bind(role_id)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
    for permission in permissions {
        sqlx::query(
            "INSERT INTO iam_role_permissions (role_id, permission_key)
             VALUES ($1, $2)",
        )
        .bind(role_id)
        .bind(permission)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
    }
    Ok(())
}

async fn bump_version(tx: &mut sqlx::PgConnection, org_id: &Id) -> Result<u64> {
    let version: i64 = sqlx::query_scalar("SELECT bump_iam_policy_version($1)")
        .bind(&org_id.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_err)?;
    u64::try_from(version).map_err(|_| Error::internal("negative IAM policy version"))
}
