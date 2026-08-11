// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `sso_sessions` 表 CRUD。

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone)]
pub struct SsoSession {
    pub id: Id,
    pub user_id: Id,
    pub provider: String,
    pub idp_subject: String,
    pub issued_at: TimestampMicros,
    pub last_login_at: TimestampMicros,
}

#[async_trait]
pub trait SsoSessionRepository: Send + Sync {
    async fn upsert(&self, sess: SsoSession) -> Result<()>;
    async fn find_by_subject(&self, provider: &str, subject: &str) -> Result<Option<SsoSession>>;
    async fn list_for_user(&self, user_id: &Id) -> Result<Vec<SsoSession>>;
}

pub struct PgSsoSessionRepository {
    pool: PgPool,
}

impl PgSsoSessionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_sess(r: sqlx::postgres::PgRow) -> SsoSession {
    SsoSession {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        user_id: Id(r.try_get::<String, _>("user_id").unwrap_or_default()),
        provider: r.try_get::<String, _>("provider").unwrap_or_default(),
        idp_subject: r.try_get::<String, _>("idp_subject").unwrap_or_default(),
        issued_at: TimestampMicros(r.try_get::<i64, _>("issued_at_micros").unwrap_or_default()),
        last_login_at: TimestampMicros(
            r.try_get::<i64, _>("last_login_at_micros")
                .unwrap_or_default(),
        ),
    }
}

#[async_trait]
impl SsoSessionRepository for PgSsoSessionRepository {
    async fn upsert(&self, sess: SsoSession) -> Result<()> {
        sqlx::query(
            "INSERT INTO sso_sessions
                (id, user_id, provider, idp_subject, issued_at_micros, last_login_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (id) DO UPDATE
             SET last_login_at_micros = EXCLUDED.last_login_at_micros",
        )
        .bind(&sess.id.0)
        .bind(&sess.user_id.0)
        .bind(&sess.provider)
        .bind(&sess.idp_subject)
        .bind(sess.issued_at.0)
        .bind(sess.last_login_at.0)
        .execute(&self.pool)
        .await
        .map_err(super::super::persistence::sqlx_err)?;
        Ok(())
    }

    async fn find_by_subject(&self, provider: &str, subject: &str) -> Result<Option<SsoSession>> {
        let row = sqlx::query(
            "SELECT id, user_id, provider, idp_subject, issued_at_micros, last_login_at_micros
             FROM sso_sessions WHERE provider = $1 AND idp_subject = $2
             ORDER BY last_login_at_micros DESC LIMIT 1",
        )
        .bind(provider)
        .bind(subject)
        .fetch_optional(&self.pool)
        .await
        .map_err(super::super::persistence::sqlx_err)?;
        Ok(row.map(row_to_sess))
    }

    async fn list_for_user(&self, user_id: &Id) -> Result<Vec<SsoSession>> {
        let rows = sqlx::query(
            "SELECT id, user_id, provider, idp_subject, issued_at_micros, last_login_at_micros
             FROM sso_sessions WHERE user_id = $1 ORDER BY last_login_at_micros DESC",
        )
        .bind(&user_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(super::super::persistence::sqlx_err)?;
        Ok(rows.into_iter().map(row_to_sess).collect())
    }
}
