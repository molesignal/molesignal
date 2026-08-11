// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `incident_groups` 表 Pg 实装（spec incidents-grouping capability）。
//!
//! 类型与 trait 已上移到 `crate::domain::alerting::incident_group`（app 层 evaluator
//! 需要直接调用 grouping）；这里只保留 Pg 实装。

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::super::sqlx_err;
use crate::{
    domain::alerting::incident_group::{
        IncidentGroup, IncidentGroupRepository, IncidentGroupState,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgIncidentGroupRepository {
    pool: PgPool,
}

impl PgIncidentGroupRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, alert_rule_id, fingerprint, state, incident_count,
                    first_at_micros, last_at_micros, acked_by, acked_at_micros,
                    resolved_at_micros";

fn row_to(r: sqlx::postgres::PgRow) -> IncidentGroup {
    IncidentGroup {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        alert_rule_id: Id(r.try_get::<String, _>("alert_rule_id").unwrap_or_default()),
        fingerprint: r.try_get::<String, _>("fingerprint").unwrap_or_default(),
        state: IncidentGroupState::parse(&r.try_get::<String, _>("state").unwrap_or_default()),
        incident_count: r.try_get::<i32, _>("incident_count").unwrap_or(1),
        first_at: TimestampMicros(r.try_get::<i64, _>("first_at_micros").unwrap_or_default()),
        last_at: TimestampMicros(r.try_get::<i64, _>("last_at_micros").unwrap_or_default()),
        acked_by: r
            .try_get::<Option<String>, _>("acked_by")
            .unwrap_or_default()
            .map(Id),
        acked_at: r
            .try_get::<Option<i64>, _>("acked_at_micros")
            .unwrap_or_default()
            .map(TimestampMicros),
        resolved_at: r
            .try_get::<Option<i64>, _>("resolved_at_micros")
            .unwrap_or_default()
            .map(TimestampMicros),
    }
}

#[async_trait]
impl IncidentGroupRepository for PgIncidentGroupRepository {
    async fn upsert_for_incident(
        &self,
        org_id: &Id,
        alert_rule_id: &Id,
        fingerprint: &str,
        at: TimestampMicros,
        window_secs: i64,
    ) -> Result<IncidentGroup> {
        let cutoff = at.0 - window_secs * 1_000_000;
        // 找最近一个同 rule+fp 的 group，且 last_at 在窗口内、state 非 resolved
        let existing_sql = format!(
            "SELECT {COLS} FROM incident_groups
             WHERE org_id = $1 AND alert_rule_id = $2 AND fingerprint = $3
                   AND last_at_micros >= $4 AND state != 'resolved'
             ORDER BY last_at_micros DESC LIMIT 1"
        );
        let existing = sqlx::query(&existing_sql)
            .bind(&org_id.0)
            .bind(&alert_rule_id.0)
            .bind(fingerprint)
            .bind(cutoff)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?;
        if let Some(row) = existing {
            let group = row_to(row);
            sqlx::query(
                "UPDATE incident_groups
                 SET last_at_micros = $2, incident_count = incident_count + 1
                 WHERE id = $1",
            )
            .bind(&group.id.0)
            .bind(at.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
            return Ok(IncidentGroup {
                last_at: at,
                incident_count: group.incident_count + 1,
                ..group
            });
        }
        // 新建
        let g = IncidentGroup {
            id: Id::new(),
            org_id: org_id.clone(),
            alert_rule_id: alert_rule_id.clone(),
            fingerprint: fingerprint.to_string(),
            state: IncidentGroupState::Open,
            incident_count: 1,
            first_at: at,
            last_at: at,
            acked_by: None,
            acked_at: None,
            resolved_at: None,
        };
        sqlx::query(
            "INSERT INTO incident_groups
                (id, org_id, alert_rule_id, fingerprint, state, incident_count,
                 first_at_micros, last_at_micros)
             VALUES ($1, $2, $3, $4, 'open', 1, $5, $5)",
        )
        .bind(&g.id.0)
        .bind(&g.org_id.0)
        .bind(&g.alert_rule_id.0)
        .bind(&g.fingerprint)
        .bind(g.first_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(g)
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<IncidentGroup> {
        let sql = format!("SELECT {COLS} FROM incident_groups WHERE org_id = $1 AND id = $2");
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row_to(row))
    }

    async fn list(
        &self,
        org_id: &Id,
        state_filter: Option<IncidentGroupState>,
        limit: i64,
    ) -> Result<Vec<IncidentGroup>> {
        let sql = format!(
            "SELECT {COLS} FROM incident_groups
             WHERE org_id = $1 AND ($2::TEXT IS NULL OR state = $2)
             ORDER BY last_at_micros DESC LIMIT $3"
        );
        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(state_filter.map(|s| s.as_str()))
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn ack(&self, org_id: &Id, id: &Id, acked_by: &Id, at: TimestampMicros) -> Result<()> {
        sqlx::query(
            "UPDATE incident_groups
             SET state = 'acked', acked_by = $3, acked_at_micros = $4
             WHERE org_id = $1 AND id = $2 AND state = 'open'",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(&acked_by.0)
        .bind(at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn resolve(&self, org_id: &Id, id: &Id, at: TimestampMicros) -> Result<()> {
        sqlx::query(
            "UPDATE incident_groups
             SET state = 'resolved', resolved_at_micros = $3
             WHERE org_id = $1 AND id = $2 AND state != 'resolved'",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }
}
