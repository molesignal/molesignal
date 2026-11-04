// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `alert_source_incidents`：外部事件在来源这一侧的状态，以及按去重键串行处理用的锁。
//!
//! 锁是一个事务里的 `pg_advisory_xact_lock`：同一个去重键的告警不管落在哪个节点上都一条一条
//! 处理；持锁的节点挂了，连接断开锁就放开。等锁最多 10 秒，等不到报错，发送方会重试。

use async_trait::async_trait;
use sqlx::{PgPool, Row, TracedTransaction, types::Json};

use super::sqlx_err;
use crate::{
    domain::alerting::source::{AlertMember, KeyLock, SourceIncident, SourceIncidentRepository},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgSourceIncidentRepository {
    pool: PgPool,
}

impl PgSourceIncidentRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to(row: sqlx::postgres::PgRow) -> Result<SourceIncident> {
    let members: Json<Vec<AlertMember>> = row.try_get("members").map_err(sqlx_err)?;

    Ok(SourceIncident {
        incident_id: Id::from_string(row.try_get::<String, _>("incident_id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        source_id: Id::from_string(row.try_get::<String, _>("source_id").map_err(sqlx_err)?),
        dedup_key: row.try_get("dedup_key").map_err(sqlx_err)?,
        last_event_at: row
            .try_get::<Option<TimestampMicros>, _>("last_event_at")
            .map_err(sqlx_err)?,
        members: members.0,
        last_intake_at: row
            .try_get::<TimestampMicros, _>("last_intake_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl SourceIncidentRepository for PgSourceIncidentRepository {
    async fn get(&self, incident_id: &Id) -> Result<Option<SourceIncident>> {
        let row = sqlx::query(
            "SELECT incident_id, org_id, source_id, dedup_key, last_event_at, members,
                    last_intake_at
               FROM alert_source_incidents
              WHERE incident_id = $1",
        )
        .bind(&incident_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row.map(row_to).transpose()
    }

    async fn upsert(&self, row: SourceIncident) -> Result<()> {
        // GREATEST 忽略 NULL：不带时间的消息不会把记着的时间清掉。
        sqlx::query(
            "INSERT INTO alert_source_incidents
                (incident_id, org_id, source_id, dedup_key, last_event_at, members,
                 last_intake_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (incident_id) DO UPDATE SET
                last_event_at = GREATEST(alert_source_incidents.last_event_at,
                                                EXCLUDED.last_event_at),
                members = EXCLUDED.members,
                last_intake_at = GREATEST(alert_source_incidents.last_intake_at,
                                                 EXCLUDED.last_intake_at)",
        )
        .bind(&row.incident_id.0)
        .bind(&row.org_id.0)
        .bind(&row.source_id.0)
        .bind(&row.dedup_key)
        .bind(row.last_event_at)
        .bind(Json(&row.members))
        .bind(row.last_intake_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn lock(&self, key: &str) -> Result<Box<dyn KeyLock>> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        sqlx::query("SET LOCAL lock_timeout = '10s'")
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;

        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(key)
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;

        Ok(Box::new(PgKeyLock { transaction }))
    }
}

/// 持着锁的事务：提交或丢弃时锁放开。
struct PgKeyLock {
    transaction: TracedTransaction<'static>,
}

#[async_trait]
impl KeyLock for PgKeyLock {
    async fn release(self: Box<Self>) -> Result<()> {
        self.transaction.commit().await.map_err(sqlx_err)
    }
}
