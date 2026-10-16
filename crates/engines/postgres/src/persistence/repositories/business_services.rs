// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Business services: one row each, holding the dependencies, the health last
//! worked out and the periods of the last seven days.

use async_trait::async_trait;
use sqlx::{PgPool, Row, postgres::PgRow, types::Json};

use super::sqlx_err;
use crate::{
    domain::business_service::{
        BusinessService, BusinessServiceDependency, BusinessServiceHealth,
        BusinessServiceRepository, BusinessServiceStatus, HealthCause, HealthPeriod, HealthWrite,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const COLS: &str = "id, org_id, name, description, dependencies, health_status, health_causes,
    health_changed_at_micros, health_evaluated_at_micros, health_history, created_by,
    created_at_micros, updated_at_micros";

pub struct PgBusinessServiceRepository {
    pool: PgPool,
}

impl PgBusinessServiceRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl BusinessServiceRepository for PgBusinessServiceRepository {
    async fn create_business_service(&self, service: BusinessService) -> Result<BusinessService> {
        let sql = format!(
            "INSERT INTO business_services
                 (id, org_id, name, description, dependencies, created_by,
                  created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING {COLS}"
        );

        let row = sqlx::query(&sql)
            .bind(&service.id.0)
            .bind(&service.org_id.0)
            .bind(&service.name)
            .bind(&service.description)
            .bind(Json(&service.dependencies))
            .bind(service.created_by.as_ref().map(Id::as_str))
            .bind(service.created_at.0)
            .bind(service.updated_at.0)
            .fetch_one(&self.pool)
            .await
            .map_err(name_error)?;

        row_to_service(row)
    }

    async fn update_business_service(&self, service: BusinessService) -> Result<BusinessService> {
        let sql = format!(
            "UPDATE business_services
             SET name = $3, description = $4, dependencies = $5, updated_at_micros = $6
             WHERE org_id = $1 AND id = $2
             RETURNING {COLS}"
        );

        let row = sqlx::query(&sql)
            .bind(&service.org_id.0)
            .bind(&service.id.0)
            .bind(&service.name)
            .bind(&service.description)
            .bind(Json(&service.dependencies))
            .bind(service.updated_at.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(name_error)?
            .ok_or_else(|| Error::not_found("business service not found"))?;

        row_to_service(row)
    }

    async fn delete_business_service(
        &self,
        org_id: &Id,
        id: &Id,
        updated_at: TimestampMicros,
    ) -> Result<()> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        sqlx::query(
            "UPDATE status_page_components
             SET business_service_id = NULL, business_service_mode = NULL,
                 business_service_notify = TRUE, updated_at_micros = $3
             WHERE org_id = $1 AND business_service_id = $2",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(updated_at.0)
        .execute(&mut *transaction)
        .await
        .map_err(sqlx_err)?;

        sqlx::query(
            "UPDATE business_services
             SET dependencies = COALESCE((
                     SELECT jsonb_agg(dependency ORDER BY position)
                     FROM jsonb_array_elements(dependencies)
                          WITH ORDINALITY AS listed(dependency, position)
                     WHERE NOT (dependency->>'kind' = 'business_service'
                                AND dependency->>'target' = $2)
                 ), '[]'::JSONB),
                 updated_at_micros = $3
             WHERE org_id = $1
               AND dependencies @> jsonb_build_array(
                   jsonb_build_object('kind', 'business_service', 'target', $2::TEXT)
               )",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(updated_at.0)
        .execute(&mut *transaction)
        .await
        .map_err(sqlx_err)?;

        let deleted = sqlx::query("DELETE FROM business_services WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;

        if deleted.rows_affected() == 0 {
            return Err(Error::not_found("business service not found"));
        }

        transaction.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn get_business_service(&self, org_id: &Id, id: &Id) -> Result<BusinessService> {
        let sql = format!("SELECT {COLS} FROM business_services WHERE org_id = $1 AND id = $2");

        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found("business service not found"))?;

        row_to_service(row)
    }

    async fn list_business_services(&self, org_id: &Id) -> Result<Vec<BusinessService>> {
        let sql = format!(
            "SELECT {COLS} FROM business_services
             WHERE org_id = $1 ORDER BY lower(name), id LIMIT 1000"
        );

        sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
            .into_iter()
            .map(row_to_service)
            .collect()
    }

    async fn list_business_service_organizations(&self) -> Result<Vec<Id>> {
        let organizations: Vec<String> =
            sqlx::query_scalar("SELECT DISTINCT org_id FROM business_services ORDER BY org_id")
                .fetch_all(&self.pool)
                .await
                .map_err(sqlx_err)?;

        Ok(organizations.into_iter().map(Id).collect())
    }

    async fn record_business_service_health(
        &self,
        org_id: &Id,
        id: &Id,
        before: &BusinessServiceHealth,
        after: &BusinessServiceHealth,
    ) -> Result<HealthWrite> {
        let changed = after.status_changed_from(before);

        // Whatever is written, it is written only over the status and change
        // time this evaluation started from.
        let result = sqlx::query(
            "UPDATE business_services
             SET health_status = $5, health_causes = $6, health_changed_at_micros = $7,
                 health_evaluated_at_micros = $8, health_history = $9
             WHERE org_id = $1 AND id = $2
               AND health_status = $3
               AND health_changed_at_micros IS NOT DISTINCT FROM $4",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(before.status.as_str())
        .bind(before.changed_at.map(|at| at.0))
        .bind(after.status.as_str())
        .bind(Json(&after.causes))
        .bind(after.changed_at.map(|at| at.0))
        .bind(after.evaluated_at.map(|at| at.0))
        .bind(Json(&after.history))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(match (result.rows_affected(), changed) {
            (0, _) => HealthWrite::Lost,
            (_, true) => HealthWrite::Changed,
            (_, false) => HealthWrite::Unchanged,
        })
    }
}

fn row_to_service(row: PgRow) -> Result<BusinessService> {
    let status: String = row.try_get("health_status").map_err(sqlx_err)?;
    let dependencies: Json<Vec<BusinessServiceDependency>> =
        row.try_get("dependencies").map_err(sqlx_err)?;
    let causes: Json<Vec<HealthCause>> = row.try_get("health_causes").map_err(sqlx_err)?;
    let history: Json<Vec<HealthPeriod>> = row.try_get("health_history").map_err(sqlx_err)?;

    let micros = |column: &str| -> Result<Option<TimestampMicros>> {
        row.try_get::<Option<i64>, _>(column)
            .map(|value| value.map(TimestampMicros))
            .map_err(sqlx_err)
    };

    Ok(BusinessService {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        description: row.try_get("description").map_err(sqlx_err)?,
        dependencies: dependencies.0,
        health: BusinessServiceHealth {
            status: BusinessServiceStatus::parse(&status).ok_or_else(|| {
                Error::internal(format!("unknown business service health: {status}"))
            })?,
            causes: causes.0,
            changed_at: micros("health_changed_at_micros")?,
            evaluated_at: micros("health_evaluated_at_micros")?,
            history: history.0,
        },
        created_by: row
            .try_get::<Option<String>, _>("created_by")
            .map_err(sqlx_err)?
            .map(Id),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

fn name_error(error: sqlx::Error) -> Error {
    match sqlx_err(error) {
        Error::Conflict(_) => Error::conflict("a business service with this name already exists"),
        other => other,
    }
}
