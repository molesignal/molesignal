// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一次性密码重置令牌，存在 `one_time_tokens` 里（`kind = 'password_reset'`）。
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

    /// 令牌还能用。算新密码的哈希之前先问一声，随便填的令牌不必占着算哈希的名额。
    async fn is_usable(&self, token_hash: &str, now: TimestampMicros) -> Result<bool>;

    /// 原子消费令牌并更新密码，回答账号的邮箱；无效、过期或已使用时回答 `None`。
    async fn consume_and_update_password(
        &self,
        token_hash: &str,
        password_hash: &str,
        now: TimestampMicros,
    ) -> Result<Option<String>>;
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
    async fn is_usable(&self, token_hash: &str, now: TimestampMicros) -> Result<bool> {
        let row = sqlx::query(
            "SELECT 1 AS usable FROM one_time_tokens
             WHERE token_hash = $1 AND kind = 'password_reset'
               AND used_at IS NULL AND expires_at > $2",
        )
        .bind(token_hash)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(row.is_some())
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "one_time_tokens")
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
            "SELECT created_at
             FROM one_time_tokens
             WHERE kind = 'password_reset' AND user_id = $1
             ORDER BY created_at DESC
             LIMIT 1",
        )
        .bind(&user_id.0)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        if latest
            .as_ref()
            .and_then(|row| row.try_get::<TimestampMicros, _>("created_at").ok())
            .is_some_and(|created_at| now.0.saturating_sub(created_at.0) < cooldown_micros)
        {
            tx.rollback().await.map_err(sqlx_err)?;
            return Ok(false);
        }

        // 新令牌会让该用户所有尚未消费的旧令牌立即失效。
        sqlx::query(
            "UPDATE one_time_tokens
             SET used_at = $2
             WHERE kind = 'password_reset' AND user_id = $1 AND used_at IS NULL",
        )
        .bind(&user_id.0)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        let inserted = sqlx::query(
            "INSERT INTO one_time_tokens
                (token_hash, kind, user_id, created_at, expires_at)
             VALUES ($1, 'password_reset', $2, $3, $4)
             ON CONFLICT DO NOTHING",
        )
        .bind(token_hash)
        .bind(&user_id.0)
        .bind(now)
        .bind(expires_at)
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
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "one_time_tokens")
    )]
    async fn consume_and_update_password(
        &self,
        token_hash: &str,
        password_hash: &str,
        now: TimestampMicros,
    ) -> Result<Option<String>> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let row = sqlx::query(
            "SELECT prt.user_id, u.email
             FROM one_time_tokens prt
             JOIN users u ON u.id = prt.user_id
             WHERE prt.token_hash = $1
               AND prt.kind = 'password_reset'
               AND prt.used_at IS NULL
               AND prt.expires_at > $2
               AND u.disabled = FALSE
               AND u.status = 'active'
             FOR UPDATE OF prt",
        )
        .bind(token_hash)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        let Some(row) = row else {
            tx.rollback().await.map_err(sqlx_err)?;
            return Ok(None);
        };

        let user_id: String = row.try_get("user_id").map_err(sqlx_err)?;
        let email: String = row.try_get("email").map_err(sqlx_err)?;

        // 新密码生效时，用旧密码开始的登录一并作废。
        let updated = sqlx::query(
            "UPDATE users
             SET password_hash = $2,
                 sessions_valid_after = GREATEST(sessions_valid_after, $3)
             WHERE id = $1",
        )
        .bind(&user_id)
        .bind(password_hash)
        .bind(now)
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
            "UPDATE one_time_tokens
             SET used_at = $2
             WHERE kind = 'password_reset' AND user_id = $1 AND used_at IS NULL",
        )
        .bind(&user_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        // 重置链接发到了账号邮箱，邮箱归属由此得到证明：由某个单点登录服务新建、邮箱未经证明的
        // 绑定作废，那个身份源之后要再经本人确认才能登录这个账号（见 `sso_identities`）。
        sqlx::query("DELETE FROM sso_identities WHERE user_id = $1 AND proof = 'created'")
            .bind(&user_id)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(Some(email))
    }
}
