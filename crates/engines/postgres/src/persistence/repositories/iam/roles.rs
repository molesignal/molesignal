// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Org-scoped IAM role catalog.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use super::super::sqlx_err;
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

mod owner_catalog;
mod queries;

use queries::{get_sql, list_sql, list_with_usage_sql};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IamRole {
    pub id: Id,
    pub org_id: Id,
    pub key: String,
    pub name: String,
    pub description: String,
    pub builtin: bool,
    pub role_type: String,
    pub scope: String,
    /// The role's own permissions.
    pub permissions: Vec<String>,
    /// The permission policies the role attaches.
    pub policy_ids: Vec<Id>,
    /// What the role grants: its own permissions and those of its policies. Read only.
    pub effective_permissions: Vec<String>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleUsage {
    pub memberships: i64,
    pub service_accounts: i64,
    pub invitations: i64,
    pub bindings: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IamRoleWithUsage {
    pub role: IamRole,
    pub usage: RoleUsage,
}

impl RoleUsage {
    pub fn total(&self) -> i64 {
        self.memberships + self.service_accounts + self.invitations + self.bindings
    }
}

#[async_trait]
pub trait IamRoleRepository: Send + Sync {
    async fn ensure_builtin_roles(&self, org_id: &Id) -> Result<()>;
    /// Keep the owner role holding exactly the organization permissions of the catalog. Returns
    /// how many roles changed. Run at startup, after migrations.
    async fn grant_catalog_to_organization_owners(&self) -> Result<u64>;
    async fn list(&self, org_id: &Id) -> Result<Vec<IamRole>>;
    async fn list_with_usage(&self, org_id: &Id) -> Result<Vec<IamRoleWithUsage>>;
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
        policy_ids: row
            .try_get::<Vec<String>, _>("policy_ids")
            .map_err(sqlx_err)?
            .into_iter()
            .map(Id)
            .collect(),
        effective_permissions: row.try_get("effective_permissions").map_err(sqlx_err)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
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
                 role_type, scope, created_at, updated_at, permissions
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
                 $2,
                 catalog.permissions
               FROM iam_roles catalog
               JOIN organizations organization
                 ON organization.id = $1
              WHERE catalog.org_id IS NULL
                AND catalog.scope =
                    CASE WHEN organization.system THEN 'platform' ELSE 'organization' END
             ON CONFLICT (org_id, role_key) DO NOTHING",
        )
        .bind(&org_id.0)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

        // A built-in role has exactly the permissions of its built-in row.
        let synced = sqlx::query(
            "UPDATE iam_roles role
                SET permissions = catalog.permissions
               FROM iam_roles catalog
              WHERE role.org_id = $1
                AND role.builtin
                AND catalog.org_id IS NULL
                AND catalog.role_key = role.role_key
                AND role.permissions IS DISTINCT FROM catalog.permissions",
        )
        .bind(&org_id.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

        let changed = inserted > 0 || synced > 0;

        if changed {
            bump_version(&mut tx, org_id).await?;
        }

        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn grant_catalog_to_organization_owners(&self) -> Result<u64> {
        owner_catalog::grant_catalog_to_organization_owners(&self.pool).await
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<IamRole>> {
        let rows = sqlx::query(&list_sql())
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                let permissions = row.try_get("permissions").map_err(sqlx_err)?;
                row_to(row, permissions)
            })
            .collect()
    }

    async fn list_with_usage(&self, org_id: &Id) -> Result<Vec<IamRoleWithUsage>> {
        let rows = sqlx::query(&list_with_usage_sql())
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                let permissions = row.try_get("permissions").map_err(sqlx_err)?;

                let usage = RoleUsage {
                    memberships: row.try_get("memberships").map_err(sqlx_err)?,
                    service_accounts: row.try_get("service_accounts").map_err(sqlx_err)?,
                    invitations: row.try_get("invitations").map_err(sqlx_err)?,
                    bindings: row.try_get("bindings").map_err(sqlx_err)?,
                };

                Ok(IamRoleWithUsage {
                    role: row_to(row, permissions)?,
                    usage,
                })
            })
            .collect()
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<IamRole> {
        let row = sqlx::query(&get_sql())
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;

        let permissions = row.try_get("permissions").map_err(sqlx_err)?;
        row_to(row, permissions)
    }

    async fn create(&self, mut role: IamRole) -> Result<IamRole> {
        role.permissions = self
            .validate_permission_keys(&role.permissions, &role.scope)
            .await?;
        self.validate_policy_ids(&role.org_id, &role.policy_ids)
            .await?;
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        sqlx::query(
            "INSERT INTO iam_roles
                (id, org_id, role_key, name, description, builtin, role_type, scope,
                 policy_ids, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(&role.id.0)
        .bind(&role.org_id.0)
        .bind(&role.key)
        .bind(&role.name)
        .bind(&role.description)
        .bind(role.builtin)
        .bind(&role.role_type)
        .bind(&role.scope)
        .bind(policy_ids(&role.policy_ids))
        .bind(role.created_at)
        .bind(role.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        replace_permissions(&mut tx, &role.id.0, &role.permissions).await?;
        bump_version(&mut tx, &role.org_id).await?;
        tx.commit().await.map_err(sqlx_err)?;
        self.get(&role.org_id, &role.id).await
    }

    async fn update(&self, mut role: IamRole) -> Result<IamRole> {
        role.permissions = self
            .validate_permission_keys(&role.permissions, &role.scope)
            .await?;
        self.validate_policy_ids(&role.org_id, &role.policy_ids)
            .await?;
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let result = sqlx::query(
            "UPDATE iam_roles
                SET name = $3,
                    description = $4,
                    policy_ids = $5,
                    updated_at = $6
              WHERE org_id = $1 AND id = $2",
        )
        .bind(&role.org_id.0)
        .bind(&role.id.0)
        .bind(&role.name)
        .bind(&role.description)
        .bind(policy_ids(&role.policy_ids))
        .bind(role.updated_at)
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
                   FROM service_accounts account
                  JOIN iam_roles r ON r.id = account.role_id
                  WHERE account.org_id = $1
                    AND r.role_key = $2
                    AND account.deleted_at IS NULL) AS service_accounts,
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
            service_accounts: row.try_get("service_accounts").map_err(sqlx_err)?,
            invitations: row.try_get("invitations").map_err(sqlx_err)?,
            bindings: row.try_get("bindings").map_err(sqlx_err)?,
        })
    }
}

impl PgIamRoleRepository {
    /// A role attaches built-in policies and its organization's own.
    async fn validate_policy_ids(&self, org_id: &Id, ids: &[Id]) -> Result<()> {
        let ids = policy_ids(ids);

        if ids.is_empty() {
            return Ok(());
        }

        let known: Vec<String> = sqlx::query_scalar(
            "SELECT id
               FROM iam_policies
              WHERE id = ANY($1)
                AND (organization_id IS NULL OR organization_id = $2)",
        )
        .bind(&ids)
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        match ids
            .iter()
            .find(|id| !known.iter().any(|known| known == *id))
        {
            Some(unknown) => Err(Error::invalid(format!(
                "unknown permission policy: {unknown}"
            ))),
            None => Ok(()),
        }
    }

    /// The permissions, sorted and once each, all of them known and of the role's scope.
    async fn validate_permission_keys(
        &self,
        permissions: &[String],
        expected_scope: &str,
    ) -> Result<Vec<String>> {
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

            if scope != expected_scope {
                return Err(Error::invalid(format!(
                    "permission {permission_key} has {scope} scope, expected {expected_scope}"
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
}

/// A role lists the policies it attaches once each, in the order they were given.
fn policy_ids(ids: &[Id]) -> Vec<&str> {
    let mut seen = std::collections::BTreeSet::new();

    ids.iter()
        .map(|id| id.0.as_str())
        .filter(|id| seen.insert(*id))
        .collect()
}

/// A role lists its permissions sorted and once each.
async fn replace_permissions(
    tx: &mut sqlx::PgConnection,
    role_id: &str,
    permissions: &[String],
) -> Result<()> {
    let permissions: Vec<&str> = permissions
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    sqlx::query("UPDATE iam_roles SET permissions = $2 WHERE id = $1")
        .bind(role_id)
        .bind(&permissions)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

    Ok(())
}

pub(super) async fn bump_version(tx: &mut sqlx::PgConnection, org_id: &Id) -> Result<u64> {
    let version: i64 = sqlx::query_scalar("SELECT bump_iam_policy_version($1)")
        .bind(&org_id.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_err)?;

    u64::try_from(version).map_err(|_| Error::internal("negative IAM policy version"))
}
