// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 账号的 Passkey（`user_passkeys`）。
//!
//! 一行是一个 WebAuthn 凭据：webauthn-rs 的 `Passkey`（公钥、签名计数、备份状态）存成 JSONB，
//! 不是密文。加上账号的第一个第二因素时多因素认证随之开启：同一个事务里存下恢复码，让账号此前
//! 开始的登录作废；去掉最后一个时一并删掉恢复码。

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};
use webauthn_rs::prelude::Passkey;

use super::factors::{end_sessions_in, replace_codes_in, still_on_in};
use crate::{
    infra::persistence::sqlx_err,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 一个已存下的 Passkey。
#[derive(Debug, Clone)]
pub struct StoredPasskey {
    pub id: Id,
    pub user_id: Id,
    pub passkey: Passkey,
    pub name: String,
    /// 浏览器报告的验证器型号（32 位十六进制），只用来显示来源。
    pub aaguid: Option<String>,
    pub created_at: TimestampMicros,
    pub last_used_at: Option<TimestampMicros>,
}

/// 这个 Passkey 让多因素认证开启时一并存下的：新的一组恢复码，和让此前登录作废的时间。
pub struct TurnOn<'a> {
    pub recovery_code_hashes: &'a [String],
    pub sessions_end: TimestampMicros,
}

#[async_trait]
pub trait PasskeyRepository: Send + Sync {
    async fn list(&self, user_id: &Id) -> Result<Vec<StoredPasskey>>;
    async fn by_credential(&self, credential_id: &[u8]) -> Result<Option<StoredPasskey>>;
    /// 存下新的 Passkey；`turn_on` 有值时同一个事务里换上恢复码、让此前开始的登录作废。凭据已经
    /// 存在时什么都不做，返回 `false`。
    async fn add(&self, passkey: &StoredPasskey, turn_on: Option<TurnOn<'_>>) -> Result<bool>;
    /// 用过一次：记下更新过的签名计数和备份状态，以及使用时间。
    async fn record_use(&self, id: &Id, passkey: &Passkey, at: TimestampMicros) -> Result<()>;
    async fn rename(&self, user_id: &Id, id: &Id, name: &str) -> Result<bool>;
    /// 删掉一个 Passkey，账号在 `sessions_end` 之前开始的登录作废。不存在时返回 `None`，否则
    /// 返回多因素认证是否仍开着。
    async fn remove(
        &self,
        user_id: &Id,
        id: &Id,
        sessions_end: TimestampMicros,
    ) -> Result<Option<bool>>;
}

pub struct PgPasskeyRepository {
    pool: PgPool,
}

impl PgPasskeyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLUMNS: &str = "id, user_id, passkey, name, aaguid, created_at, last_used_at";

fn row_to_passkey(row: sqlx::postgres::PgRow) -> Result<StoredPasskey> {
    let passkey: Json<serde_json::Value> = row.try_get("passkey").map_err(sqlx_err)?;

    Ok(StoredPasskey {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        user_id: Id::from_string(row.try_get::<String, _>("user_id").map_err(sqlx_err)?),
        passkey: serde_json::from_value(passkey.0)
            .map_err(|error| Error::internal(format!("stored passkey cannot be read: {error}")))?,
        name: row.try_get("name").map_err(sqlx_err)?,
        aaguid: row.try_get("aaguid").map_err(sqlx_err)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        last_used_at: row
            .try_get::<Option<TimestampMicros>, _>("last_used_at")
            .map_err(sqlx_err)?,
    })
}

fn stored(passkey: &Passkey) -> Result<Json<serde_json::Value>> {
    serde_json::to_value(passkey)
        .map(Json)
        .map_err(|error| Error::internal(format!("passkey cannot be stored: {error}")))
}

#[async_trait]
impl PasskeyRepository for PgPasskeyRepository {
    async fn list(&self, user_id: &Id) -> Result<Vec<StoredPasskey>> {
        sqlx::query(&format!(
            "SELECT {COLUMNS} FROM user_passkeys WHERE user_id = $1 ORDER BY created_at"
        ))
        .bind(&user_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?
        .into_iter()
        .map(row_to_passkey)
        .collect()
    }

    async fn by_credential(&self, credential_id: &[u8]) -> Result<Option<StoredPasskey>> {
        sqlx::query(&format!(
            "SELECT {COLUMNS} FROM user_passkeys WHERE credential_id = $1"
        ))
        .bind(credential_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(row_to_passkey)
        .transpose()
    }

    async fn add(&self, passkey: &StoredPasskey, turn_on: Option<TurnOn<'_>>) -> Result<bool> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let added = sqlx::query(
            "INSERT INTO user_passkeys
                (id, user_id, credential_id, passkey, name, aaguid, created_at,
                 last_used_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, NULL)
             ON CONFLICT (credential_id) DO NOTHING",
        )
        .bind(&passkey.id.0)
        .bind(&passkey.user_id.0)
        .bind(passkey.passkey.cred_id().to_vec())
        .bind(stored(&passkey.passkey)?)
        .bind(&passkey.name)
        .bind(&passkey.aaguid)
        .bind(passkey.created_at)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

        if added == 0 {
            tx.rollback().await.map_err(sqlx_err)?;
            return Ok(false);
        }

        if let Some(turn_on) = turn_on {
            replace_codes_in(
                &mut tx,
                &passkey.user_id,
                turn_on.recovery_code_hashes,
                passkey.created_at,
            )
            .await?;

            end_sessions_in(&mut tx, &passkey.user_id, turn_on.sessions_end).await?;
        }

        tx.commit().await.map_err(sqlx_err)?;
        Ok(true)
    }

    async fn record_use(&self, id: &Id, passkey: &Passkey, at: TimestampMicros) -> Result<()> {
        sqlx::query("UPDATE user_passkeys SET passkey = $2, last_used_at = $3 WHERE id = $1")
            .bind(&id.0)
            .bind(stored(passkey)?)
            .bind(at)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }

    async fn rename(&self, user_id: &Id, id: &Id, name: &str) -> Result<bool> {
        let renamed =
            sqlx::query("UPDATE user_passkeys SET name = $3 WHERE id = $2 AND user_id = $1")
                .bind(&user_id.0)
                .bind(&id.0)
                .bind(name)
                .execute(&self.pool)
                .await
                .map_err(sqlx_err)?
                .rows_affected();

        Ok(renamed == 1)
    }

    async fn remove(
        &self,
        user_id: &Id,
        id: &Id,
        sessions_end: TimestampMicros,
    ) -> Result<Option<bool>> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let removed = sqlx::query("DELETE FROM user_passkeys WHERE id = $2 AND user_id = $1")
            .bind(&user_id.0)
            .bind(&id.0)
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
}
