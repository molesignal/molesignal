// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Permission policies: the built-in ones, which belong to no organization, and an
//! organization's own. A policy keeps its document and the permissions the document resolved to
//! when it was saved; roles attach policies and API tokens follow them.

use std::collections::BTreeSet;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row, postgres::PgRow, types::Json};

use super::{super::sqlx_err, roles::bump_version};
use crate::{
    domain::iam::policy::PolicyDocument,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IamPolicy {
    pub id: Id,
    /// The organization a custom policy belongs to; none for a built-in policy.
    pub org_id: Option<Id>,
    pub key: String,
    pub name: String,
    pub description: String,
    pub document: Value,
    /// What the document allows, resolved against the permission catalog when it was saved.
    pub permissions: Vec<String>,
    pub created_by: Option<Id>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl IamPolicy {
    pub fn builtin(&self) -> bool {
        self.org_id.is_none()
    }
}

/// What uses a policy in an organization.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct PolicyUsage {
    /// Roles that attach it.
    pub roles: i64,
    /// API tokens that follow it and still work: neither revoked nor expired.
    pub tokens: i64,
}

impl PolicyUsage {
    pub fn total(&self) -> i64 {
        self.roles + self.tokens
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IamPolicyWithUsage {
    pub policy: IamPolicy,
    pub usage: PolicyUsage,
}

#[async_trait]
pub trait IamPolicyRepository: Send + Sync {
    /// The built-in policies and the organization's own, with what uses each there.
    async fn list(&self, org_id: &Id) -> Result<Vec<IamPolicyWithUsage>>;
    /// A built-in policy or one of the organization's, with what uses it there.
    async fn get(&self, org_id: &Id, id: &Id) -> Result<IamPolicyWithUsage>;
    async fn create(&self, policy: IamPolicy) -> Result<IamPolicy>;
    /// Saves a custom policy's name, description, document and permissions.
    async fn update(&self, policy: IamPolicy) -> Result<IamPolicy>;
    /// Deletes a custom policy that no role attaches and no working token follows. A token
    /// that no longer works and followed it keeps the permissions it had, as a list.
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;
    /// Resolves the documents of custom policies that name permissions by pattern again, so a
    /// permission added to the catalog reaches them. Returns how many changed. Run at startup.
    async fn refresh_patterns(&self) -> Result<u64>;
}

pub struct PgIamPolicyRepository {
    pool: PgPool,
}

impl PgIamPolicyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "policy.id, policy.organization_id, policy.policy_key, policy.name,
                    policy.description, policy.document, policy.permissions, policy.created_by,
                    policy.created_at, policy.updated_at";

/// What uses `policy` in the organization `$1` at the time `$2`, as two columns.
const USAGE: &str = "(SELECT COUNT(*)
                        FROM iam_roles role
                       WHERE role.org_id = $1
                         AND policy.id = ANY(role.policy_ids)) AS roles,
                     (SELECT COUNT(*)
                        FROM api_tokens token
                       WHERE token.org_id = $1
                         AND token.policy_id = policy.id
                         AND NOT token.revoked
                         AND (token.expires_at IS NULL
                              OR token.expires_at > $2)) AS tokens";

fn row_to(row: &PgRow) -> Result<IamPolicy> {
    Ok(IamPolicy {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: row
            .try_get::<Option<String>, _>("organization_id")
            .map_err(sqlx_err)?
            .map(Id),
        key: row.try_get("policy_key").map_err(sqlx_err)?,
        name: row.try_get("name").map_err(sqlx_err)?,
        description: row.try_get("description").map_err(sqlx_err)?,
        document: row
            .try_get::<Json<Value>, _>("document")
            .map_err(sqlx_err)?
            .0,
        permissions: row.try_get("permissions").map_err(sqlx_err)?,
        created_by: row
            .try_get::<Option<String>, _>("created_by")
            .map_err(sqlx_err)?
            .map(Id),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
    })
}

fn with_usage(row: &PgRow) -> Result<IamPolicyWithUsage> {
    Ok(IamPolicyWithUsage {
        policy: row_to(row)?,
        usage: PolicyUsage {
            roles: row.try_get("roles").map_err(sqlx_err)?,
            tokens: row.try_get("tokens").map_err(sqlx_err)?,
        },
    })
}

#[async_trait]
impl IamPolicyRepository for PgIamPolicyRepository {
    async fn list(&self, org_id: &Id) -> Result<Vec<IamPolicyWithUsage>> {
        let sql = format!(
            "SELECT {COLS}, {USAGE}
               FROM iam_policies policy
              WHERE policy.organization_id IS NULL OR policy.organization_id = $1
           ORDER BY policy.organization_id IS NOT NULL, lower(policy.name), policy.policy_key"
        );

        sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(TimestampMicros::now())
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
            .iter()
            .map(with_usage)
            .collect()
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<IamPolicyWithUsage> {
        let sql = format!(
            "SELECT {COLS}, {USAGE}
               FROM iam_policies policy
              WHERE policy.id = $3
                AND (policy.organization_id IS NULL OR policy.organization_id = $1)"
        );

        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(TimestampMicros::now())
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found("permission policy not found"))?;

        with_usage(&row)
    }

    async fn create(&self, policy: IamPolicy) -> Result<IamPolicy> {
        let Some(org_id) = policy.org_id.as_ref() else {
            return Err(Error::invalid(
                "a new permission policy belongs to an organization",
            ));
        };

        let taken: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1
                   FROM iam_policies
                  WHERE policy_key = $2
                    AND (organization_id IS NULL OR organization_id = $1)
             )",
        )
        .bind(&org_id.0)
        .bind(&policy.key)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if taken {
            return Err(Error::conflict(format!(
                "a permission policy with key {} already exists",
                policy.key
            )));
        }

        let sql = format!(
            "INSERT INTO iam_policies AS policy
                 (id, organization_id, policy_key, name, description, document, permissions,
                  created_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT DO NOTHING
             RETURNING {COLS}"
        );

        let row = sqlx::query(&sql)
            .bind(&policy.id.0)
            .bind(&org_id.0)
            .bind(&policy.key)
            .bind(&policy.name)
            .bind(&policy.description)
            .bind(Json(&policy.document))
            .bind(&policy.permissions)
            .bind(policy.created_by.as_ref().map(|id| &id.0))
            .bind(policy.created_at)
            .bind(policy.updated_at)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| {
                Error::conflict(format!(
                    "a permission policy with key {} already exists",
                    policy.key
                ))
            })?;

        row_to(&row)
    }

    async fn update(&self, policy: IamPolicy) -> Result<IamPolicy> {
        let Some(org_id) = policy.org_id.as_ref() else {
            return Err(Error::forbidden(
                "built-in permission policies cannot be edited",
            ));
        };

        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let sql = format!(
            "UPDATE iam_policies policy
                SET name = $3,
                    description = $4,
                    document = $5,
                    permissions = $6,
                    updated_at = $7
              WHERE policy.organization_id = $1 AND policy.id = $2
          RETURNING {COLS}"
        );

        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&policy.id.0)
            .bind(&policy.name)
            .bind(&policy.description)
            .bind(Json(&policy.document))
            .bind(&policy.permissions)
            .bind(policy.updated_at)
            .fetch_optional(&mut *tx)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found("permission policy not found"))?;

        // Roles that attach it and tokens that follow it grant what it allows now.
        bump_version(&mut tx, org_id).await?;
        tx.commit().await.map_err(sqlx_err)?;
        row_to(&row)
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let sql = format!(
            "SELECT {COLS}, {USAGE}
               FROM iam_policies policy
              WHERE policy.organization_id = $1 AND policy.id = $3
                FOR UPDATE"
        );

        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(TimestampMicros::now())
            .bind(&id.0)
            .fetch_optional(&mut *tx)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found("permission policy not found"))?;

        let IamPolicyWithUsage { policy, usage } = with_usage(&row)?;

        if usage.total() > 0 {
            return Err(Error::conflict(format!(
                "permission policy is in use: roles={}, tokens={}",
                usage.roles, usage.tokens
            )));
        }

        // A revoked or expired token still shows what it was limited to.
        sqlx::query(
            "UPDATE api_tokens
                SET policy_id = NULL,
                    permissions = $3
              WHERE org_id = $1 AND policy_id = $2",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(&policy.permissions)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        sqlx::query("DELETE FROM iam_policies WHERE organization_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;

        bump_version(&mut tx, org_id).await?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn refresh_patterns(&self) -> Result<u64> {
        let catalog: Vec<(String, String)> =
            sqlx::query_as("SELECT permission_key::TEXT, scope::TEXT FROM iam_permissions")
                .fetch_all(&self.pool)
                .await
                .map_err(sqlx_err)?;

        let keys_of = |wanted: &str| {
            catalog
                .iter()
                .filter(|(_, scope)| scope == wanted)
                .map(|(key, _)| key.clone())
                .collect::<BTreeSet<_>>()
        };

        let organization_catalog = keys_of("organization");
        let platform_catalog = keys_of("platform");

        // A policy of the platform organization allows platform permissions, any other
        // organization permissions.
        let platform_organizations: BTreeSet<String> =
            sqlx::query_scalar("SELECT id FROM organizations WHERE system")
                .fetch_all(&self.pool)
                .await
                .map_err(sqlx_err)?
                .into_iter()
                .collect();

        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM iam_policies policy WHERE policy.organization_id IS NOT NULL"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let mut changed = 0;

        for row in &rows {
            let policy = row_to(row)?;

            let Some(org_id) = policy.org_id.as_ref() else {
                continue;
            };

            let catalog = if platform_organizations.contains(&org_id.0) {
                &platform_catalog
            } else {
                &organization_catalog
            };

            let resolved = match PolicyDocument::parse(&policy.document) {
                Ok(document) if document.has_wildcards() => document.resolve(catalog),
                _ => continue,
            };

            let permissions = match resolved {
                Ok(permissions) => permissions,
                Err(error) => {
                    tracing::warn!(policy = %policy.id, %error, "permission policy no longer resolves");
                    continue;
                }
            };

            if permissions == policy.permissions {
                continue;
            }

            let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

            sqlx::query("UPDATE iam_policies SET permissions = $2 WHERE id = $1")
                .bind(&policy.id.0)
                .bind(&permissions)
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;

            bump_version(&mut tx, org_id).await?;
            tx.commit().await.map_err(sqlx_err)?;
            changed += 1;
        }

        Ok(changed)
    }
}
