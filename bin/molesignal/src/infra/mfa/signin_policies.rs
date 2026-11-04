// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 组织的登录要求，存在 `org_settings` 的 `signin` 一节。没有这一节时没有要求。判断在
//! [`crate::app::iam::signin_policy`]。

use std::collections::HashSet;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use crate::{
    infra::persistence::{
        repositories::org_settings::{self, sections::SIGNIN},
        sqlx_err,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 进入一个组织的会话要满足什么。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SigninPolicy {
    /// 要过多因素认证。
    pub require_mfa: bool,
    /// 只能经本组织的 OIDC、OAuth 2.0、SAML 单点登录进入。
    pub sso_only: bool,
}

/// `signin` 一节的值。
#[derive(Serialize, Deserialize)]
struct StoredPolicy {
    #[serde(default)]
    require_mfa: bool,
    #[serde(default)]
    sso_only: bool,
}

#[async_trait]
pub trait SigninPolicyRepository: Send + Sync {
    async fn get(&self, org_id: &Id) -> Result<SigninPolicy>;
    async fn put(
        &self,
        org_id: &Id,
        policy: SigninPolicy,
        updated_by: &Id,
        at: TimestampMicros,
    ) -> Result<()>;
    /// 这些组织里要求多因素认证的。
    async fn requiring_mfa_among(&self, org_ids: &[Id]) -> Result<HashSet<Id>>;
}

pub struct PgSigninPolicyRepository {
    pool: PgPool,
}

impl PgSigninPolicyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SigninPolicyRepository for PgSigninPolicyRepository {
    async fn get(&self, org_id: &Id) -> Result<SigninPolicy> {
        let section = org_settings::read::<StoredPolicy>(&self.pool, &org_id.0, SIGNIN).await?;

        Ok(
            section.map_or_else(SigninPolicy::default, |section| SigninPolicy {
                require_mfa: section.value.require_mfa,
                sso_only: section.value.sso_only,
            }),
        )
    }

    async fn put(
        &self,
        org_id: &Id,
        policy: SigninPolicy,
        updated_by: &Id,
        at: TimestampMicros,
    ) -> Result<()> {
        let stored = StoredPolicy {
            require_mfa: policy.require_mfa,
            sso_only: policy.sso_only,
        };

        org_settings::write(
            &self.pool,
            &org_id.0,
            SIGNIN,
            &stored,
            Some(&updated_by.0),
            at,
        )
        .await
    }

    async fn requiring_mfa_among(&self, org_ids: &[Id]) -> Result<HashSet<Id>> {
        if org_ids.is_empty() {
            return Ok(HashSet::new());
        }

        let ids = org_ids.iter().map(|id| id.0.clone()).collect::<Vec<_>>();

        let rows = sqlx::query(
            "SELECT org_id FROM org_settings
             WHERE section = $2 AND (value->>'require_mfa')::BOOLEAN AND org_id = ANY($1::TEXT[])",
        )
        .bind(&ids)
        .bind(SIGNIN)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                row.try_get::<String, _>("org_id")
                    .map(Id::from_string)
                    .map_err(sqlx_err)
            })
            .collect()
    }
}
