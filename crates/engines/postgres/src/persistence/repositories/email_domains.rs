// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! org 级邮箱域白名单的 Pg 实装，存在 `organizations.email_domains`（升序、去重）。
//!
//! 空名单 = 不限制；邀请 / SSO 自助开户时校验邮箱域是否准入。

use async_trait::async_trait;
use sqlx::PgPool;

use super::sqlx_err;
use crate::shared::{Result, ids::Id};

#[async_trait]
pub trait EmailDomainRepository: Send + Sync {
    /// 列出某 org 的允许域（已规范化，升序）。空 = 不限制。
    async fn list(&self, org_id: &Id) -> Result<Vec<String>>;
    /// 新增一条允许域；`domain` 由 caller 规范化后传入。幂等（已存在则无操作）。
    async fn add(&self, org_id: &Id, domain: &str) -> Result<()>;
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
        let domains: Option<Vec<String>> =
            sqlx::query_scalar("SELECT email_domains FROM organizations WHERE id = $1")
                .bind(&org_id.0)
                .fetch_optional(&self.pool)
                .await
                .map_err(sqlx_err)?;

        Ok(domains.unwrap_or_default())
    }

    async fn add(&self, org_id: &Id, domain: &str) -> Result<()> {
        // 加入后重排，名单始终升序、无重复。
        sqlx::query(
            "UPDATE organizations
             SET email_domains = ARRAY(
                     SELECT DISTINCT listed.domain
                       FROM unnest(email_domains || ARRAY[$2::TEXT]) AS listed(domain)
                      ORDER BY listed.domain
                 )
             WHERE id = $1 AND NOT ($2 = ANY(email_domains))",
        )
        .bind(&org_id.0)
        .bind(domain)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn delete(&self, org_id: &Id, domain: &str) -> Result<()> {
        sqlx::query(
            "UPDATE organizations
             SET email_domains = array_remove(email_domains, $2::TEXT)
             WHERE id = $1 AND $2 = ANY(email_domains)",
        )
        .bind(&org_id.0)
        .bind(domain)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }
}
