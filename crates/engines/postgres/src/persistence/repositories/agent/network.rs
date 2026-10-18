// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `agent_network_settings`：组织允许团队成员与个人连的内网主机与网段。

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::{
    agent::network::{MemberNetworkPolicy, NetworkPolicyStore},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgNetworkPolicyStore {
    pool: PgPool,
}

impl PgNetworkPolicyStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl NetworkPolicyStore for PgNetworkPolicyStore {
    async fn get(&self, org_id: &Id) -> Result<MemberNetworkPolicy> {
        let row = sqlx::query(
            "SELECT member_private_hosts, member_private_cidrs, updated_by, updated_at_micros
             FROM agent_network_settings WHERE org_id = $1",
        )
        .bind(&org_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let Some(row) = row else {
            return Ok(MemberNetworkPolicy::default());
        };

        let hosts: Json<Vec<String>> = row.try_get("member_private_hosts").map_err(sqlx_err)?;
        let cidrs: Json<Vec<String>> = row.try_get("member_private_cidrs").map_err(sqlx_err)?;

        Ok(MemberNetworkPolicy {
            private_hosts: hosts.0,
            private_cidrs: cidrs.0,
            updated_by: Some(Id(row.try_get("updated_by").map_err(sqlx_err)?)),
            updated_at: Some(TimestampMicros(
                row.try_get("updated_at_micros").map_err(sqlx_err)?,
            )),
        })
    }

    async fn put(&self, org_id: &Id, policy: MemberNetworkPolicy) -> Result<MemberNetworkPolicy> {
        let updated_by = policy
            .updated_by
            .clone()
            .unwrap_or_else(|| Id(String::new()));
        let updated_at = policy.updated_at.unwrap_or_else(TimestampMicros::now);

        sqlx::query(
            "INSERT INTO agent_network_settings
                (org_id, member_private_hosts, member_private_cidrs, updated_by, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (org_id) DO UPDATE SET
                member_private_hosts = EXCLUDED.member_private_hosts,
                member_private_cidrs = EXCLUDED.member_private_cidrs,
                updated_by = EXCLUDED.updated_by,
                updated_at_micros = EXCLUDED.updated_at_micros",
        )
        .bind(&org_id.0)
        .bind(Json(&policy.private_hosts))
        .bind(Json(&policy.private_cidrs))
        .bind(&updated_by.0)
        .bind(updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(MemberNetworkPolicy {
            updated_by: Some(updated_by),
            updated_at: Some(updated_at),
            ..policy
        })
    }
}
