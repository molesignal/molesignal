// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 组织允许团队成员与个人连的内网主机与网段，存在 `org_settings` 的 `agent.network` 一节。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use super::super::org_settings::{self, sections::AGENT_NETWORK};
use crate::{
    agent::network::{MemberNetworkPolicy, NetworkPolicyStore},
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// `agent.network` 一节的值。
#[derive(Serialize, Deserialize)]
struct StoredNetworkPolicy {
    #[serde(default)]
    member_private_hosts: Vec<String>,
    #[serde(default)]
    member_private_cidrs: Vec<String>,
}

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
        let section =
            org_settings::read::<StoredNetworkPolicy>(&self.pool, &org_id.0, AGENT_NETWORK).await?;

        let Some(section) = section else {
            return Ok(MemberNetworkPolicy::default());
        };

        Ok(MemberNetworkPolicy {
            private_hosts: section.value.member_private_hosts,
            private_cidrs: section.value.member_private_cidrs,
            updated_by: section.updated_by.map(Id),
            updated_at: Some(section.updated_at),
        })
    }

    async fn put(&self, org_id: &Id, policy: MemberNetworkPolicy) -> Result<MemberNetworkPolicy> {
        let updated_by = policy
            .updated_by
            .clone()
            .unwrap_or_else(|| Id(String::new()));
        let updated_at = policy.updated_at.unwrap_or_else(TimestampMicros::now);

        let stored = StoredNetworkPolicy {
            member_private_hosts: policy.private_hosts.clone(),
            member_private_cidrs: policy.private_cidrs.clone(),
        };

        org_settings::write(
            &self.pool,
            &org_id.0,
            AGENT_NETWORK,
            &stored,
            Some(&updated_by.0),
            updated_at,
        )
        .await?;

        Ok(MemberNetworkPolicy {
            updated_by: Some(updated_by),
            updated_at: Some(updated_at),
            ..policy
        })
    }
}
