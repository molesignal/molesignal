// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `alert_rule_eval_state` 表的 Pg 实装。一条规则一行（`group_key` 为空）；按列拆分的规则另有每组
//! 一行，各自去抖，并记着这一组的取值。
//!
//! - `claim_cycle`：认领一轮评估，同一个评估周期只放行一个节点（成员变化时两个节点可能同时拥有一条规则）。
//!   只用规则那一行。
//! - `upsert_state`：一轮评估的计数在评估器里算好，整行写回。
//! - `reset`：incident resolve / 规则阈值变更时清零（含 severity_streaks；预测规则的序列留着）。
//! - `list_groups` / `forget_groups`：按列拆分的规则记着哪些组，以及忘掉不再需要的组。

use std::collections::BTreeMap;

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::{
    domain::alerting::{
        forecast::ForecastSeries,
        repositories::{AlertRuleEvalState, AlertRuleEvalStateRepository},
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

const COLS: &str = "rule_id, group_key, group_labels, consecutive_matches, consecutive_clears,
     consecutive_no_data, no_data_since, last_eval_at, last_matched,
     severity_streaks, series";

pub struct PgAlertRuleEvalStateRepository {
    pool: PgPool,
}

impl PgAlertRuleEvalStateRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn count(row: &sqlx::postgres::PgRow, column: &str) -> Result<u32> {
    let value: i32 = row.try_get(column).map_err(sqlx_err)?;
    Ok(u32::try_from(value).unwrap_or_default())
}

fn row_to_state(row: &sqlx::postgres::PgRow) -> Result<AlertRuleEvalState> {
    let streaks: Json<BTreeMap<String, u32>> = row
        .try_get("severity_streaks")
        .unwrap_or_else(|_| Json(BTreeMap::new()));

    let labels: Json<BTreeMap<String, String>> = row
        .try_get("group_labels")
        .unwrap_or_else(|_| Json(BTreeMap::new()));

    let series: Json<ForecastSeries> = row
        .try_get("series")
        .unwrap_or_else(|_| Json(ForecastSeries::default()));

    let no_data_since: Option<i64> = row
        .try_get::<Option<TimestampMicros>, _>("no_data_since")
        .map_err(sqlx_err)?
        .map(|t| t.0);

    Ok(AlertRuleEvalState {
        rule_id: Id::from_string(row.try_get::<String, _>("rule_id").map_err(sqlx_err)?),
        group_key: row.try_get("group_key").map_err(sqlx_err)?,
        group_labels: labels.0,
        consecutive_matches: count(row, "consecutive_matches")?,
        consecutive_clears: count(row, "consecutive_clears")?,
        consecutive_no_data: count(row, "consecutive_no_data")?,
        no_data_since: no_data_since.map(TimestampMicros),
        last_eval_at: row
            .try_get::<TimestampMicros, _>("last_eval_at")
            .map_err(sqlx_err)?,
        last_matched: row.try_get("last_matched").map_err(sqlx_err)?,
        severity_streaks: streaks.0,
        series: series.0,
    })
}

fn column(count: u32) -> i32 {
    i32::try_from(count).unwrap_or(i32::MAX)
}

#[async_trait]
impl AlertRuleEvalStateRepository for PgAlertRuleEvalStateRepository {
    async fn claim_cycle(
        &self,
        rule_id: &Id,
        eval_at: TimestampMicros,
        min_gap_micros: i64,
    ) -> Result<bool> {
        // 单条语句里完成「检查 + 推进」：并发的两个认领里只有一个能满足 WHERE。
        let claimed = sqlx::query(
            "INSERT INTO alert_rule_eval_state
                (rule_id, group_key, consecutive_matches, last_eval_at, last_matched)
             VALUES ($1, '', 0, $2, FALSE)
             ON CONFLICT (rule_id, group_key) DO UPDATE
             SET last_eval_at = EXCLUDED.last_eval_at
             WHERE alert_rule_eval_state.last_eval_at
                   <= EXCLUDED.last_eval_at - $3 * INTERVAL '1 microsecond'",
        )
        .bind(&rule_id.0)
        .bind(eval_at)
        .bind(min_gap_micros)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

        Ok(claimed == 1)
    }

    async fn upsert_state(&self, state: AlertRuleEvalState) -> Result<AlertRuleEvalState> {
        sqlx::query(
            "INSERT INTO alert_rule_eval_state
                (rule_id, group_key, group_labels, consecutive_matches, consecutive_clears,
                 consecutive_no_data, no_data_since, last_eval_at, last_matched,
                 severity_streaks, series)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (rule_id, group_key) DO UPDATE
             SET group_labels = EXCLUDED.group_labels,
                 consecutive_matches = EXCLUDED.consecutive_matches,
                 consecutive_clears = EXCLUDED.consecutive_clears,
                 consecutive_no_data = EXCLUDED.consecutive_no_data,
                 no_data_since = EXCLUDED.no_data_since,
                 last_eval_at = GREATEST(
                    alert_rule_eval_state.last_eval_at,
                    EXCLUDED.last_eval_at),
                 last_matched = EXCLUDED.last_matched,
                 severity_streaks = EXCLUDED.severity_streaks,
                 series = EXCLUDED.series",
        )
        .bind(&state.rule_id.0)
        .bind(&state.group_key)
        .bind(Json(&state.group_labels))
        .bind(column(state.consecutive_matches))
        .bind(column(state.consecutive_clears))
        .bind(column(state.consecutive_no_data))
        .bind(state.no_data_since)
        .bind(state.last_eval_at)
        .bind(state.last_matched)
        .bind(Json(&state.severity_streaks))
        .bind(Json(&state.series))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(state)
    }

    async fn get(&self, rule_id: &Id, group_key: &str) -> Result<Option<AlertRuleEvalState>> {
        let row = sqlx::query(&format!(
            "SELECT {COLS}
             FROM alert_rule_eval_state
             WHERE rule_id = $1 AND group_key = $2"
        ))
        .bind(&rule_id.0)
        .bind(group_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row.as_ref().map(row_to_state).transpose()
    }

    async fn list_groups(&self, rule_id: &Id) -> Result<Vec<AlertRuleEvalState>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS}
             FROM alert_rule_eval_state
             WHERE rule_id = $1 AND group_key <> ''"
        ))
        .bind(&rule_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter().map(row_to_state).collect()
    }

    async fn reset(&self, rule_id: &Id, group_key: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO alert_rule_eval_state
                (rule_id, group_key, consecutive_matches, last_eval_at, last_matched)
             VALUES ($1, $2, 0, $3, FALSE)
             ON CONFLICT (rule_id, group_key) DO UPDATE
             SET consecutive_matches = 0, last_matched = FALSE,
                 severity_streaks = '{}'::jsonb",
        )
        .bind(&rule_id.0)
        .bind(group_key)
        .bind(TimestampMicros::now())
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn forget_groups(&self, rule_id: &Id, group_keys: &[String]) -> Result<()> {
        if group_keys.is_empty() {
            return Ok(());
        }

        sqlx::query(
            "DELETE FROM alert_rule_eval_state
              WHERE rule_id = $1
                AND group_key <> ''
                AND group_key = ANY($2)",
        )
        .bind(&rule_id.0)
        .bind(group_keys)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }
}
