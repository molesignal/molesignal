// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgConnection, PgPool, Row};

use super::sqlx_err;
use crate::{
    domain::iam::{Organization, OrganizationRepository, SYSTEM_ORG_NAME, SYSTEM_ORG_SLUG},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgOrganizationRepository {
    pool: PgPool,
}

impl PgOrganizationRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 每次启动都执行的并发安全系统组织引导。结构性冲突视为篡改并 fail-fast。
    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "organizations")
    )]
    pub async fn ensure_system_organization(&self) -> Result<Organization> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('molesignal.system.organization'))")
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        let existing = sqlx::query(
            "SELECT id, name, slug, system, disabled, created_at_micros
             FROM organizations
             WHERE system OR name = $1 OR slug = $2
             ORDER BY system DESC
             LIMIT 1",
        )
        .bind(SYSTEM_ORG_NAME)
        .bind(SYSTEM_ORG_SLUG)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        let organization = if let Some(row) = existing {
            let organization = row_to_org(row)?;
            if !organization.system
                || organization.name != SYSTEM_ORG_NAME
                || organization.slug != SYSTEM_ORG_SLUG
                || organization.disabled
            {
                return Err(Error::conflict(
                    "tampered or conflicting `_sys` organization identity",
                ));
            }
            organization
        } else {
            let organization = Organization {
                id: Id::new(),
                name: SYSTEM_ORG_NAME.into(),
                slug: SYSTEM_ORG_SLUG.into(),
                system: true,
                disabled: false,
                created_at: TimestampMicros::now(),
            };
            sqlx::query(
                "INSERT INTO organizations (id, name, slug, system, created_at_micros)
                 VALUES ($1, $2, $3, TRUE, $4)",
            )
            .bind(&organization.id.0)
            .bind(&organization.name)
            .bind(&organization.slug)
            .bind(organization.created_at.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
            organization
        };
        sync_system_iam_roles(&mut tx, &organization).await?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok(organization)
    }
}

async fn sync_system_iam_roles(tx: &mut PgConnection, organization: &Organization) -> Result<()> {
    let now = TimestampMicros::now();
    let roles_changed = sqlx::query(
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
          WHERE catalog.role_type = 'platform'
            AND catalog.scope = 'platform'
         ON CONFLICT (org_id, role_key) DO UPDATE
         SET name = EXCLUDED.name,
             description = EXCLUDED.description,
             builtin = TRUE,
             role_type = EXCLUDED.role_type,
             scope = EXCLUDED.scope,
             updated_at_micros = EXCLUDED.updated_at_micros
         WHERE iam_roles.name IS DISTINCT FROM EXCLUDED.name
            OR iam_roles.description IS DISTINCT FROM EXCLUDED.description
            OR iam_roles.builtin IS DISTINCT FROM TRUE
            OR iam_roles.role_type IS DISTINCT FROM EXCLUDED.role_type
            OR iam_roles.scope IS DISTINCT FROM EXCLUDED.scope",
    )
    .bind(&organization.id.0)
    .bind(now.0)
    .execute(&mut *tx)
    .await
    .map_err(sqlx_err)?
    .rows_affected();

    let permissions_removed = sqlx::query(
        "DELETE FROM iam_role_permissions role_permission
          USING iam_roles role
          WHERE role_permission.role_id = role.id
            AND role.org_id = $1
            AND role.builtin
            AND role.scope = 'platform'
            AND NOT EXISTS (
                SELECT 1
                  FROM iam_builtin_role_permissions catalog
                 WHERE catalog.role_key = role.role_key
                   AND catalog.permission_key = role_permission.permission_key
            )",
    )
    .bind(&organization.id.0)
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
            AND role.scope = 'platform'
         ON CONFLICT (role_id, permission_key) DO NOTHING",
    )
    .bind(&organization.id.0)
    .execute(&mut *tx)
    .await
    .map_err(sqlx_err)?
    .rows_affected();

    if roles_changed > 0 || permissions_removed > 0 || permissions_inserted > 0 {
        sqlx::query_scalar::<i64>("SELECT bump_iam_policy_version($1)")
            .bind(&organization.id.0)
            .fetch_one(&mut *tx)
            .await
            .map_err(sqlx_err)?;
    }
    Ok(())
}

fn row_to_org(row: sqlx::postgres::PgRow) -> Result<Organization> {
    Ok(Organization {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        slug: row.try_get("slug").map_err(sqlx_err)?,
        system: row.try_get("system").map_err(sqlx_err)?,
        disabled: row.try_get("disabled").map_err(sqlx_err)?,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl OrganizationRepository for PgOrganizationRepository {
    async fn create(&self, org: Organization) -> Result<Organization> {
        org.validate_system_invariants()?;
        sqlx::query(
            "INSERT INTO organizations (id, name, slug, system, disabled, created_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&org.id.0)
        .bind(&org.name)
        .bind(&org.slug)
        .bind(org.system)
        .bind(org.disabled)
        .bind(org.created_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(org)
    }

    async fn get(&self, id: &Id) -> Result<Organization> {
        let row = sqlx::query(
            "SELECT id, name, slug, system, disabled, created_at_micros FROM organizations WHERE id = $1",
        )
        .bind(&id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row_to_org(row)
    }

    async fn get_by_slug(&self, slug: &str) -> Result<Organization> {
        let row = sqlx::query(
            "SELECT id, name, slug, system, disabled, created_at_micros FROM organizations WHERE slug = $1",
        )
        .bind(slug)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row_to_org(row)
    }

    async fn list(&self) -> Result<Vec<Organization>> {
        let rows = sqlx::query("SELECT id, name, slug, system, disabled, created_at_micros FROM organizations ORDER BY created_at_micros")
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_org).collect()
    }

    async fn update_name(&self, id: &Id, name: String) -> Result<Organization> {
        self.get(id).await?.ensure_mutable()?;
        sqlx::query("UPDATE organizations SET name = $2 WHERE id = $1")
            .bind(&id.0)
            .bind(&name)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        self.get(id).await
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "UPDATE", db.collection.name = "organizations")
    )]
    async fn set_disabled(&self, id: &Id, disabled: bool) -> Result<Organization> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('molesignal.organization.status'))")
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        let row = sqlx::query(
            "SELECT id, name, slug, system, disabled, created_at_micros
             FROM organizations
             WHERE id = $1
             FOR UPDATE",
        )
        .bind(&id.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        let current = row_to_org(row)?;
        current.ensure_mutable()?;

        if disabled && !current.disabled {
            let enabled_tenants = sqlx::query_scalar::<i64>(
                "SELECT COUNT(*) FROM organizations WHERE NOT system AND NOT disabled",
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(sqlx_err)?;
            if enabled_tenants <= 1 {
                return Err(Error::invalid(
                    "cannot disable the last enabled tenant organization",
                ));
            }
        }

        sqlx::query("UPDATE organizations SET disabled = $2 WHERE id = $1")
            .bind(&id.0)
            .bind(disabled)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        tx.commit().await.map_err(sqlx_err)?;
        self.get(id).await
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "organizations")
    )]
    async fn delete(&self, id: &Id) -> Result<()> {
        self.get(id).await?.ensure_mutable()?;
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        sqlx::query("DELETE FROM iam_memberships WHERE org_id = $1")
            .bind(&id.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(&id.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }
}
