// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `domains` + `acme_challenges` 表 Pg 实装（spec Domain management）。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainRow {
    pub id: Id,
    pub org_id: Id,
    pub hostname: String,
    pub state: String, // pending | provisioning | active | failed | expired
    pub cert_pem: Option<String>,
    pub cert_not_after: Option<TimestampMicros>,
    pub last_error: Option<String>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcmeChallenge {
    pub token: String,
    pub domain_id: Id,
    pub key_authorization: String,
    pub expires_at: TimestampMicros,
}

#[async_trait]
pub trait DomainRepository: Send + Sync {
    async fn create(&self, d: DomainRow) -> Result<DomainRow>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<DomainRow>;
    async fn find_by_hostname(&self, hostname: &str) -> Result<Option<DomainRow>>;
    async fn list(&self, org_id: &Id) -> Result<Vec<DomainRow>>;
    /// 跨 org 按 state 列出（ACME runner 扫 `pending` 签发 / `active` 续期用）。
    async fn list_by_state(&self, state: &str) -> Result<Vec<DomainRow>>;
    async fn update_state(
        &self,
        id: &Id,
        state: &str,
        cert_pem: Option<&str>,
        cert_not_after: Option<TimestampMicros>,
        last_error: Option<&str>,
    ) -> Result<()>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;

    async fn put_challenge(&self, c: AcmeChallenge) -> Result<()>;
    async fn get_challenge(&self, token: &str) -> Result<Option<AcmeChallenge>>;
    async fn delete_expired_challenges(&self, now: TimestampMicros) -> Result<u64>;
}

pub struct PgDomainRepository {
    pool: PgPool,
}

impl PgDomainRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, hostname, state, cert_pem, cert_not_after_micros,
                    last_error, created_at_micros, updated_at_micros";

fn row_to(r: sqlx::postgres::PgRow) -> DomainRow {
    DomainRow {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        hostname: r.try_get::<String, _>("hostname").unwrap_or_default(),
        state: r.try_get::<String, _>("state").unwrap_or_default(),
        cert_pem: r
            .try_get::<Option<String>, _>("cert_pem")
            .unwrap_or_default(),
        cert_not_after: r
            .try_get::<Option<i64>, _>("cert_not_after_micros")
            .unwrap_or_default()
            .map(TimestampMicros),
        last_error: r
            .try_get::<Option<String>, _>("last_error")
            .unwrap_or_default(),
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
        updated_at: TimestampMicros(r.try_get::<i64, _>("updated_at_micros").unwrap_or_default()),
    }
}

fn ch_row(r: sqlx::postgres::PgRow) -> AcmeChallenge {
    AcmeChallenge {
        token: r.try_get::<String, _>("token").unwrap_or_default(),
        domain_id: Id(r.try_get::<String, _>("domain_id").unwrap_or_default()),
        key_authorization: r
            .try_get::<String, _>("key_authorization")
            .unwrap_or_default(),
        expires_at: TimestampMicros(r.try_get::<i64, _>("expires_at_micros").unwrap_or_default()),
    }
}

#[async_trait]
impl DomainRepository for PgDomainRepository {
    async fn create(&self, d: DomainRow) -> Result<DomainRow> {
        sqlx::query(
            "INSERT INTO domains
                (id, org_id, hostname, state, cert_pem, cert_not_after_micros, last_error,
                 created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(&d.id.0)
        .bind(&d.org_id.0)
        .bind(&d.hostname)
        .bind(&d.state)
        .bind(&d.cert_pem)
        .bind(d.cert_not_after.map(|t| t.0))
        .bind(&d.last_error)
        .bind(d.created_at.0)
        .bind(d.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(d)
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<DomainRow> {
        let sql = format!("SELECT {COLS} FROM domains WHERE org_id = $1 AND id = $2");
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row_to(row))
    }

    async fn find_by_hostname(&self, hostname: &str) -> Result<Option<DomainRow>> {
        let sql = format!("SELECT {COLS} FROM domains WHERE hostname = $1");
        let row = sqlx::query(&sql)
            .bind(hostname)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row.map(row_to))
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<DomainRow>> {
        let sql = format!("SELECT {COLS} FROM domains WHERE org_id = $1 ORDER BY hostname");
        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn list_by_state(&self, state: &str) -> Result<Vec<DomainRow>> {
        let sql = format!("SELECT {COLS} FROM domains WHERE state = $1 ORDER BY hostname");
        let rows = sqlx::query(&sql)
            .bind(state)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn update_state(
        &self,
        id: &Id,
        state: &str,
        cert_pem: Option<&str>,
        cert_not_after: Option<TimestampMicros>,
        last_error: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE domains SET state = $2, cert_pem = $3, cert_not_after_micros = $4,
                                last_error = $5, updated_at_micros = $6
             WHERE id = $1",
        )
        .bind(&id.0)
        .bind(state)
        .bind(cert_pem)
        .bind(cert_not_after.map(|t| t.0))
        .bind(last_error)
        .bind(TimestampMicros::now().0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM domains WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }

    async fn put_challenge(&self, c: AcmeChallenge) -> Result<()> {
        sqlx::query(
            "INSERT INTO acme_challenges (token, domain_id, key_authorization, expires_at_micros)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (token) DO UPDATE
             SET key_authorization = EXCLUDED.key_authorization,
                 expires_at_micros = EXCLUDED.expires_at_micros",
        )
        .bind(&c.token)
        .bind(&c.domain_id.0)
        .bind(&c.key_authorization)
        .bind(c.expires_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn get_challenge(&self, token: &str) -> Result<Option<AcmeChallenge>> {
        let row = sqlx::query(
            "SELECT token, domain_id, key_authorization, expires_at_micros
             FROM acme_challenges WHERE token = $1",
        )
        .bind(token)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(row.map(ch_row))
    }

    async fn delete_expired_challenges(&self, now: TimestampMicros) -> Result<u64> {
        let res = sqlx::query("DELETE FROM acme_challenges WHERE expires_at_micros < $1")
            .bind(now.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(res.rows_affected())
    }
}
