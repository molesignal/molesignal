// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `alert_rule_eval_state` 表的 Pg 实装。
//!
//! - `upsert_match`：单档去抖，`ON CONFLICT` 累加/清零 consecutive_matches（severity_streaks 不动）。
//! - `upsert_state`：多档去抖，整行写回（含 per-severity streaks）。
//! - `reset`：incident resolve / 规则阈值变更时清零（含 severity_streaks）。

use std::collections::BTreeMap;

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::{
    domain::alerting::repositories::{AlertRuleEvalState, AlertRuleEvalStateRepository},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgAlertRuleEvalStateRepository {
    pool: PgPool,
}

impl PgAlertRuleEvalStateRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_state(row: &sqlx::postgres::PgRow) -> Result<AlertRuleEvalState> {
    let streaks: Json<BTreeMap<String, u32>> = row
        .try_get("severity_streaks")
        .unwrap_or_else(|_| Json(BTreeMap::new()));
    Ok(AlertRuleEvalState {
        rule_id: Id::from_string(row.try_get::<String, _>("rule_id").map_err(sqlx_err)?),
        consecutive_matches: row
            .try_get::<i32, _>("consecutive_matches")
            .map_err(sqlx_err)? as u32,
        last_eval_at: TimestampMicros(row.try_get("last_eval_at_micros").map_err(sqlx_err)?),
        last_matched: row.try_get("last_matched").map_err(sqlx_err)?,
        severity_streaks: streaks.0,
    })
}

#[async_trait]
impl AlertRuleEvalStateRepository for PgAlertRuleEvalStateRepository {
    async fn upsert_match(
        &self,
        rule_id: &Id,
        matched: bool,
        eval_at: TimestampMicros,
    ) -> Result<AlertRuleEvalState> {
        let row = sqlx::query(
            "INSERT INTO alert_rule_eval_state
                (rule_id, consecutive_matches, last_eval_at_micros, last_matched)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (rule_id) DO UPDATE
             SET consecutive_matches =
                    CASE WHEN EXCLUDED.last_matched
                         THEN alert_rule_eval_state.consecutive_matches + 1
                         ELSE 0 END,
                 last_eval_at_micros = EXCLUDED.last_eval_at_micros,
                 last_matched = EXCLUDED.last_matched
             RETURNING rule_id, consecutive_matches, last_eval_at_micros, last_matched, severity_streaks",
        )
        .bind(&rule_id.0)
        .bind(if matched { 1i32 } else { 0i32 })
        .bind(eval_at.0)
        .bind(matched)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row_to_state(&row)
    }

    async fn upsert_state(&self, state: AlertRuleEvalState) -> Result<AlertRuleEvalState> {
        sqlx::query(
            "INSERT INTO alert_rule_eval_state
                (rule_id, consecutive_matches, last_eval_at_micros, last_matched, severity_streaks)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (rule_id) DO UPDATE
             SET consecutive_matches = EXCLUDED.consecutive_matches,
                 last_eval_at_micros = EXCLUDED.last_eval_at_micros,
                 last_matched = EXCLUDED.last_matched,
                 severity_streaks = EXCLUDED.severity_streaks",
        )
        .bind(&state.rule_id.0)
        .bind(state.consecutive_matches as i32)
        .bind(state.last_eval_at.0)
        .bind(state.last_matched)
        .bind(Json(&state.severity_streaks))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(state)
    }

    async fn get(&self, rule_id: &Id) -> Result<Option<AlertRuleEvalState>> {
        let row = sqlx::query(
            "SELECT rule_id, consecutive_matches, last_eval_at_micros, last_matched, severity_streaks
             FROM alert_rule_eval_state
             WHERE rule_id = $1",
        )
        .bind(&rule_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        match row {
            Some(r) => Ok(Some(row_to_state(&r)?)),
            None => Ok(None),
        }
    }

    async fn reset(&self, rule_id: &Id) -> Result<()> {
        sqlx::query(
            "INSERT INTO alert_rule_eval_state
                (rule_id, consecutive_matches, last_eval_at_micros, last_matched)
             VALUES ($1, 0, $2, FALSE)
             ON CONFLICT (rule_id) DO UPDATE
             SET consecutive_matches = 0, last_matched = FALSE,
                 severity_streaks = '{}'::jsonb",
        )
        .bind(&rule_id.0)
        .bind(TimestampMicros::now().0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }
}
