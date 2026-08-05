// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::{
    domain::notify::{
        preference::NotifyCategory,
        repositories::{
            NotifyRouteReferenceRepository, OrganizationNotifyDefaultRepository,
            TeamNotifyDefaultRepository,
        },
        routing::{NotifyDefaultRoute, OrganizationNotifyDefault, TeamNotifyDefault},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgTeamNotifyDefaultRepository {
    pool: PgPool,
}

impl PgTeamNotifyDefaultRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

pub struct PgOrganizationNotifyDefaultRepository {
    pool: PgPool,
}

pub struct PgNotifyRouteReferenceRepository {
    pool: PgPool,
}

impl PgNotifyRouteReferenceRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl PgOrganizationNotifyDefaultRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_team_default(row: sqlx::postgres::PgRow) -> Result<TeamNotifyDefault> {
    let category: String = row.try_get("category").map_err(sqlx_err)?;
    let routes: Json<Vec<NotifyDefaultRoute>> = row.try_get("routes").map_err(sqlx_err)?;
    Ok(TeamNotifyDefault {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        organization_id: Id::from_string(
            row.try_get::<String, _>("organization_id")
                .map_err(sqlx_err)?,
        ),
        team_id: Id::from_string(row.try_get::<String, _>("team_id").map_err(sqlx_err)?),
        category: NotifyCategory::parse(&category)?,
        routes: routes.0,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

fn row_to_organization_default(row: sqlx::postgres::PgRow) -> Result<OrganizationNotifyDefault> {
    let category: String = row.try_get("category").map_err(sqlx_err)?;
    let routes: Json<Vec<NotifyDefaultRoute>> = row.try_get("routes").map_err(sqlx_err)?;
    Ok(OrganizationNotifyDefault {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        organization_id: Id::from_string(
            row.try_get::<String, _>("organization_id")
                .map_err(sqlx_err)?,
        ),
        category: NotifyCategory::parse(&category)?,
        routes: routes.0,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl TeamNotifyDefaultRepository for PgTeamNotifyDefaultRepository {
    async fn get(
        &self,
        organization_id: &Id,
        team_id: &Id,
        category: NotifyCategory,
    ) -> Result<Option<TeamNotifyDefault>> {
        let row = sqlx::query(
            "SELECT id, organization_id, team_id, category, routes, enabled,
                    created_at_micros, updated_at_micros
               FROM team_notify_defaults
              WHERE organization_id = $1 AND team_id = $2 AND category = $3",
        )
        .bind(&organization_id.0)
        .bind(&team_id.0)
        .bind(category.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row.map(row_to_team_default).transpose()
    }

    async fn list(&self, organization_id: &Id, team_id: &Id) -> Result<Vec<TeamNotifyDefault>> {
        let rows = sqlx::query(
            "SELECT id, organization_id, team_id, category, routes, enabled,
                    created_at_micros, updated_at_micros
               FROM team_notify_defaults
              WHERE organization_id = $1 AND team_id = $2
           ORDER BY category, id",
        )
        .bind(&organization_id.0)
        .bind(&team_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_team_default).collect()
    }

    async fn upsert(&self, mut route: TeamNotifyDefault) -> Result<TeamNotifyDefault> {
        let row = sqlx::query(
            "INSERT INTO team_notify_defaults (
                 id, organization_id, team_id, category, routes, enabled,
                 created_at_micros, updated_at_micros
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (organization_id, team_id, category) DO UPDATE SET
                 routes = EXCLUDED.routes,
                 enabled = EXCLUDED.enabled,
                 updated_at_micros = EXCLUDED.updated_at_micros
             RETURNING id, created_at_micros",
        )
        .bind(&route.id.0)
        .bind(&route.organization_id.0)
        .bind(&route.team_id.0)
        .bind(route.category.as_str())
        .bind(Json(&route.routes))
        .bind(route.enabled)
        .bind(route.created_at.0)
        .bind(route.updated_at.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        route.id = Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?);
        route.created_at = TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?);
        Ok(route)
    }

    async fn delete(
        &self,
        organization_id: &Id,
        team_id: &Id,
        category: NotifyCategory,
    ) -> Result<()> {
        let deleted = sqlx::query(
            "DELETE FROM team_notify_defaults
              WHERE organization_id = $1 AND team_id = $2 AND category = $3",
        )
        .bind(&organization_id.0)
        .bind(&team_id.0)
        .bind(category.as_str())
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if deleted.rows_affected() == 0 {
            return Err(Error::not_found("team notify default"));
        }
        Ok(())
    }
}

#[async_trait]
impl OrganizationNotifyDefaultRepository for PgOrganizationNotifyDefaultRepository {
    async fn get(
        &self,
        organization_id: &Id,
        category: NotifyCategory,
    ) -> Result<Option<OrganizationNotifyDefault>> {
        let row = sqlx::query(
            "SELECT id, organization_id, category, routes, enabled,
                    created_at_micros, updated_at_micros
               FROM organization_notify_defaults
              WHERE organization_id = $1 AND category = $2",
        )
        .bind(&organization_id.0)
        .bind(category.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row.map(row_to_organization_default).transpose()
    }

    async fn list(&self, organization_id: &Id) -> Result<Vec<OrganizationNotifyDefault>> {
        let rows = sqlx::query(
            "SELECT id, organization_id, category, routes, enabled,
                    created_at_micros, updated_at_micros
               FROM organization_notify_defaults
              WHERE organization_id = $1
           ORDER BY category, id",
        )
        .bind(&organization_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_organization_default).collect()
    }

    async fn upsert(
        &self,
        mut route: OrganizationNotifyDefault,
    ) -> Result<OrganizationNotifyDefault> {
        let row = sqlx::query(
            "INSERT INTO organization_notify_defaults (
                 id, organization_id, category, routes, enabled,
                 created_at_micros, updated_at_micros
             ) VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (organization_id, category) DO UPDATE SET
                 routes = EXCLUDED.routes,
                 enabled = EXCLUDED.enabled,
                 updated_at_micros = EXCLUDED.updated_at_micros
             RETURNING id, created_at_micros",
        )
        .bind(&route.id.0)
        .bind(&route.organization_id.0)
        .bind(route.category.as_str())
        .bind(Json(&route.routes))
        .bind(route.enabled)
        .bind(route.created_at.0)
        .bind(route.updated_at.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        route.id = Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?);
        route.created_at = TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?);
        Ok(route)
    }

    async fn delete(&self, organization_id: &Id, category: NotifyCategory) -> Result<()> {
        let deleted = sqlx::query(
            "DELETE FROM organization_notify_defaults
              WHERE organization_id = $1 AND category = $2",
        )
        .bind(&organization_id.0)
        .bind(category.as_str())
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if deleted.rows_affected() == 0 {
            return Err(Error::not_found("organization notify default"));
        }
        Ok(())
    }
}

#[async_trait]
impl NotifyRouteReferenceRepository for PgNotifyRouteReferenceRepository {
    async fn count_for_connector(&self, organization_id: &Id, connector_id: &Id) -> Result<u64> {
        let count: i64 = sqlx::query_scalar(
            "SELECT
                 (
                     SELECT COUNT(*)
                       FROM team_notify_defaults AS defaults
                      WHERE defaults.organization_id = $1
                        AND EXISTS (
                            SELECT 1
                              FROM jsonb_array_elements(defaults.routes) AS route
                             WHERE route ->> 'connector_id' = $2
                        )
                 )
                 +
                 (
                     SELECT COUNT(*)
                       FROM organization_notify_defaults AS defaults
                      WHERE defaults.organization_id = $1
                        AND EXISTS (
                            SELECT 1
                              FROM jsonb_array_elements(defaults.routes) AS route
                             WHERE route ->> 'connector_id' = $2
                        )
                 )
                 +
                 (
                     SELECT COUNT(*)
                       FROM notify_policies AS policy
                      WHERE policy.organization_id = $1
                        AND (
                            EXISTS (
                                SELECT 1
                                  FROM jsonb_array_elements_text(
                                      policy.delivery_config -> 'connector_ids'
                                  ) AS configured(connector_id)
                                 WHERE configured.connector_id = $2
                            )
                            OR EXISTS (
                                SELECT 1
                                  FROM jsonb_array_elements_text(
                                      policy.escalation_config
                                          -> 'delivery_config'
                                          -> 'connector_ids'
                                  ) AS configured(connector_id)
                                 WHERE configured.connector_id = $2
                            )
                        )
                 )",
        )
        .bind(&organization_id.0)
        .bind(&connector_id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        u64::try_from(count).map_err(|_| Error::internal("negative notify route reference count"))
    }
}
