// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一次性密码重置令牌。
//!
//! API 只把随机令牌的 SHA-256 摘要交给本仓库；数据库中不保存可直接使用的明文。
//! 消费令牌与更新用户密码在同一个事务中完成，避免令牌已作废但密码未更新。

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

#[async_trait]
pub trait PasswordResetRepository: Send + Sync {
    /// 为用户签发一个新令牌。
    ///
    /// 返回 `false` 表示仍在冷却窗口内，或另一个并发请求已经成功签发。
    async fn issue(
        &self,
        user_id: &Id,
        token_hash: &str,
        now: TimestampMicros,
        expires_at: TimestampMicros,
        cooldown_micros: i64,
    ) -> Result<bool>;

    /// 原子消费令牌并更新密码；无效、过期或已使用时返回 `false`。
    async fn consume_and_update_password(
        &self,
        token_hash: &str,
        password_hash: &str,
        now: TimestampMicros,
    ) -> Result<bool>;
}

pub struct PgPasswordResetRepository {
    pool: PgPool,
}

impl PgPasswordResetRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PasswordResetRepository for PgPasswordResetRepository {
    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "password_reset_tokens")
    )]
    async fn issue(
        &self,
        user_id: &Id,
        token_hash: &str,
        now: TimestampMicros,
        expires_at: TimestampMicros,
        cooldown_micros: i64,
    ) -> Result<bool> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let latest = sqlx::query(
            "SELECT created_at_micros
             FROM password_reset_tokens
             WHERE user_id = $1
             ORDER BY created_at_micros DESC
             LIMIT 1",
        )
        .bind(&user_id.0)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        if latest
            .as_ref()
            .and_then(|row| row.try_get::<i64, _>("created_at_micros").ok())
            .is_some_and(|created_at| now.0.saturating_sub(created_at) < cooldown_micros)
        {
            tx.rollback().await.map_err(sqlx_err)?;
            return Ok(false);
        }

        // 新令牌会让该用户所有尚未消费的旧令牌立即失效。
        sqlx::query(
            "UPDATE password_reset_tokens
             SET used_at_micros = $2
             WHERE user_id = $1 AND used_at_micros IS NULL",
        )
        .bind(&user_id.0)
        .bind(now.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        let inserted = sqlx::query(
            "INSERT INTO password_reset_tokens
                (id, user_id, token_hash, created_at_micros, expires_at_micros, used_at_micros)
             VALUES ($1, $2, $3, $4, $5, NULL)
             ON CONFLICT DO NOTHING",
        )
        .bind(&Id::new().0)
        .bind(&user_id.0)
        .bind(token_hash)
        .bind(now.0)
        .bind(expires_at.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?
        .rows_affected()
            == 1;

        if inserted {
            tx.commit().await.map_err(sqlx_err)?;
        } else {
            // 并发签发输掉唯一索引竞争时，保留获胜请求写入的令牌。
            tx.rollback().await.map_err(sqlx_err)?;
        }
        Ok(inserted)
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "password_reset_tokens")
    )]
    async fn consume_and_update_password(
        &self,
        token_hash: &str,
        password_hash: &str,
        now: TimestampMicros,
    ) -> Result<bool> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let row = sqlx::query(
            "SELECT prt.user_id
             FROM password_reset_tokens prt
             JOIN users u ON u.id = prt.user_id
             WHERE prt.token_hash = $1
               AND prt.used_at_micros IS NULL
               AND prt.expires_at_micros > $2
               AND u.disabled = FALSE
               AND u.status = 'active'
             FOR UPDATE OF prt",
        )
        .bind(token_hash)
        .bind(now.0)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        let Some(row) = row else {
            tx.rollback().await.map_err(sqlx_err)?;
            return Ok(false);
        };
        let user_id: String = row.try_get("user_id").map_err(sqlx_err)?;

        let updated = sqlx::query("UPDATE users SET password_hash = $2 WHERE id = $1")
            .bind(&user_id)
            .bind(password_hash)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?
            .rows_affected();
        if updated != 1 {
            tx.rollback().await.map_err(sqlx_err)?;
            return Err(Error::internal(
                "password reset token references a missing user",
            ));
        }

        // 成功重置后，同一用户的所有待用链接都失效。
        sqlx::query(
            "UPDATE password_reset_tokens
             SET used_at_micros = $2
             WHERE user_id = $1 AND used_at_micros IS NULL",
        )
        .bind(&user_id)
        .bind(now.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(true)
    }
}
