// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `org_trials` 表 Pg 实装：per-org 试用状态机持久化。

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::{
    domain::billing::{NotifyStage, Trial, TrialState},
    shared::{Result, ids::Id, time::TimestampMicros},
};

#[async_trait]
pub trait TrialRepository: Send + Sync {
    /// 读某 org 的试用记录；不存在返回 None。
    async fn get(&self, org_id: &Id) -> Result<Option<Trial>>;
    /// 创建（幂等：已存在则原样返回现有记录）。
    async fn create_if_absent(&self, trial: Trial) -> Result<Trial>;
    /// 列出仍处 `active` 状态的试用（sweeper 用）。
    async fn list_active(&self) -> Result<Vec<Trial>>;
    /// 推进状态 + 通知阶段。
    async fn update_progress(
        &self,
        org_id: &Id,
        state: TrialState,
        notified_stage: NotifyStage,
        now: TimestampMicros,
    ) -> Result<()>;
}

pub struct PgTrialRepository {
    pool: PgPool,
}

impl PgTrialRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "org_id, started_at_micros, ends_at_micros, state, notified_stage,
                    created_at_micros, updated_at_micros";

fn row_to(r: sqlx::postgres::PgRow) -> Trial {
    Trial {
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        started_at: TimestampMicros(r.try_get::<i64, _>("started_at_micros").unwrap_or_default()),
        ends_at: TimestampMicros(r.try_get::<i64, _>("ends_at_micros").unwrap_or_default()),
        state: TrialState::parse(&r.try_get::<String, _>("state").unwrap_or_default()),
        notified_stage: NotifyStage::parse(
            &r.try_get::<String, _>("notified_stage").unwrap_or_default(),
        ),
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
        updated_at: TimestampMicros(r.try_get::<i64, _>("updated_at_micros").unwrap_or_default()),
    }
}

#[async_trait]
impl TrialRepository for PgTrialRepository {
    async fn get(&self, org_id: &Id) -> Result<Option<Trial>> {
        let row = sqlx::query(&format!("SELECT {COLS} FROM org_trials WHERE org_id = $1"))
            .bind(&org_id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row.map(row_to))
    }

    async fn create_if_absent(&self, trial: Trial) -> Result<Trial> {
        sqlx::query(
            "INSERT INTO org_trials
                (org_id, started_at_micros, ends_at_micros, state, notified_stage,
                 created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (org_id) DO NOTHING",
        )
        .bind(&trial.org_id.0)
        .bind(trial.started_at.0)
        .bind(trial.ends_at.0)
        .bind(trial.state.as_str())
        .bind(trial.notified_stage.as_str())
        .bind(trial.created_at.0)
        .bind(trial.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        // 返回库里的权威记录（并发下可能是别人插入的那条）。
        Ok(self.get(&trial.org_id).await?.unwrap_or(trial))
    }

    async fn list_active(&self) -> Result<Vec<Trial>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM org_trials WHERE state = 'active' ORDER BY ends_at_micros ASC"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn update_progress(
        &self,
        org_id: &Id,
        state: TrialState,
        notified_stage: NotifyStage,
        now: TimestampMicros,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE org_trials
                SET state = $2, notified_stage = $3, updated_at_micros = $4
             WHERE org_id = $1",
        )
        .bind(&org_id.0)
        .bind(state.as_str())
        .bind(notified_stage.as_str())
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }
}
