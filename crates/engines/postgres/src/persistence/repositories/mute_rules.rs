// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `mute_rules` 表 Pg 实装（告警屏蔽）。列名 `time_window`（`window` 是 PG 保留字）。

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::alerting::{
        mute::{MuteRule, MuteRuleRepository, MuteWindow},
        semantic_group::LabelMatcher,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgMuteRuleRepository {
    pool: PgPool,
}

impl PgMuteRuleRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, name, enabled, matchers, time_window, comment,
     created_by, created_at_micros, updated_at_micros";

fn row_to(row: sqlx::postgres::PgRow) -> Result<MuteRule> {
    let matchers: Json<Vec<LabelMatcher>> = row.try_get("matchers").map_err(sqlx_err)?;
    let window: Json<MuteWindow> = row.try_get("time_window").map_err(sqlx_err)?;
    Ok(MuteRule {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        matchers: matchers.0,
        window: window.0,
        comment: row.try_get("comment").map_err(sqlx_err)?,
        created_by: row
            .try_get::<Option<String>, _>("created_by")
            .map_err(sqlx_err)?
            .map(Id::from_string),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl MuteRuleRepository for PgMuteRuleRepository {
    async fn create(&self, r: MuteRule) -> Result<MuteRule> {
        sqlx::query(
            "INSERT INTO mute_rules
                (id, org_id, name, enabled, matchers, time_window, comment,
                 created_by, created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(&r.id.0)
        .bind(&r.org_id.0)
        .bind(&r.name)
        .bind(r.enabled)
        .bind(Json(&r.matchers))
        .bind(Json(&r.window))
        .bind(&r.comment)
        .bind(r.created_by.as_ref().map(|i| i.0.clone()))
        .bind(r.created_at.0)
        .bind(r.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(r)
    }

    async fn update(&self, r: MuteRule) -> Result<MuteRule> {
        sqlx::query(
            "UPDATE mute_rules SET
               name = $2, enabled = $3, matchers = $4, time_window = $5,
               comment = $6, updated_at_micros = $7
             WHERE id = $1",
        )
        .bind(&r.id.0)
        .bind(&r.name)
        .bind(r.enabled)
        .bind(Json(&r.matchers))
        .bind(Json(&r.window))
        .bind(&r.comment)
        .bind(r.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(r)
    }

    async fn get(&self, id: &Id) -> Result<MuteRule> {
        let row = sqlx::query(&format!("SELECT {COLS} FROM mute_rules WHERE id = $1"))
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        row_to(row)
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<MuteRule>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM mute_rules WHERE org_id = $1 ORDER BY created_at_micros, id"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to).collect()
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM mute_rules WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }

    async fn list_enabled(&self, org_id: &Id) -> Result<Vec<MuteRule>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM mute_rules WHERE org_id = $1 AND enabled = TRUE"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to).collect()
    }
}
