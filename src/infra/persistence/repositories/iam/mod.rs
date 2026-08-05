// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Persistence boundary for unified IAM access.
//!
//! Mutations bump `iam_policy_versions` in the same transaction. Readers use
//! the version as part of their cache key, so a committed revocation is visible
//! on the next request without relying on a TTL.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::iam::{
        IamAssignedRole,
        access::{
            IamCrossOrgGrant, IamCrossOrgGrantQuery, IamCrossOrgGrantStatus, IamPrincipalType,
            IamRepository, IamResourceRelationship, IamRoleBinding,
            ResolvedIamResourceRelationship, ResolvedIamRoleBinding,
        },
        catalog::{
            IamPermissionBundle, IamPermissionCatalog, IamPermissionDefinition, IamPermissionScope,
        },
        navigation::{IamRouteCatalog, IamRouteDefinition, IamRoutePermissionMode, IamRouteScope},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub mod memberships;
pub mod platform_administrators;
pub mod roles;

pub struct PgIamRepository {
    pool: PgPool,
}

impl PgIamRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl IamRepository for PgIamRepository {
    async fn permission_catalog_version(&self) -> Result<u64> {
        let version: i64 = sqlx::query_scalar(
            "SELECT version
               FROM iam_permission_catalog_versions
              WHERE catalog_key = 'permissions'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        u64::try_from(version)
            .map_err(|_| Error::internal("negative IAM permission catalog version"))
    }

    async fn permission_catalog(&self) -> Result<IamPermissionCatalog> {
        let version: i64 = sqlx::query_scalar(
            "SELECT version
               FROM iam_permission_catalog_versions
              WHERE catalog_key = 'permissions'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let permission_rows = sqlx::query(
            "SELECT
                 permission.permission_key,
                 permission.scope,
                 permission.domain,
                 permission.label_key,
                 permission.description_key,
                 permission.feature,
                 COALESCE(
                     array_agg(builtin.role_key ORDER BY builtin.role_key)
                         FILTER (WHERE builtin.role_key IS NOT NULL),
                     ARRAY[]::TEXT[]
                 ) AS builtin_roles
               FROM iam_permissions permission
          LEFT JOIN iam_builtin_role_permissions builtin
                 ON builtin.permission_key = permission.permission_key
           GROUP BY permission.permission_key,
                    permission.scope,
                    permission.domain,
                    permission.label_key,
                    permission.description_key,
                    permission.feature
           ORDER BY permission.scope, permission.domain, permission.permission_key",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        let permissions = permission_rows
            .into_iter()
            .map(|row| {
                let scope: String = row.try_get("scope").map_err(sqlx_err)?;
                Ok(IamPermissionDefinition {
                    key: row.try_get("permission_key").map_err(sqlx_err)?,
                    scope: permission_scope_from_str(&scope)?,
                    domain: row.try_get("domain").map_err(sqlx_err)?,
                    label_key: row.try_get("label_key").map_err(sqlx_err)?,
                    description_key: row.try_get("description_key").map_err(sqlx_err)?,
                    builtin_roles: row.try_get("builtin_roles").map_err(sqlx_err)?,
                    feature: row.try_get("feature").map_err(sqlx_err)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let bundle_rows = sqlx::query(
            "SELECT
                 bundle.bundle_key,
                 bundle.label_key,
                 bundle.description_key,
                 COALESCE(
                     array_agg(item.permission_key ORDER BY item.position)
                         FILTER (WHERE item.permission_key IS NOT NULL),
                     ARRAY[]::TEXT[]
                 ) AS permissions
               FROM iam_permission_bundles bundle
          LEFT JOIN iam_permission_bundle_items item
                 ON item.bundle_key = bundle.bundle_key
           GROUP BY bundle.bundle_key, bundle.label_key, bundle.description_key
           ORDER BY bundle.bundle_key",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        let bundles = bundle_rows
            .into_iter()
            .map(|row| {
                Ok(IamPermissionBundle {
                    key: row.try_get("bundle_key").map_err(sqlx_err)?,
                    label_key: row.try_get("label_key").map_err(sqlx_err)?,
                    description_key: row.try_get("description_key").map_err(sqlx_err)?,
                    permissions: row.try_get("permissions").map_err(sqlx_err)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(IamPermissionCatalog {
            version: u64::try_from(version)
                .map_err(|_| Error::internal("negative IAM permission catalog version"))?,
            permissions,
            bundles,
        })
    }

    async fn route_catalog_version(&self) -> Result<u64> {
        let version: i64 = sqlx::query_scalar(
            "SELECT version
               FROM iam_route_catalog_versions
              WHERE catalog_key = 'routes'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        u64::try_from(version).map_err(|_| Error::internal("negative IAM route catalog version"))
    }

    async fn route_catalog(&self) -> Result<IamRouteCatalog> {
        let version: i64 = sqlx::query_scalar(
            "SELECT version
               FROM iam_route_catalog_versions
              WHERE catalog_key = 'routes'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        let rows = sqlx::query(
            "SELECT
                 route.route_key,
                 route.path_pattern,
                 route.scope,
                 route.permission_mode,
                 route.required_features,
                 route.navigation_group,
                 route.navigation_position,
                 route.enabled,
                 COALESCE(
                     array_agg(item.permission_key ORDER BY item.position)
                         FILTER (WHERE item.permission_key IS NOT NULL),
                     ARRAY[]::TEXT[]
                 ) AS permissions
               FROM iam_routes route
          LEFT JOIN iam_route_permissions item
                 ON item.route_key = route.route_key
           GROUP BY route.route_key,
                    route.path_pattern,
                    route.scope,
                    route.permission_mode,
                    route.required_features,
                    route.navigation_group,
                    route.navigation_position,
                    route.enabled
           ORDER BY route.route_key",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        let routes = rows
            .into_iter()
            .map(|row| {
                let scope: String = row.try_get("scope").map_err(sqlx_err)?;
                let permission_mode: String = row.try_get("permission_mode").map_err(sqlx_err)?;
                Ok(IamRouteDefinition {
                    id: row.try_get("route_key").map_err(sqlx_err)?,
                    path_pattern: row.try_get("path_pattern").map_err(sqlx_err)?,
                    scope: route_scope_from_str(&scope)?,
                    permission_mode: route_permission_mode_from_str(&permission_mode)?,
                    permissions: row.try_get("permissions").map_err(sqlx_err)?,
                    required_features: row.try_get("required_features").map_err(sqlx_err)?,
                    navigation_group: row.try_get("navigation_group").map_err(sqlx_err)?,
                    navigation_position: row.try_get("navigation_position").map_err(sqlx_err)?,
                    enabled: row.try_get("enabled").map_err(sqlx_err)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(IamRouteCatalog {
            version: u64::try_from(version)
                .map_err(|_| Error::internal("negative IAM route catalog version"))?,
            routes,
        })
    }

    async fn permission_scope(&self, permission_key: &str) -> Result<Option<IamPermissionScope>> {
        let scope = sqlx::query_scalar::<String>(
            "SELECT scope
               FROM iam_permissions
              WHERE permission_key = $1",
        )
        .bind(permission_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        scope
            .map(|scope| permission_scope_from_str(&scope))
            .transpose()
    }

    async fn role_permissions(&self, organization_id: &Id, role_id: &Id) -> Result<Vec<String>> {
        sqlx::query_scalar(
            "SELECT role_permission.permission_key
               FROM iam_roles role
               JOIN iam_role_permissions role_permission
                 ON role_permission.role_id = role.id
               JOIN iam_permissions permission
                 ON permission.permission_key = role_permission.permission_key
                AND (
                    (role.scope = 'platform' AND permission.scope = 'platform')
                    OR (
                        role.scope IN ('organization', 'resource')
                        AND permission.scope = 'organization'
                    )
                )
              WHERE role.org_id = $1
                AND role.id = $2
           ORDER BY role_permission.permission_key",
        )
        .bind(&organization_id.0)
        .bind(&role_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)
    }

    async fn role_summary(
        &self,
        organization_id: &Id,
        role_id: &Id,
    ) -> Result<Option<IamAssignedRole>> {
        let row = sqlx::query(
            "SELECT id, role_key, name, builtin
               FROM iam_roles
              WHERE org_id = $1
                AND id = $2",
        )
        .bind(&organization_id.0)
        .bind(&role_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row.map(|row| {
            Ok(IamAssignedRole {
                id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
                key: row.try_get("role_key").map_err(sqlx_err)?,
                name: row.try_get("name").map_err(sqlx_err)?,
                builtin: row.try_get("builtin").map_err(sqlx_err)?,
            })
        })
        .transpose()
    }

    async fn role_for_purpose(
        &self,
        organization_id: &Id,
        purpose: &str,
    ) -> Result<Option<IamAssignedRole>> {
        let row = sqlx::query(
            "SELECT role.id, role.role_key, role.name, role.builtin
               FROM iam_builtin_role_purposes purpose
               JOIN iam_roles role
                 ON role.org_id = $1
                AND role.role_key = purpose.role_key
              WHERE purpose.purpose = $2",
        )
        .bind(&organization_id.0)
        .bind(purpose)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row.map(|row| {
            Ok(IamAssignedRole {
                id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
                key: row.try_get("role_key").map_err(sqlx_err)?,
                name: row.try_get("name").map_err(sqlx_err)?,
                builtin: row.try_get("builtin").map_err(sqlx_err)?,
            })
        })
        .transpose()
    }

    async fn validate_permission_keys(
        &self,
        permission_keys: &[String],
        expected_scope: IamPermissionScope,
    ) -> Result<Vec<String>> {
        let normalized = permission_keys
            .iter()
            .map(|key| key.trim().to_ascii_lowercase())
            .collect::<BTreeSet<_>>();
        if normalized.is_empty() {
            return Ok(Vec::new());
        }
        let normalized = normalized.into_iter().collect::<Vec<_>>();
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
            let actual_scope = permission_scope_from_str(&scope)?;
            if actual_scope != expected_scope {
                return Err(Error::invalid(format!(
                    "permission {permission_key} has {scope} scope, expected {}",
                    expected_scope.as_str()
                )));
            }
            validated.push(permission_key);
        }
        if validated.len() != normalized.len() {
            let validated = validated.iter().collect::<BTreeSet<_>>();
            let unknown = normalized
                .iter()
                .find(|key| !validated.contains(key))
                .cloned()
                .unwrap_or_default();
            return Err(Error::invalid(format!("unknown permission: {unknown}")));
        }
        Ok(validated)
    }

    async fn policy_version(&self, organization_id: &Id) -> Result<u64> {
        let now = TimestampMicros::now();
        sqlx::query(
            "INSERT INTO iam_policy_versions (organization_id, version, updated_at_micros)
             VALUES ($1, 1, $2)
             ON CONFLICT (organization_id) DO NOTHING",
        )
        .bind(&organization_id.0)
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        let version: i64 = sqlx::query_scalar(
            "SELECT version FROM iam_policy_versions WHERE organization_id = $1",
        )
        .bind(&organization_id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        u64::try_from(version).map_err(|_| Error::internal("negative IAM policy version"))
    }

    async fn membership_exists(&self, organization_id: &Id, user_id: &Id) -> Result<bool> {
        sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1
                   FROM iam_memberships
                  WHERE org_id = $1
                    AND user_id = $2
             )",
        )
        .bind(&organization_id.0)
        .bind(&user_id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)
    }

    async fn active_role_bindings(
        &self,
        organization_id: &Id,
        user_id: &Id,
        now: TimestampMicros,
    ) -> Result<Vec<ResolvedIamRoleBinding>> {
        let rows = sqlx::query(
            "SELECT
                 b.id,
                 b.organization_id,
                 b.role_id,
                 b.principal_type,
                 b.principal_id,
                 b.resource_type,
                 b.resource_id,
                 b.conditions,
                 b.starts_at_micros,
                 b.expires_at_micros,
                 b.created_by,
                 b.created_at_micros,
                 r.role_key,
                 r.name AS role_name,
                 r.builtin AS role_builtin,
                 permission.permission_key
               FROM iam_role_bindings b
               JOIN iam_roles r
                 ON r.id = b.role_id
                AND r.org_id = b.organization_id
          LEFT JOIN iam_builtin_roles builtin_role
                 ON builtin_role.role_key = r.role_key
          LEFT JOIN iam_role_permissions rp
                 ON rp.role_id = r.id
          LEFT JOIN iam_permissions permission
                 ON permission.permission_key = rp.permission_key
                AND permission.scope = 'organization'
              WHERE b.organization_id = $1
                AND (b.starts_at_micros IS NULL OR b.starts_at_micros <= $3)
                AND (b.expires_at_micros IS NULL OR b.expires_at_micros > $3)
                AND (
                    (b.principal_type = 'user' AND b.principal_id = $2)
                    OR (
                        b.principal_type = 'team'
                        AND EXISTS (
                            SELECT 1
                              FROM teams t
                             WHERE t.org_id = b.organization_id
                               AND t.id = b.principal_id
                               AND t.member_ids @> jsonb_build_array($2::text)
                        )
                    )
                )
           ORDER BY
                COALESCE(builtin_role.display_priority, 1000),
                r.name,
                b.id,
                rp.permission_key",
        )
        .bind(&organization_id.0)
        .bind(&user_id.0)
        .bind(now.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let mut resolved = BTreeMap::<String, ResolvedIamRoleBinding>::new();
        for row in rows {
            let binding = row_to_binding(&row)?;
            let id = binding.id.0.clone();
            let entry = resolved
                .entry(id)
                .or_insert_with(|| ResolvedIamRoleBinding {
                    binding,
                    role_key: row.try_get("role_key").unwrap_or_default(),
                    role_name: row.try_get("role_name").unwrap_or_default(),
                    role_builtin: row.try_get("role_builtin").unwrap_or(false),
                    permissions: Vec::new(),
                });
            if let Some(permission) = row
                .try_get::<Option<String>, _>("permission_key")
                .map_err(sqlx_err)?
                && !entry.permissions.contains(&permission)
            {
                entry.permissions.push(permission);
            }
        }
        Ok(resolved.into_values().collect())
    }

    async fn list_role_bindings(&self, organization_id: &Id) -> Result<Vec<IamRoleBinding>> {
        let rows = sqlx::query(
            "SELECT id, organization_id, role_id, principal_type, principal_id,
                    resource_type, resource_id, conditions, starts_at_micros,
                    expires_at_micros, created_by, created_at_micros
               FROM iam_role_bindings
              WHERE organization_id = $1
           ORDER BY created_at_micros DESC",
        )
        .bind(&organization_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.iter().map(row_to_binding).collect()
    }

    async fn create_role_binding(&self, binding: IamRoleBinding) -> Result<(IamRoleBinding, u64)> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        sqlx::query(
            "INSERT INTO iam_role_bindings (
                 id, organization_id, role_id, principal_type, principal_id,
                 resource_type, resource_id, conditions, starts_at_micros,
                 expires_at_micros, created_by, created_at_micros
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(&binding.id.0)
        .bind(&binding.organization_id.0)
        .bind(&binding.role_id.0)
        .bind(binding.principal_type.as_str())
        .bind(&binding.principal_id.0)
        .bind(&binding.resource_type)
        .bind(&binding.resource_id)
        .bind(Json(&binding.conditions))
        .bind(binding.starts_at.map(|value| value.0))
        .bind(binding.expires_at.map(|value| value.0))
        .bind(&binding.created_by.0)
        .bind(binding.created_at.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        let version = bump_version(&mut tx, &binding.organization_id).await?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok((binding, version))
    }

    async fn delete_role_binding(&self, organization_id: &Id, binding_id: &Id) -> Result<u64> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let result = sqlx::query(
            "DELETE FROM iam_role_bindings
              WHERE organization_id = $1 AND id = $2",
        )
        .bind(&organization_id.0)
        .bind(&binding_id.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found("role binding not found"));
        }
        let version = bump_version(&mut tx, organization_id).await?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok(version)
    }

    async fn list_relationships(
        &self,
        organization_id: &Id,
    ) -> Result<Vec<IamResourceRelationship>> {
        let rows = sqlx::query(
            "SELECT id, organization_id, resource_type, resource_id, role_id,
                    subject_type, subject_id, container_type, container_id,
                    created_by, created_at_micros
               FROM iam_relationships
              WHERE organization_id = $1
           ORDER BY created_at_micros DESC",
        )
        .bind(&organization_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.iter().map(row_to_relationship).collect()
    }

    async fn matching_relationships(
        &self,
        organization_id: &Id,
        user_id: &Id,
        resource_type: &str,
        resource_id: &str,
        container_type: Option<&str>,
        container_id: Option<&str>,
    ) -> Result<Vec<ResolvedIamResourceRelationship>> {
        let rows = sqlx::query(
            "SELECT rel.id, rel.organization_id, rel.resource_type,
                    rel.resource_id, rel.role_id, rel.subject_type,
                    rel.subject_id, rel.container_type, rel.container_id,
                    rel.created_by, rel.created_at_micros, role.role_key,
                    COALESCE(
                        array_agg(role_permission.permission_key
                                  ORDER BY role_permission.permission_key)
                            FILTER (WHERE role_permission.permission_key IS NOT NULL),
                        ARRAY[]::TEXT[]
                    ) AS permissions
               FROM iam_relationships rel
               JOIN iam_roles role
                 ON role.id = rel.role_id
                AND role.org_id = rel.organization_id
          LEFT JOIN iam_role_permissions role_permission
                 ON role_permission.role_id = role.id
              WHERE rel.organization_id = $1
                AND (
                    (rel.resource_type = $3 AND rel.resource_id = $4)
                    OR (
                        $5::TEXT IS NOT NULL
                        AND $6::TEXT IS NOT NULL
                        AND rel.resource_type = $5
                        AND rel.resource_id = $6
                    )
                )
                AND (
                    (rel.subject_type = 'user' AND rel.subject_id = $2)
                    OR (
                        rel.subject_type = 'team'
                        AND EXISTS (
                            SELECT 1
                              FROM teams t
                             WHERE t.org_id = rel.organization_id
                               AND t.id = rel.subject_id
                               AND t.member_ids @> jsonb_build_array($2::text)
                        )
                    )
                )
           GROUP BY rel.id, rel.organization_id, rel.resource_type,
                    rel.resource_id, rel.role_id, rel.subject_type,
                    rel.subject_id, rel.container_type, rel.container_id,
                    rel.created_by, rel.created_at_micros, role.role_key
           ORDER BY rel.created_at_micros DESC",
        )
        .bind(&organization_id.0)
        .bind(&user_id.0)
        .bind(resource_type)
        .bind(resource_id)
        .bind(container_type)
        .bind(container_id)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.iter()
            .map(|row| {
                Ok(ResolvedIamResourceRelationship {
                    relationship: row_to_relationship(row)?,
                    role_key: row.try_get("role_key").map_err(sqlx_err)?,
                    permissions: row.try_get("permissions").map_err(sqlx_err)?,
                })
            })
            .collect()
    }

    async fn create_relationship(
        &self,
        relationship: IamResourceRelationship,
    ) -> Result<(IamResourceRelationship, u64)> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        sqlx::query(
            "INSERT INTO iam_relationships (
                 id, organization_id, resource_type, resource_id, role_id,
                 subject_type, subject_id, container_type, container_id,
                 created_by, created_at_micros
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(&relationship.id.0)
        .bind(&relationship.organization_id.0)
        .bind(&relationship.resource_type)
        .bind(&relationship.resource_id)
        .bind(&relationship.role_id.0)
        .bind(relationship.subject_type.as_str())
        .bind(&relationship.subject_id.0)
        .bind(&relationship.container_type)
        .bind(&relationship.container_id)
        .bind(&relationship.created_by.0)
        .bind(relationship.created_at.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        let version = bump_version(&mut tx, &relationship.organization_id).await?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok((relationship, version))
    }

    async fn delete_relationship(&self, organization_id: &Id, relationship_id: &Id) -> Result<u64> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let result = sqlx::query(
            "DELETE FROM iam_relationships
              WHERE organization_id = $1 AND id = $2",
        )
        .bind(&organization_id.0)
        .bind(&relationship_id.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found("relationship not found"));
        }
        let version = bump_version(&mut tx, organization_id).await?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok(version)
    }

    async fn list_cross_org_grants(&self, organization_id: &Id) -> Result<Vec<IamCrossOrgGrant>> {
        let rows = sqlx::query(
            "SELECT id, source_organization_id, target_organization_id,
                    grantee_type, grantee_id, resource_type, resource_selector,
                    permissions, conditions, starts_at_micros, expires_at_micros,
                    status, approved_by, approved_at_micros, revoked_by,
                    revoked_at_micros, created_by, created_at_micros
               FROM iam_cross_org_grants
              WHERE source_organization_id = $1 OR target_organization_id = $1
           ORDER BY created_at_micros DESC",
        )
        .bind(&organization_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.iter().map(row_to_grant).collect()
    }

    async fn matching_cross_org_grants(
        &self,
        query: &IamCrossOrgGrantQuery,
    ) -> Result<Vec<IamCrossOrgGrant>> {
        let rows = sqlx::query(
            "SELECT id, source_organization_id, target_organization_id,
                    grantee_type, grantee_id, resource_type, resource_selector,
                    permissions, conditions, starts_at_micros, expires_at_micros,
                    status, approved_by, approved_at_micros, revoked_by,
                    revoked_at_micros, created_by, created_at_micros
               FROM iam_cross_org_grants grant_row
              WHERE source_organization_id = $1
                AND target_organization_id = $2
                AND resource_type = $4
                AND status = 'active'
                AND (starts_at_micros IS NULL OR starts_at_micros <= $7)
                AND (expires_at_micros IS NULL OR expires_at_micros > $7)
                AND permissions ? $6
                AND (
                    resource_selector -> 'ids' ? $5
                    OR resource_selector ->> 'all' = 'true'
                )
                AND (
                    (grantee_type = 'user' AND grantee_id = $3)
                    OR (grantee_type = 'organization' AND grantee_id = $2)
                    OR (
                        grantee_type = 'team'
                        AND EXISTS (
                            SELECT 1
                              FROM teams t
                             WHERE t.org_id = grant_row.target_organization_id
                               AND t.id = grant_row.grantee_id
                               AND t.member_ids @> jsonb_build_array($3::text)
                        )
                    )
                )",
        )
        .bind(&query.source_organization_id.0)
        .bind(&query.target_organization_id.0)
        .bind(&query.user_id.0)
        .bind(&query.resource_type)
        .bind(&query.resource_id)
        .bind(&query.permission)
        .bind(query.now.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.iter().map(row_to_grant).collect()
    }

    async fn create_cross_org_grant(
        &self,
        grant: IamCrossOrgGrant,
    ) -> Result<(IamCrossOrgGrant, u64)> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        sqlx::query(
            "INSERT INTO iam_cross_org_grants (
                 id, source_organization_id, target_organization_id,
                 grantee_type, grantee_id, resource_type, resource_selector,
                 permissions, conditions, starts_at_micros, expires_at_micros,
                 status, approved_by, approved_at_micros, revoked_by,
                 revoked_at_micros, created_by, created_at_micros
             ) VALUES (
                 $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18
             )",
        )
        .bind(&grant.id.0)
        .bind(&grant.source_organization_id.0)
        .bind(&grant.target_organization_id.0)
        .bind(grant.grantee_type.as_str())
        .bind(&grant.grantee_id.0)
        .bind(&grant.resource_type)
        .bind(Json(&grant.resource_selector))
        .bind(Json(&grant.permissions))
        .bind(Json(&grant.conditions))
        .bind(grant.starts_at.map(|value| value.0))
        .bind(grant.expires_at.map(|value| value.0))
        .bind(grant.status.as_str())
        .bind(grant.approved_by.as_ref().map(|value| &value.0))
        .bind(grant.approved_at.map(|value| value.0))
        .bind(grant.revoked_by.as_ref().map(|value| &value.0))
        .bind(grant.revoked_at.map(|value| value.0))
        .bind(&grant.created_by.0)
        .bind(grant.created_at.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        let version = bump_version(&mut tx, &grant.source_organization_id).await?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok((grant, version))
    }

    async fn set_cross_org_grant_status(
        &self,
        organization_id: &Id,
        grant_id: &Id,
        status: IamCrossOrgGrantStatus,
        actor_id: &Id,
        now: TimestampMicros,
    ) -> Result<(IamCrossOrgGrant, u64)> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let row = match status {
            IamCrossOrgGrantStatus::Active => sqlx::query(
                "UPDATE iam_cross_org_grants
                        SET status = 'active',
                            approved_by = $3,
                            approved_at_micros = $4,
                            revoked_by = NULL,
                            revoked_at_micros = NULL
                      WHERE id = $1
                        AND target_organization_id = $2
                        AND status = 'pending'
                  RETURNING id, source_organization_id, target_organization_id,
                            grantee_type, grantee_id, resource_type, resource_selector,
                            permissions, conditions, starts_at_micros, expires_at_micros,
                            status, approved_by, approved_at_micros, revoked_by,
                            revoked_at_micros, created_by, created_at_micros",
            )
            .bind(&grant_id.0)
            .bind(&organization_id.0)
            .bind(&actor_id.0)
            .bind(now.0)
            .fetch_optional(&mut *tx)
            .await
            .map_err(sqlx_err)?,
            IamCrossOrgGrantStatus::Revoked => sqlx::query(
                "UPDATE iam_cross_org_grants
                        SET status = 'revoked',
                            revoked_by = $3,
                            revoked_at_micros = $4
                      WHERE id = $1
                        AND (source_organization_id = $2 OR target_organization_id = $2)
                        AND status <> 'revoked'
                  RETURNING id, source_organization_id, target_organization_id,
                            grantee_type, grantee_id, resource_type, resource_selector,
                            permissions, conditions, starts_at_micros, expires_at_micros,
                            status, approved_by, approved_at_micros, revoked_by,
                            revoked_at_micros, created_by, created_at_micros",
            )
            .bind(&grant_id.0)
            .bind(&organization_id.0)
            .bind(&actor_id.0)
            .bind(now.0)
            .fetch_optional(&mut *tx)
            .await
            .map_err(sqlx_err)?,
            IamCrossOrgGrantStatus::Pending => {
                return Err(Error::invalid("grant status cannot be reset to pending"));
            }
        }
        .ok_or_else(|| Error::not_found("cross-organization grant not found"))?;

        let grant = row_to_grant(&row)?;
        let source_version = bump_version(&mut tx, &grant.source_organization_id).await?;
        if grant.target_organization_id != grant.source_organization_id {
            let _ = bump_version(&mut tx, &grant.target_organization_id).await?;
        }
        tx.commit().await.map_err(sqlx_err)?;
        Ok((grant, source_version))
    }
}

fn parse_principal_type(value: &str) -> IamPrincipalType {
    match value {
        "team" => IamPrincipalType::Team,
        "group" => IamPrincipalType::Group,
        "service_account" => IamPrincipalType::ServiceAccount,
        "organization" => IamPrincipalType::Organization,
        _ => IamPrincipalType::User,
    }
}

fn parse_grant_status(value: &str) -> IamCrossOrgGrantStatus {
    match value {
        "active" => IamCrossOrgGrantStatus::Active,
        "revoked" => IamCrossOrgGrantStatus::Revoked,
        _ => IamCrossOrgGrantStatus::Pending,
    }
}

fn permission_scope_from_str(value: &str) -> Result<IamPermissionScope> {
    match value {
        "platform" => Ok(IamPermissionScope::Platform),
        "organization" => Ok(IamPermissionScope::Organization),
        other => Err(Error::internal(format!(
            "unknown IAM permission scope: {other}"
        ))),
    }
}

fn route_scope_from_str(value: &str) -> Result<IamRouteScope> {
    match value {
        "any" => Ok(IamRouteScope::Any),
        "organization" => Ok(IamRouteScope::Organization),
        "system" => Ok(IamRouteScope::System),
        "none" => Ok(IamRouteScope::None),
        other => Err(Error::internal(format!("unknown IAM route scope: {other}"))),
    }
}

fn route_permission_mode_from_str(value: &str) -> Result<IamRoutePermissionMode> {
    match value {
        "all" => Ok(IamRoutePermissionMode::All),
        "any" => Ok(IamRoutePermissionMode::Any),
        other => Err(Error::internal(format!(
            "unknown IAM route permission mode: {other}"
        ))),
    }
}

fn row_to_binding(row: &sqlx::postgres::PgRow) -> Result<IamRoleBinding> {
    Ok(IamRoleBinding {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        organization_id: Id(row.try_get("organization_id").map_err(sqlx_err)?),
        role_id: Id(row.try_get("role_id").map_err(sqlx_err)?),
        principal_type: parse_principal_type(
            &row.try_get::<String, _>("principal_type")
                .map_err(sqlx_err)?,
        ),
        principal_id: Id(row.try_get("principal_id").map_err(sqlx_err)?),
        resource_type: row.try_get("resource_type").map_err(sqlx_err)?,
        resource_id: row.try_get("resource_id").map_err(sqlx_err)?,
        conditions: row
            .try_get::<Json<Value>, _>("conditions")
            .map_err(sqlx_err)?
            .0,
        starts_at: row
            .try_get::<Option<i64>, _>("starts_at_micros")
            .map_err(sqlx_err)?
            .map(TimestampMicros),
        expires_at: row
            .try_get::<Option<i64>, _>("expires_at_micros")
            .map_err(sqlx_err)?
            .map(TimestampMicros),
        created_by: Id(row.try_get("created_by").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
    })
}

fn row_to_relationship(row: &sqlx::postgres::PgRow) -> Result<IamResourceRelationship> {
    Ok(IamResourceRelationship {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        organization_id: Id(row.try_get("organization_id").map_err(sqlx_err)?),
        resource_type: row.try_get("resource_type").map_err(sqlx_err)?,
        resource_id: row.try_get("resource_id").map_err(sqlx_err)?,
        role_id: Id(row.try_get("role_id").map_err(sqlx_err)?),
        subject_type: parse_principal_type(
            &row.try_get::<String, _>("subject_type").map_err(sqlx_err)?,
        ),
        subject_id: Id(row.try_get("subject_id").map_err(sqlx_err)?),
        container_type: row.try_get("container_type").map_err(sqlx_err)?,
        container_id: row.try_get("container_id").map_err(sqlx_err)?,
        created_by: Id(row.try_get("created_by").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
    })
}

fn row_to_grant(row: &sqlx::postgres::PgRow) -> Result<IamCrossOrgGrant> {
    Ok(IamCrossOrgGrant {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        source_organization_id: Id(row.try_get("source_organization_id").map_err(sqlx_err)?),
        target_organization_id: Id(row.try_get("target_organization_id").map_err(sqlx_err)?),
        grantee_type: parse_principal_type(
            &row.try_get::<String, _>("grantee_type").map_err(sqlx_err)?,
        ),
        grantee_id: Id(row.try_get("grantee_id").map_err(sqlx_err)?),
        resource_type: row.try_get("resource_type").map_err(sqlx_err)?,
        resource_selector: row
            .try_get::<Json<Value>, _>("resource_selector")
            .map_err(sqlx_err)?
            .0,
        permissions: row
            .try_get::<Json<Vec<String>>, _>("permissions")
            .map_err(sqlx_err)?
            .0,
        conditions: row
            .try_get::<Json<Value>, _>("conditions")
            .map_err(sqlx_err)?
            .0,
        starts_at: row
            .try_get::<Option<i64>, _>("starts_at_micros")
            .map_err(sqlx_err)?
            .map(TimestampMicros),
        expires_at: row
            .try_get::<Option<i64>, _>("expires_at_micros")
            .map_err(sqlx_err)?
            .map(TimestampMicros),
        status: parse_grant_status(&row.try_get::<String, _>("status").map_err(sqlx_err)?),
        approved_by: row
            .try_get::<Option<String>, _>("approved_by")
            .map_err(sqlx_err)?
            .map(Id),
        approved_at: row
            .try_get::<Option<i64>, _>("approved_at_micros")
            .map_err(sqlx_err)?
            .map(TimestampMicros),
        revoked_by: row
            .try_get::<Option<String>, _>("revoked_by")
            .map_err(sqlx_err)?
            .map(Id),
        revoked_at: row
            .try_get::<Option<i64>, _>("revoked_at_micros")
            .map_err(sqlx_err)?
            .map(TimestampMicros),
        created_by: Id(row.try_get("created_by").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
    })
}

async fn bump_version(tx: &mut sqlx::PgConnection, organization_id: &Id) -> Result<u64> {
    let version: i64 = sqlx::query_scalar("SELECT bump_iam_policy_version($1)")
        .bind(&organization_id.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_err)?;
    u64::try_from(version).map_err(|_| Error::internal("negative IAM policy version"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn principal_and_grant_values_match_schema() {
        assert_eq!(IamPrincipalType::ServiceAccount.as_str(), "service_account");
        assert_eq!(IamCrossOrgGrantStatus::Active.as_str(), "active");
    }
}
