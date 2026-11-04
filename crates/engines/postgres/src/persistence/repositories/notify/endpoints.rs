// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::{
    domain::notify::{endpoint::UserNotifyEndpoint, repositories::UserNotifyEndpointRepository},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgUserNotifyEndpointRepository {
    pool: PgPool,
}

impl PgUserNotifyEndpointRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, organization_id, user_id, connector_id, provider_type,
    external_identity, display_name, metadata, verified, enabled,
    created_at, updated_at";

fn row_to_endpoint(row: sqlx::postgres::PgRow) -> Result<UserNotifyEndpoint> {
    let metadata: Json<serde_json::Value> = row.try_get("metadata").map_err(sqlx_err)?;

    Ok(UserNotifyEndpoint {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        organization_id: Id::from_string(
            row.try_get::<String, _>("organization_id")
                .map_err(sqlx_err)?,
        ),
        user_id: Id::from_string(row.try_get::<String, _>("user_id").map_err(sqlx_err)?),
        connector_id: Id::from_string(row.try_get::<String, _>("connector_id").map_err(sqlx_err)?),
        provider_type: row.try_get("provider_type").map_err(sqlx_err)?,
        external_identity: row.try_get("external_identity").map_err(sqlx_err)?,
        display_name: row.try_get("display_name").map_err(sqlx_err)?,
        metadata: metadata.0,
        verified: row.try_get("verified").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl UserNotifyEndpointRepository for PgUserNotifyEndpointRepository {
    async fn create(&self, endpoint: UserNotifyEndpoint) -> Result<UserNotifyEndpoint> {
        sqlx::query(
            "INSERT INTO user_notify_endpoints (
                 id, organization_id, user_id, connector_id, provider_type,
                 external_identity, display_name, metadata, verified, enabled,
                 created_at, updated_at
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(&endpoint.id.0)
        .bind(&endpoint.organization_id.0)
        .bind(&endpoint.user_id.0)
        .bind(&endpoint.connector_id.0)
        .bind(&endpoint.provider_type)
        .bind(&endpoint.external_identity)
        .bind(&endpoint.display_name)
        .bind(Json(&endpoint.metadata))
        .bind(endpoint.verified)
        .bind(endpoint.enabled)
        .bind(endpoint.created_at)
        .bind(endpoint.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(endpoint)
    }

    async fn update(&self, endpoint: UserNotifyEndpoint) -> Result<UserNotifyEndpoint> {
        let updated = sqlx::query(
            "UPDATE user_notify_endpoints
                SET connector_id = $4,
                    provider_type = $5,
                    external_identity = $6,
                    display_name = $7,
                    metadata = $8,
                    verified = $9,
                    enabled = $10,
                    updated_at = $11
              WHERE organization_id = $1 AND user_id = $2 AND id = $3",
        )
        .bind(&endpoint.organization_id.0)
        .bind(&endpoint.user_id.0)
        .bind(&endpoint.id.0)
        .bind(&endpoint.connector_id.0)
        .bind(&endpoint.provider_type)
        .bind(&endpoint.external_identity)
        .bind(&endpoint.display_name)
        .bind(Json(&endpoint.metadata))
        .bind(endpoint.verified)
        .bind(endpoint.enabled)
        .bind(endpoint.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if updated.rows_affected() == 0 {
            return Err(Error::not_found("user notify endpoint"));
        }

        Ok(endpoint)
    }

    async fn get(&self, organization_id: &Id, user_id: &Id, id: &Id) -> Result<UserNotifyEndpoint> {
        let row = sqlx::query(&format!(
            "SELECT {COLS}
               FROM user_notify_endpoints
              WHERE organization_id = $1 AND user_id = $2 AND id = $3"
        ))
        .bind(&organization_id.0)
        .bind(&user_id.0)
        .bind(&id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row_to_endpoint(row)
    }

    async fn list(&self, organization_id: &Id, user_id: &Id) -> Result<Vec<UserNotifyEndpoint>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS}
               FROM user_notify_endpoints
              WHERE organization_id = $1 AND user_id = $2
           ORDER BY display_name NULLS LAST, id"
        ))
        .bind(&organization_id.0)
        .bind(&user_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to_endpoint).collect()
    }

    async fn list_for_organization(&self, organization_id: &Id) -> Result<Vec<UserNotifyEndpoint>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS}
               FROM user_notify_endpoints
              WHERE organization_id = $1
           ORDER BY user_id, display_name NULLS LAST, id"
        ))
        .bind(&organization_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to_endpoint).collect()
    }

    async fn count_for_connector(&self, organization_id: &Id, connector_id: &Id) -> Result<u64> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
               FROM user_notify_endpoints
              WHERE organization_id = $1 AND connector_id = $2",
        )
        .bind(&organization_id.0)
        .bind(&connector_id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        u64::try_from(count).map_err(|_| Error::internal("negative endpoint count"))
    }

    async fn delete(&self, organization_id: &Id, user_id: &Id, id: &Id) -> Result<()> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        // 先锁住这一行：正在保存的接收顺序用到它时，等那次保存完再检查引用。
        let locked = sqlx::query(
            "SELECT id FROM user_notify_endpoints
              WHERE organization_id = $1 AND user_id = $2 AND id = $3
                FOR UPDATE",
        )
        .bind(&organization_id.0)
        .bind(&user_id.0)
        .bind(&id.0)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        if locked.is_none() {
            return Err(Error::not_found("user notify endpoint"));
        }

        let referenced: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1
                   FROM user_notify_preferences preference,
                        jsonb_array_elements(preference.steps) AS step
                  WHERE preference.organization_id = $1
                    AND preference.user_id = $2
                    AND step->>'endpoint_id' = $3
             )",
        )
        .bind(&organization_id.0)
        .bind(&user_id.0)
        .bind(&id.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        if referenced {
            return Err(Error::conflict(
                "user notify endpoint is referenced by a notify preference",
            ));
        }

        sqlx::query("DELETE FROM user_notify_endpoints WHERE id = $1")
            .bind(&id.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }
}
