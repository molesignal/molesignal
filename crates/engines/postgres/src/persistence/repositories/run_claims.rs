// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `run_claims` 表的 Pg 实装：后台工作单元（报告、pipeline、connector）的运行租约。
//!
//! 认领是一条 `INSERT .. ON CONFLICT DO UPDATE .. WHERE`：并发的两个认领里只有一个满足
//! WHERE，不需要额外加锁。持有者崩溃后租约到期，别的节点即可接手重跑。

use async_trait::async_trait;
use sqlx::PgPool;

use super::sqlx_err;
use crate::{
    domain::run_claim::RunClaimRepository,
    shared::{Result, time::TimestampMicros},
};

pub struct PgRunClaimRepository {
    pool: PgPool,
}

impl PgRunClaimRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RunClaimRepository for PgRunClaimRepository {
    async fn try_claim(
        &self,
        kind: &str,
        key: &str,
        holder: &str,
        now: TimestampMicros,
        until: TimestampMicros,
    ) -> Result<bool> {
        let claimed = sqlx::query(
            "INSERT INTO run_claims (kind, claim_key, holder, claimed_until_micros)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (kind, claim_key) DO UPDATE
             SET holder = EXCLUDED.holder,
                 claimed_until_micros = EXCLUDED.claimed_until_micros
             WHERE run_claims.claimed_until_micros <= $5
                OR run_claims.holder = EXCLUDED.holder",
        )
        .bind(kind)
        .bind(key)
        .bind(holder)
        .bind(until.0)
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();
        Ok(claimed == 1)
    }

    async fn release(&self, kind: &str, key: &str, holder: &str) -> Result<()> {
        sqlx::query("DELETE FROM run_claims WHERE kind = $1 AND claim_key = $2 AND holder = $3")
            .bind(kind)
            .bind(key)
            .bind(holder)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }
}
