// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 等待本人确认的关联请求（`sso_link_requests`）。
//!
//! 已有账号第一次通过某个服务登录时，先存下身份源给出的身份，等本人在发起登录的页面确认：
//! 输入发到账号邮箱的验证码，或输入账号密码。浏览器持有 `code`，邮件里只有验证码；两者都在
//! 服务端对上才算邮件确认，所以别人发起的关联，账号主人点开邮件也不会被关联上。只存摘要。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row, types::Json};

use crate::{
    infra::persistence::sqlx_err,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 每个账号同时未过期的请求上限，挡住反复发起的关联。
pub const MAX_PENDING_PER_USER: i64 = 10;

/// 身份源当时给出的身份；确认后据此入组和映射角色。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedIdentity {
    pub email: String,
    pub display_name: String,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub email_verified: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct SsoLinkRequest {
    pub id: Id,
    pub provider_id: Id,
    pub subject: String,
    pub user_id: Id,
    pub identity: LinkedIdentity,
    pub code_hash: String,
    pub email_code_hash: Option<String>,
    pub email_sent_at: Option<TimestampMicros>,
    pub attempts: i32,
    pub created_at: TimestampMicros,
    pub expires_at: TimestampMicros,
}

#[async_trait]
pub trait SsoLinkRequestRepository: Send + Sync {
    /// 存下新请求并清掉过期的；账号的未过期请求已到上限时返回 `false`。
    async fn create(&self, request: SsoLinkRequest) -> Result<bool>;
    /// 未过期的请求。
    async fn find_by_code(
        &self,
        code_hash: &str,
        now: TimestampMicros,
    ) -> Result<Option<SsoLinkRequest>>;
    /// 账号最近一次发验证码的时间，用于冷却。
    async fn last_email_sent(&self, user_id: &Id) -> Result<Option<TimestampMicros>>;
    async fn set_email_code(
        &self,
        id: &Id,
        email_code_hash: &str,
        at: TimestampMicros,
    ) -> Result<()>;
    /// 记一次输错，返回累计次数。
    async fn record_failure(&self, id: &Id) -> Result<i32>;
    async fn delete(&self, id: &Id) -> Result<()>;
}

pub struct PgSsoLinkRequestRepository {
    pool: PgPool,
}

impl PgSsoLinkRequestRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_request(row: sqlx::postgres::PgRow) -> Result<SsoLinkRequest> {
    let identity: Json<LinkedIdentity> = row.try_get("identity").map_err(sqlx_err)?;

    Ok(SsoLinkRequest {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        provider_id: Id::from_string(row.try_get::<String, _>("provider_id").map_err(sqlx_err)?),
        subject: row.try_get("subject").map_err(sqlx_err)?,
        user_id: Id::from_string(row.try_get::<String, _>("user_id").map_err(sqlx_err)?),
        identity: identity.0,
        code_hash: row.try_get("code_hash").map_err(sqlx_err)?,
        email_code_hash: row.try_get("email_code_hash").map_err(sqlx_err)?,
        email_sent_at: row
            .try_get::<Option<TimestampMicros>, _>("email_sent_at")
            .map_err(sqlx_err)?,
        attempts: row.try_get("attempts").map_err(sqlx_err)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        expires_at: row
            .try_get::<TimestampMicros, _>("expires_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl SsoLinkRequestRepository for PgSsoLinkRequestRepository {
    async fn create(&self, request: SsoLinkRequest) -> Result<bool> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        sqlx::query("DELETE FROM sso_link_requests WHERE expires_at <= $1")
            .bind(request.created_at)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;

        let pending: i64 =
            sqlx::query("SELECT COUNT(*)::BIGINT AS n FROM sso_link_requests WHERE user_id = $1")
                .bind(&request.user_id.0)
                .fetch_one(&mut *tx)
                .await
                .map_err(sqlx_err)?
                .try_get("n")
                .map_err(sqlx_err)?;

        if pending >= MAX_PENDING_PER_USER {
            tx.rollback().await.map_err(sqlx_err)?;
            return Ok(false);
        }

        sqlx::query(
            "INSERT INTO sso_link_requests
                (id, provider_id, subject, user_id, identity, code_hash, email_code_hash,
                 email_sent_at, attempts, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, NULL, NULL, 0, $7, $8)",
        )
        .bind(&request.id.0)
        .bind(&request.provider_id.0)
        .bind(&request.subject)
        .bind(&request.user_id.0)
        .bind(Json(request.identity.clone()))
        .bind(&request.code_hash)
        .bind(request.created_at)
        .bind(request.expires_at)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(true)
    }

    async fn find_by_code(
        &self,
        code_hash: &str,
        now: TimestampMicros,
    ) -> Result<Option<SsoLinkRequest>> {
        sqlx::query(
            "SELECT id, provider_id, subject, user_id, identity, code_hash, email_code_hash,
                    email_sent_at, attempts, created_at, expires_at
             FROM sso_link_requests
             WHERE code_hash = $1 AND expires_at > $2",
        )
        .bind(code_hash)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(row_to_request)
        .transpose()
    }

    async fn last_email_sent(&self, user_id: &Id) -> Result<Option<TimestampMicros>> {
        let sent: Option<TimestampMicros> = sqlx::query(
            "SELECT MAX(email_sent_at) AS sent FROM sso_link_requests WHERE user_id = $1",
        )
        .bind(&user_id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?
        .try_get("sent")
        .map_err(sqlx_err)?;

        Ok(sent)
    }

    async fn set_email_code(
        &self,
        id: &Id,
        email_code_hash: &str,
        at: TimestampMicros,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE sso_link_requests
             SET email_code_hash = $2, email_sent_at = $3
             WHERE id = $1",
        )
        .bind(&id.0)
        .bind(email_code_hash)
        .bind(at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn record_failure(&self, id: &Id) -> Result<i32> {
        let row = sqlx::query(
            "UPDATE sso_link_requests SET attempts = attempts + 1 WHERE id = $1 RETURNING attempts",
        )
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::unauthorized("this confirmation has expired; sign in again"))?;

        row.try_get("attempts").map_err(sqlx_err)
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM sso_link_requests WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }
}
