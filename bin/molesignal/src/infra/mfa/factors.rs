// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 账号已开启的第二因素：身份验证器和恢复码，存在 `users_ext`。Passkey 在 [`super::passkeys`]；
//! 账号有身份验证器或 Passkey 就算开着多因素认证。
//!
//! 身份验证器的密钥用根密钥加密存放，只在核对验证码时解开；恢复码只存摘要。开启、换掉、关闭
//! 都在一个事务里完成，并让账号此前开始的登录作废（`users.sessions_valid_after`）。

use std::collections::HashSet;

use async_trait::async_trait;
use sqlx::{PgPool, Row, TracedTransaction};

use super::lockouts;
use crate::{
    infra::{cipher::CipherRootKey, persistence::sqlx_err},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 关掉身份验证器。
const CLEAR_TOTP: &str = "UPDATE users_ext
     SET totp_secret_nonce = NULL, totp_secret_ciphertext = NULL,
         totp_last_used_step = 0, totp_created_at = NULL
     WHERE user_id = $1 AND totp_secret_ciphertext IS NOT NULL";

/// 已开启的身份验证器。
pub struct TotpFactor {
    /// 解开后的密钥。
    pub secret: Vec<u8>,
    /// 最后用掉的时间步。
    pub last_used_step: i64,
    pub created_at: TimestampMicros,
}

/// 开启或换掉身份验证器。
pub struct NewTotp<'a> {
    pub secret: &'a [u8],
    /// 确认时用掉的时间步：确认用的验证码不能再拿来登录。
    pub confirmed_step: i64,
    /// 新的一组恢复码的摘要；为空时保留原来的恢复码。
    pub recovery_code_hashes: &'a [String],
    pub at: TimestampMicros,
    /// 账号在这之前开始的登录作废。开启多因素认证、换验证器时才有；已经用着 Passkey 再加一个验证器
    /// 不影响别的登录。
    pub sessions_end: Option<TimestampMicros>,
}

#[async_trait]
pub trait MfaFactorRepository: Send + Sync {
    async fn totp(&self, user_id: &Id) -> Result<Option<TotpFactor>>;
    async fn save_totp(&self, user_id: &Id, totp: NewTotp<'_>) -> Result<()>;
    /// 用掉 `step` 这个时间步。只有比上次用掉的晚才算数，多个节点同时收到同一个验证码时只有一个
    /// 能用上；返回是否用上了。
    async fn use_totp_step(&self, user_id: &Id, step: i64) -> Result<bool>;
    /// 关掉身份验证器，账号在 `sessions_end` 之前开始的登录作废。还有 Passkey 时多因素认证仍开着，
    /// 恢复码保留；没有了就一并删掉恢复码。原来没开身份验证器时返回 `None`，否则返回多因素认证是否
    /// 仍开着。
    async fn remove_totp(
        &self,
        user_id: &Id,
        sessions_end: TimestampMicros,
    ) -> Result<Option<bool>>;
    /// 关掉多因素认证：删掉身份验证器、Passkey、恢复码和等着验证的请求，账号在 `sessions_end` 之前
    /// 开始的登录作废；返回原来是否开着。
    async fn remove_all(&self, user_id: &Id, sessions_end: TimestampMicros) -> Result<bool>;
    /// 整组换掉恢复码。
    async fn replace_recovery_codes(
        &self,
        user_id: &Id,
        code_hashes: &[String],
        at: TimestampMicros,
    ) -> Result<()>;
    /// 用掉一个恢复码：没用过才算数，返回还剩几个；不是这个账号的有效恢复码时返回 `None`。
    async fn use_recovery_code(&self, user_id: &Id, code_hash: &str) -> Result<Option<i64>>;
    async fn recovery_codes_left(&self, user_id: &Id) -> Result<i64>;
    /// 这些账号里开了多因素认证的。
    async fn enabled_among(&self, user_ids: &[Id]) -> Result<HashSet<Id>>;
    /// 让账号在 `sessions_end` 之前开始的登录作废。
    async fn end_sessions(&self, user_id: &Id, sessions_end: TimestampMicros) -> Result<()>;
}

pub struct PgMfaFactorRepository {
    pool: PgPool,
    cipher: CipherRootKey,
}

impl PgMfaFactorRepository {
    pub fn new(pool: PgPool, cipher: CipherRootKey) -> Self {
        Self { pool, cipher }
    }
}

pub(super) async fn end_sessions_in(
    tx: &mut TracedTransaction<'_>,
    user_id: &Id,
    sessions_end: TimestampMicros,
) -> Result<()> {
    sqlx::query(
        "UPDATE users
         SET sessions_valid_after = GREATEST(sessions_valid_after, $2)
         WHERE id = $1",
    )
    .bind(&user_id.0)
    .bind(sessions_end)
    .execute(&mut **tx)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

/// 删掉账号的身份验证器、Passkey、恢复码、输错计数和等着验证的请求；返回原来是否开着多因素认证。
async fn remove_all_in(
    tx: &mut TracedTransaction<'_>,
    user_id: &Id,
    sessions_end: TimestampMicros,
) -> Result<bool> {
    let mut removed = 0;

    for statement in [CLEAR_TOTP, "DELETE FROM user_passkeys WHERE user_id = $1"] {
        removed += sqlx::query(statement)
            .bind(&user_id.0)
            .execute(&mut **tx)
            .await
            .map_err(sqlx_err)?
            .rows_affected();
    }

    sqlx::query("DELETE FROM one_time_tokens WHERE kind = 'mfa_challenge' AND user_id = $1")
        .bind(&user_id.0)
        .execute(&mut **tx)
        .await
        .map_err(sqlx_err)?;

    forget_codes_and_failures(tx, user_id).await?;
    end_sessions_in(tx, user_id, sessions_end).await?;
    Ok(removed > 0)
}

/// 删掉恢复码和第二步验证输错的次数。
async fn forget_codes_and_failures(tx: &mut TracedTransaction<'_>, user_id: &Id) -> Result<()> {
    sqlx::query(
        "UPDATE users_ext
         SET recovery_code_hashes = ARRAY[]::TEXT[], recovery_codes_created_at = NULL
         WHERE user_id = $1",
    )
    .bind(&user_id.0)
    .execute(&mut **tx)
    .await
    .map_err(sqlx_err)?;

    sqlx::query("DELETE FROM signin_attempts WHERE account_hash = $1 AND source_hash = ''")
        .bind(lockouts::row_key(user_id))
        .execute(&mut **tx)
        .await
        .map_err(sqlx_err)?;

    Ok(())
}

/// 去掉一个因子之后：账号还有身份验证器或 Passkey 就返回 `true`；一个都没有了，多因素认证随之
/// 关闭，恢复码和输错计数一并删掉，返回 `false`。
pub(super) async fn still_on_in(tx: &mut TracedTransaction<'_>, user_id: &Id) -> Result<bool> {
    let on: bool = sqlx::query(
        "SELECT EXISTS (
                    SELECT 1 FROM users_ext
                    WHERE user_id = $1 AND totp_secret_ciphertext IS NOT NULL
                )
             OR EXISTS (SELECT 1 FROM user_passkeys WHERE user_id = $1) AS still_on",
    )
    .bind(&user_id.0)
    .fetch_one(&mut **tx)
    .await
    .map_err(sqlx_err)?
    .try_get("still_on")
    .map_err(sqlx_err)?;

    if !on {
        forget_codes_and_failures(tx, user_id).await?;
    }

    Ok(on)
}

/// 在服务器上离线重置：按邮箱找到账号，关掉它的多因素认证，所有登录作废。给 root 丢了验证器、
/// 或者所有管理员都进不来时用。找不到账号时返回 `None`，否则返回原来是否开着。
pub async fn reset_by_email(pool: &PgPool, email: &str) -> Result<Option<bool>> {
    let row = sqlx::query("SELECT id FROM users WHERE lower(email) = lower($1)")
        .bind(email.trim())
        .fetch_optional(pool)
        .await
        .map_err(sqlx_err)?;

    let Some(row) = row else {
        return Ok(None);
    };

    let user_id = Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?);
    let mut tx = sqlx::begin(pool).await.map_err(sqlx_err)?;
    let removed = remove_all_in(&mut tx, &user_id, TimestampMicros::now()).await?;
    tx.commit().await.map_err(sqlx_err)?;

    Ok(Some(removed))
}

pub(super) async fn replace_codes_in(
    tx: &mut TracedTransaction<'_>,
    user_id: &Id,
    code_hashes: &[String],
    at: TimestampMicros,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO users_ext (user_id, recovery_code_hashes, recovery_codes_created_at)
         VALUES ($1, $2, $3)
         ON CONFLICT (user_id) DO UPDATE SET
            recovery_code_hashes = EXCLUDED.recovery_code_hashes,
            recovery_codes_created_at = EXCLUDED.recovery_codes_created_at",
    )
    .bind(&user_id.0)
    .bind(code_hashes)
    .bind(at)
    .execute(&mut **tx)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

#[async_trait]
impl MfaFactorRepository for PgMfaFactorRepository {
    async fn totp(&self, user_id: &Id) -> Result<Option<TotpFactor>> {
        let row = sqlx::query(
            "SELECT totp_secret_nonce, totp_secret_ciphertext, totp_last_used_step,
                    totp_created_at
             FROM users_ext WHERE user_id = $1 AND totp_secret_ciphertext IS NOT NULL",
        )
        .bind(&user_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let Some(row) = row else {
            return Ok(None);
        };

        let nonce: Vec<u8> = row.try_get("totp_secret_nonce").map_err(sqlx_err)?;
        let ciphertext: Vec<u8> = row.try_get("totp_secret_ciphertext").map_err(sqlx_err)?;

        let secret = self.cipher.open(&nonce, &ciphertext).map_err(|error| {
            Error::internal(format!("authenticator secret cannot be opened: {error}"))
        })?;

        Ok(Some(TotpFactor {
            secret,
            last_used_step: row.try_get("totp_last_used_step").map_err(sqlx_err)?,
            created_at: row.try_get("totp_created_at").map_err(sqlx_err)?,
        }))
    }

    async fn save_totp(&self, user_id: &Id, totp: NewTotp<'_>) -> Result<()> {
        let (nonce, ciphertext) = self.cipher.seal(totp.secret).map_err(|error| {
            Error::internal(format!("authenticator secret cannot be sealed: {error}"))
        })?;

        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        sqlx::query(
            "INSERT INTO users_ext
                (user_id, totp_secret_nonce, totp_secret_ciphertext, totp_last_used_step,
                 totp_created_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (user_id) DO UPDATE SET
                totp_secret_nonce = EXCLUDED.totp_secret_nonce,
                totp_secret_ciphertext = EXCLUDED.totp_secret_ciphertext,
                totp_last_used_step = EXCLUDED.totp_last_used_step,
                totp_created_at = EXCLUDED.totp_created_at",
        )
        .bind(&user_id.0)
        .bind(nonce)
        .bind(ciphertext)
        .bind(totp.confirmed_step)
        .bind(totp.at)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        if !totp.recovery_code_hashes.is_empty() {
            replace_codes_in(&mut tx, user_id, totp.recovery_code_hashes, totp.at).await?;
        }

        if let Some(sessions_end) = totp.sessions_end {
            end_sessions_in(&mut tx, user_id, sessions_end).await?;
        }

        tx.commit().await.map_err(sqlx_err)
    }

    async fn use_totp_step(&self, user_id: &Id, step: i64) -> Result<bool> {
        let used = sqlx::query(
            "UPDATE users_ext SET totp_last_used_step = $2
             WHERE user_id = $1 AND totp_secret_ciphertext IS NOT NULL
               AND totp_last_used_step < $2",
        )
        .bind(&user_id.0)
        .bind(step)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

        Ok(used == 1)
    }

    async fn remove_totp(
        &self,
        user_id: &Id,
        sessions_end: TimestampMicros,
    ) -> Result<Option<bool>> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let removed = sqlx::query(CLEAR_TOTP)
            .bind(&user_id.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?
            .rows_affected();

        if removed == 0 {
            tx.rollback().await.map_err(sqlx_err)?;
            return Ok(None);
        }

        let still_on = still_on_in(&mut tx, user_id).await?;
        end_sessions_in(&mut tx, user_id, sessions_end).await?;
        tx.commit().await.map_err(sqlx_err)?;

        Ok(Some(still_on))
    }

    async fn remove_all(&self, user_id: &Id, sessions_end: TimestampMicros) -> Result<bool> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let removed = remove_all_in(&mut tx, user_id, sessions_end).await?;
        tx.commit().await.map_err(sqlx_err)?;

        Ok(removed)
    }

    async fn replace_recovery_codes(
        &self,
        user_id: &Id,
        code_hashes: &[String],
        at: TimestampMicros,
    ) -> Result<()> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        replace_codes_in(&mut tx, user_id, code_hashes, at).await?;
        tx.commit().await.map_err(sqlx_err)
    }

    async fn use_recovery_code(&self, user_id: &Id, code_hash: &str) -> Result<Option<i64>> {
        let left: Option<i32> = sqlx::query_scalar(
            "UPDATE users_ext
             SET recovery_code_hashes = array_remove(recovery_code_hashes, $2)
             WHERE user_id = $1 AND $2 = ANY(recovery_code_hashes)
             RETURNING cardinality(recovery_code_hashes)",
        )
        .bind(&user_id.0)
        .bind(code_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(left.map(i64::from))
    }

    async fn recovery_codes_left(&self, user_id: &Id) -> Result<i64> {
        sqlx::query(
            "SELECT COALESCE(
                        (SELECT cardinality(recovery_code_hashes) FROM users_ext WHERE user_id = $1),
                        0
                    )::BIGINT AS n",
        )
        .bind(&user_id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?
        .try_get("n")
        .map_err(sqlx_err)
    }

    async fn enabled_among(&self, user_ids: &[Id]) -> Result<HashSet<Id>> {
        if user_ids.is_empty() {
            return Ok(HashSet::new());
        }

        let ids = user_ids.iter().map(|id| id.0.clone()).collect::<Vec<_>>();

        let rows = sqlx::query(
            "SELECT user_id FROM users_ext
             WHERE user_id = ANY($1::TEXT[]) AND totp_secret_ciphertext IS NOT NULL
             UNION
             SELECT user_id FROM user_passkeys WHERE user_id = ANY($1::TEXT[])",
        )
        .bind(&ids)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                row.try_get::<String, _>("user_id")
                    .map(Id::from_string)
                    .map_err(sqlx_err)
            })
            .collect()
    }

    async fn end_sessions(&self, user_id: &Id, sessions_end: TimestampMicros) -> Result<()> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        end_sessions_in(&mut tx, user_id, sessions_end).await?;
        tx.commit().await.map_err(sqlx_err)
    }
}
