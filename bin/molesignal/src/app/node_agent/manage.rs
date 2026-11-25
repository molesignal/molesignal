// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Inventory management: enrollment tokens, nodes, drain, revocation, configuration and quota.

use std::collections::BTreeMap;

use domain::node_agent::{
    EnrollmentToken, NewEnrollmentToken, NodeAgent, NodeConfig, NodeStatus, TaskQuota,
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Actor, NodeAgentService, enroll::token_hash};

const MAX_CONFIG_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateEnrollmentTokenInput {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default = "default_max_uses")]
    pub max_uses: u32,
    #[serde(default = "default_ttl_minutes")]
    pub ttl_minutes: u32,
}

const fn default_max_uses() -> u32 {
    1
}

const fn default_ttl_minutes() -> u32 {
    60
}

/// A new token and where agents use it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrollmentTokenIssued {
    #[serde(flatten)]
    pub issued: NewEnrollmentToken,
    pub endpoint: String,
    pub ca_certificate_pem: String,
}

/// A node with the status it has now.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeView {
    #[serde(flatten)]
    pub node: NodeAgent,
    pub status: NodeStatus,
}

fn random_token() -> Result<String> {
    use rand::TryRng as _;

    let mut bytes = [0u8; 32];

    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .map_err(|error| Error::internal(format!("generate enrollment token: {error}")))?;

    Ok(format!("msnode_{}", hex::encode(bytes)))
}

fn validate_labels(labels: &BTreeMap<String, String>) -> Result<()> {
    if labels.len() > 32
        || labels
            .iter()
            .any(|(key, value)| key.is_empty() || key.len() > 63 || value.len() > 255)
    {
        return Err(Error::invalid(
            "at most 32 labels, keys of 1 to 63 characters and values of at most 255",
        ));
    }

    Ok(())
}

impl NodeAgentService {
    pub async fn create_enrollment_token(
        &self,
        org_id: &Id,
        actor: &Actor,
        input: CreateEnrollmentTokenInput,
    ) -> Result<EnrollmentTokenIssued> {
        validate_labels(&input.labels)?;

        if input.description.len() > 255 {
            return Err(Error::invalid(
                "`description` must stay within 255 characters",
            ));
        }

        if input.max_uses == 0 || input.max_uses > EnrollmentToken::MAX_USES {
            return Err(Error::invalid(format!(
                "`maxUses` must be between 1 and {}",
                EnrollmentToken::MAX_USES
            )));
        }

        let ttl_micros = i64::from(input.ttl_minutes) * 60 * 1_000_000;

        if input.ttl_minutes == 0 || ttl_micros > EnrollmentToken::MAX_TTL_MICROS {
            return Err(Error::invalid(
                "`ttlMinutes` must be between 1 and 43200 (30 days)",
            ));
        }

        let now = TimestampMicros::now();
        let plaintext = random_token()?;

        let token = EnrollmentToken {
            id: Id::new(),
            org_id: org_id.clone(),
            description: input.description,
            labels: input.labels,
            max_uses: input.max_uses,
            uses: 0,
            expires_at: TimestampMicros(now.0 + ttl_micros),
            revoked_at: None,
            last_used_at: None,
            created_by: actor.id.clone(),
            created_at: now,
        };

        self.repository
            .create_enrollment_token(&token, &token_hash(&plaintext))
            .await?;

        self.audit
            .record(
                org_id,
                actor,
                "node_agent.enrollment_token.create",
                ("node_agent_enrollment_token", token.id.as_str()),
                json!({
                    "maxUses": token.max_uses,
                    "expiresAt": token.expires_at,
                    "labels": token.labels,
                }),
            )
            .await;

        Ok(EnrollmentTokenIssued {
            issued: NewEnrollmentToken { token, plaintext },
            endpoint: self.endpoint.clone(),
            ca_certificate_pem: self.authority.ca_certificate_pem().to_string(),
        })
    }

    pub async fn list_enrollment_tokens(&self, org_id: &Id) -> Result<Vec<EnrollmentToken>> {
        self.repository.list_enrollment_tokens(org_id).await
    }

    pub async fn revoke_enrollment_token(
        &self,
        org_id: &Id,
        actor: &Actor,
        token_id: &Id,
    ) -> Result<EnrollmentToken> {
        let token = self
            .repository
            .revoke_enrollment_token(org_id, token_id, TimestampMicros::now())
            .await?;

        self.audit
            .record(
                org_id,
                actor,
                "node_agent.enrollment_token.revoke",
                ("node_agent_enrollment_token", token.id.as_str()),
                json!({"uses": token.uses}),
            )
            .await;

        Ok(token)
    }

    fn view(&self, node: NodeAgent, now: TimestampMicros) -> NodeView {
        let status = node.status(now, self.timings.offline_after_micros());
        NodeView { node, status }
    }

    pub async fn list_nodes(&self, org_id: &Id) -> Result<Vec<NodeView>> {
        let now = TimestampMicros::now();
        let nodes = self.repository.list_nodes(org_id).await?;
        Ok(nodes.into_iter().map(|node| self.view(node, now)).collect())
    }

    pub async fn get_node(&self, org_id: &Id, node_id: &Id) -> Result<NodeView> {
        let node = self.repository.get_node(org_id, node_id).await?;
        Ok(self.view(node, TimestampMicros::now()))
    }

    /// Stops new tasks on the node and lets running ones finish until the deadline.
    pub async fn drain(
        &self,
        org_id: &Id,
        actor: &Actor,
        node_id: &Id,
        deadline_secs: u32,
    ) -> Result<NodeView> {
        if deadline_secs == 0 || deadline_secs > 86_400 {
            return Err(Error::invalid(
                "`deadlineSeconds` must be between 1 and 86400",
            ));
        }

        let now = TimestampMicros::now();
        let deadline = TimestampMicros(now.0 + i64::from(deadline_secs) * 1_000_000);

        let node = self
            .repository
            .set_drain(org_id, node_id, Some(deadline), now)
            .await?;

        self.audit
            .record(
                org_id,
                actor,
                "node_agent.drain",
                ("node_agent", node_id.as_str()),
                json!({"deadline": deadline}),
            )
            .await;

        Ok(self.view(node, now))
    }

    pub async fn resume(&self, org_id: &Id, actor: &Actor, node_id: &Id) -> Result<NodeView> {
        let now = TimestampMicros::now();
        let node = self
            .repository
            .set_drain(org_id, node_id, None, now)
            .await?;

        self.audit
            .record(
                org_id,
                actor,
                "node_agent.resume",
                ("node_agent", node_id.as_str()),
                json!({}),
            )
            .await;

        Ok(self.view(node, now))
    }

    /// Revokes the node: its certificate stops working, its active tasks end, and a connected
    /// agent is told to wipe its identity. Enrolling again is the only way back.
    pub async fn revoke(
        &self,
        org_id: &Id,
        actor: &Actor,
        node_id: &Id,
        reason: &str,
    ) -> Result<NodeView> {
        let reason = reason.trim();

        if reason.is_empty() || reason.len() > 512 {
            return Err(Error::invalid(
                "a reason of 1 to 512 characters is required",
            ));
        }

        let now = TimestampMicros::now();

        let (node, cancelled_tasks) = self
            .repository
            .revoke_node(org_id, node_id, reason, now)
            .await?;

        self.audit
            .record(
                org_id,
                actor,
                "node_agent.revoke",
                ("node_agent", node_id.as_str()),
                json!({
                    "reason": reason,
                    "cancelledTasks": cancelled_tasks,
                    "certificateSerial": node.certificate_serial,
                }),
            )
            .await;

        Ok(self.view(node, now))
    }

    /// Stores the configuration as the node's next revision; a connected agent gets it at once.
    pub async fn set_config(
        &self,
        org_id: &Id,
        actor: &Actor,
        node_id: &Id,
        config: Value,
    ) -> Result<NodeView> {
        // Schema v1 (`domain::node_agent::NodeConfig`): stored and sent with only the keys set.
        let config = NodeConfig::parse(config)?.to_value();
        let size = serde_json::to_vec(&config).map_or(usize::MAX, |bytes| bytes.len());

        if size > MAX_CONFIG_BYTES {
            return Err(Error::invalid(
                "a node configuration must stay within 256 KiB",
            ));
        }

        let now = TimestampMicros::now();

        let node = self
            .repository
            .set_desired_config(org_id, node_id, config, &actor.id, now)
            .await?;

        self.audit
            .record(
                org_id,
                actor,
                "node_agent.config.update",
                ("node_agent", node_id.as_str()),
                json!({"revision": node.desired_config_revision(), "bytes": size}),
            )
            .await;

        Ok(self.view(node, now))
    }

    pub async fn task_quota(&self, org_id: &Id) -> Result<TaskQuota> {
        self.repository.task_quota(org_id).await
    }

    pub async fn set_task_quota(
        &self,
        org_id: &Id,
        actor: &Actor,
        quota: TaskQuota,
    ) -> Result<TaskQuota> {
        quota.validate()?;
        let previous = self.repository.task_quota(org_id).await?;

        self.repository
            .set_task_quota(org_id, &quota, &actor.id, TimestampMicros::now())
            .await?;

        self.audit
            .record(
                org_id,
                actor,
                "node_agent.quota.update",
                ("organization", org_id.as_str()),
                json!({"previous": previous, "quota": quota}),
            )
            .await;

        Ok(quota)
    }
}
