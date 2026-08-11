// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `org_email_domains` 表 Pg 实装（org 级邮箱域白名单）。
//!
//! Org-scoped 邮箱域白名单。空名单 = 不限制；邀请 / SSO 自助开户时校验邮箱域是否准入。

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[async_trait]
pub trait EmailDomainRepository: Send + Sync {
    /// 列出某 org 的允许域（已规范化，升序）。空 = 不限制。
    async fn list(&self, org_id: &Id) -> Result<Vec<String>>;
    /// 新增一条允许域；`domain` 由 caller 规范化后传入。幂等（已存在则无操作）。
    async fn add(&self, org_id: &Id, domain: &str, now: TimestampMicros) -> Result<()>;
    /// 删除一条允许域。
    async fn delete(&self, org_id: &Id, domain: &str) -> Result<()>;
}

pub struct PgEmailDomainRepository {
    pool: PgPool,
}

impl PgEmailDomainRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl EmailDomainRepository for PgEmailDomainRepository {
    async fn list(&self, org_id: &Id) -> Result<Vec<String>> {
        let rows = sqlx::query(
            "SELECT domain FROM org_email_domains WHERE org_id = $1 ORDER BY domain ASC",
        )
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(rows
            .into_iter()
            .map(|r| r.try_get::<String, _>("domain").unwrap_or_default())
            .collect())
    }

    async fn add(&self, org_id: &Id, domain: &str, now: TimestampMicros) -> Result<()> {
        sqlx::query(
            "INSERT INTO org_email_domains (org_id, domain, created_at_micros)
             VALUES ($1, $2, $3)
             ON CONFLICT (org_id, domain) DO NOTHING",
        )
        .bind(&org_id.0)
        .bind(domain)
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn delete(&self, org_id: &Id, domain: &str) -> Result<()> {
        sqlx::query("DELETE FROM org_email_domains WHERE org_id = $1 AND domain = $2")
            .bind(&org_id.0)
            .bind(domain)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }
}
