// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 等邮箱验证的注册，存在 `one_time_tokens` 里（`kind = 'signup'`，`subject` 是邮箱）。
//!
//! 证明邮箱是本人的之前还没有账号，也不存名字和密码：本人打开链接之后才设。只存令牌摘要。同一邮箱
//! 可以有几个有效的链接，第一个用掉后其余作废，所以别人反复申请不会让本人手上的链接失效。

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use crate::{
    infra::persistence::sqlx_err,
    shared::{Result, time::TimestampMicros},
};

#[async_trait]
pub trait SignupRequestRepository: Send + Sync {
    async fn create(
        &self,
        token_hash: &str,
        email: &str,
        now: TimestampMicros,
        expires_at: TimestampMicros,
    ) -> Result<()>;
    /// 令牌还有效时，它证明的邮箱。
    async fn email_of(&self, token_hash: &str, now: TimestampMicros) -> Result<Option<String>>;
    /// 用掉令牌，回答它证明的邮箱；这个邮箱的其他链接一并作废。
    async fn take(&self, token_hash: &str, now: TimestampMicros) -> Result<Option<String>>;
}

pub struct PgSignupRequestRepository {
    pool: PgPool,
}

impl PgSignupRequestRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SignupRequestRepository for PgSignupRequestRepository {
    async fn create(
        &self,
        token_hash: &str,
        email: &str,
        now: TimestampMicros,
        expires_at: TimestampMicros,
    ) -> Result<()> {
        sqlx::query("DELETE FROM one_time_tokens WHERE kind = 'signup' AND expires_at <= $1")
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        sqlx::query(
            "INSERT INTO one_time_tokens (token_hash, kind, subject, created_at, expires_at)
             VALUES ($1, 'signup', $2, $3, $4)",
        )
        .bind(token_hash)
        .bind(email)
        .bind(now)
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn email_of(&self, token_hash: &str, now: TimestampMicros) -> Result<Option<String>> {
        sqlx::query(
            "SELECT subject AS email FROM one_time_tokens
             WHERE token_hash = $1 AND kind = 'signup' AND expires_at > $2",
        )
        .bind(token_hash)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(|row| row.try_get("email").map_err(sqlx_err))
        .transpose()
    }

    async fn take(&self, token_hash: &str, now: TimestampMicros) -> Result<Option<String>> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let email: Option<String> = sqlx::query(
            "DELETE FROM one_time_tokens
             WHERE token_hash = $1 AND kind = 'signup' AND expires_at > $2
             RETURNING subject AS email",
        )
        .bind(token_hash)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlx_err)?
        .map(|row| row.try_get("email").map_err(sqlx_err))
        .transpose()?;

        if let Some(email) = &email {
            sqlx::query("DELETE FROM one_time_tokens WHERE kind = 'signup' AND subject = $1")
                .bind(email)
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;
        }

        tx.commit().await.map_err(sqlx_err)?;
        Ok(email)
    }
}
