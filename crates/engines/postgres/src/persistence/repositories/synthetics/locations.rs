// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;

use super::{
    PgSyntheticRepository,
    codec::{LOCATION_COLS, row_to_location},
};
use crate::{
    domain::synthetics::{LocationScope, ProbeLocation, SyntheticLocationRepository},
    shared::{Error, Result, ids::Id},
};

#[async_trait]
impl SyntheticLocationRepository for PgSyntheticRepository {
    async fn create_location(&self, location: ProbeLocation) -> Result<ProbeLocation> {
        if (location.scope == LocationScope::Platform) != location.organization_id.is_none() {
            return Err(Error::invalid(
                "platform Locations must be global and organization Locations must be scoped",
            ));
        }
        let sql = format!(
            "INSERT INTO synthetic_probe_locations
                (id, organization_id, name, code, description, scope, execution, lifecycle,
                 health, system_managed, egress_policy, created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
             RETURNING {LOCATION_COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(&location.id.0)
            .bind(location.organization_id.as_ref().map(|id| id.as_str()))
            .bind(&location.name)
            .bind(&location.code)
            .bind(&location.description)
            .bind(location.scope.as_str())
            .bind(location.execution.as_str())
            .bind(location.lifecycle.as_str())
            .bind(location.health.as_str())
            .bind(location.system_managed)
            .bind(sqlx::types::Json(&location.egress_policy))
            .bind(location.created_at.0)
            .bind(location.updated_at.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;
        row_to_location(row)
    }

    async fn update_location(&self, location: ProbeLocation) -> Result<ProbeLocation> {
        let sql = format!(
            "UPDATE synthetic_probe_locations
             SET name = $3, description = $4, lifecycle = $5, health = $6,
                 egress_policy = $7, updated_at_micros = $8
             WHERE id = $1 AND organization_id IS NOT DISTINCT FROM $2
               AND NOT system_managed
             RETURNING {LOCATION_COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(&location.id.0)
            .bind(location.organization_id.as_ref().map(|id| id.as_str()))
            .bind(&location.name)
            .bind(&location.description)
            .bind(location.lifecycle.as_str())
            .bind(location.health.as_str())
            .bind(sqlx::types::Json(&location.egress_policy))
            .bind(location.updated_at.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;
        row_to_location(row)
    }

    async fn get_location(&self, org_id: &Id, location_id: &Id) -> Result<ProbeLocation> {
        let sql = format!(
            "SELECT {LOCATION_COLS} FROM synthetic_probe_locations
             WHERE id = $2 AND (organization_id = $1 OR organization_id IS NULL)"
        );
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&location_id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;
        row_to_location(row)
    }

    async fn list_locations(&self, org_id: &Id) -> Result<Vec<ProbeLocation>> {
        let sql = format!(
            "SELECT {LOCATION_COLS} FROM synthetic_probe_locations
             WHERE organization_id = $1 OR organization_id IS NULL
             ORDER BY scope, name, id"
        );
        sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(row_to_location)
            .collect()
    }
}
