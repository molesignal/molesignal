// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `marketplace_subscriptions` 表 Pg 实装（spec Cloud Marketplace）。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceSubscription {
    pub id: Id,
    pub org_id: Id,
    pub provider: String, // aws | azure
    pub external_id: String,
    pub state: String, // pending | active | suspended | cancelled
    pub plan_id: Option<String>,
    pub metadata: Value,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait MarketplaceRepository: Send + Sync {
    async fn upsert_by_external(
        &self,
        s: MarketplaceSubscription,
    ) -> Result<MarketplaceSubscription>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<MarketplaceSubscription>;
    async fn find_by_external(
        &self,
        provider: &str,
        external_id: &str,
    ) -> Result<Option<MarketplaceSubscription>>;
    async fn list(&self, org_id: &Id) -> Result<Vec<MarketplaceSubscription>>;
    async fn update_state(&self, id: &Id, state: &str, at: TimestampMicros) -> Result<()>;
}

pub struct PgMarketplaceRepository {
    pool: PgPool,
}

impl PgMarketplaceRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, provider, external_id, state, plan_id, metadata,
                    created_at_micros, updated_at_micros";

fn row_to(r: sqlx::postgres::PgRow) -> MarketplaceSubscription {
    let metadata: Json<Value> = r.try_get("metadata").unwrap_or(Json(Value::Null));
    MarketplaceSubscription {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        provider: r.try_get::<String, _>("provider").unwrap_or_default(),
        external_id: r.try_get::<String, _>("external_id").unwrap_or_default(),
        state: r.try_get::<String, _>("state").unwrap_or_default(),
        plan_id: r
            .try_get::<Option<String>, _>("plan_id")
            .unwrap_or_default(),
        metadata: metadata.0,
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
        updated_at: TimestampMicros(r.try_get::<i64, _>("updated_at_micros").unwrap_or_default()),
    }
}

#[async_trait]
impl MarketplaceRepository for PgMarketplaceRepository {
    async fn upsert_by_external(
        &self,
        s: MarketplaceSubscription,
    ) -> Result<MarketplaceSubscription> {
        sqlx::query(
            "INSERT INTO marketplace_subscriptions
                (id, org_id, provider, external_id, state, plan_id, metadata,
                 created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (provider, external_id) DO UPDATE
             SET state = EXCLUDED.state,
                 plan_id = EXCLUDED.plan_id,
                 metadata = EXCLUDED.metadata,
                 updated_at_micros = EXCLUDED.updated_at_micros",
        )
        .bind(&s.id.0)
        .bind(&s.org_id.0)
        .bind(&s.provider)
        .bind(&s.external_id)
        .bind(&s.state)
        .bind(&s.plan_id)
        .bind(Json(&s.metadata))
        .bind(s.created_at.0)
        .bind(s.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(s)
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<MarketplaceSubscription> {
        let sql =
            format!("SELECT {COLS} FROM marketplace_subscriptions WHERE org_id = $1 AND id = $2");
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row_to(row))
    }

    async fn find_by_external(
        &self,
        provider: &str,
        external_id: &str,
    ) -> Result<Option<MarketplaceSubscription>> {
        let sql = format!(
            "SELECT {COLS} FROM marketplace_subscriptions
             WHERE provider = $1 AND external_id = $2"
        );
        let row = sqlx::query(&sql)
            .bind(provider)
            .bind(external_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row.map(row_to))
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<MarketplaceSubscription>> {
        let sql = format!(
            "SELECT {COLS} FROM marketplace_subscriptions
             WHERE org_id = $1 ORDER BY created_at_micros DESC"
        );
        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn update_state(&self, id: &Id, state: &str, at: TimestampMicros) -> Result<()> {
        sqlx::query(
            "UPDATE marketplace_subscriptions SET state = $2, updated_at_micros = $3 WHERE id = $1",
        )
        .bind(&id.0)
        .bind(state)
        .bind(at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }
}
