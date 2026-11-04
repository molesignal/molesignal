// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use async_trait::async_trait;

use super::{
    PgSyntheticRepository,
    codec::{AGENT_COLS, CONFIGURED_AGENT_COLS, row_to_agent},
};
use crate::{
    domain::synthetics::{AgentCapacity, ProbeAgent, ProbeCapability, SyntheticAgentRepository},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub const BUILTIN_LOCAL_AGENT_ID: &str = "builtin-local-runner";
pub const BUILTIN_LOCAL_LOCATION_ID: &str = "builtin-local";
const EMBEDDED_MAX_CONCURRENT: u32 = 8;

fn agent_select(condition: &str) -> String {
    format!(
        "SELECT {AGENT_COLS}
         FROM synthetic_probe_agents agent
         JOIN synthetic_probe_locations location ON location.id = agent.location_id
         WHERE {condition}"
    )
}

fn configured_agent_select(condition: &str) -> String {
    format!(
        "SELECT {CONFIGURED_AGENT_COLS}
         FROM synthetic_probe_agents agent
         JOIN synthetic_probe_locations location ON location.id = agent.location_id
         LEFT JOIN synthetic_probe_agent_configurations configuration
           ON configuration.agent_id = agent.id AND configuration.organization_id = $1
         WHERE {condition}"
    )
}

#[async_trait]
impl SyntheticAgentRepository for PgSyntheticRepository {
    async fn create_agent(&self, agent: ProbeAgent) -> Result<ProbeAgent> {
        let expected_org: Option<String> = sqlx::query_scalar(
            "SELECT organization_id FROM synthetic_probe_locations
             WHERE id = $1 AND execution = 'agent_pool' AND lifecycle = 'active'",
        )
        .bind(&agent.location_id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(super::sqlx_err)?;

        if expected_org.as_deref() != agent.organization_id.as_ref().map(Id::as_str) {
            return Err(Error::forbidden("Agent Location scope mismatch"));
        }

        sqlx::query(
            "INSERT INTO synthetic_probe_agents
                (id, location_id, name, hostname, status, agent_version, protocol_version,
                 capabilities, capacity, labels, public_key_der, certificate_serial,
                 certificate_expires_at, last_heartbeat_at,
                 last_result_sequence, revoked_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                     $15, $16, $17, $18)",
        )
        .bind(&agent.id.0)
        .bind(&agent.location_id.0)
        .bind(&agent.name)
        .bind(&agent.hostname)
        .bind(agent.status.as_str())
        .bind(&agent.agent_version)
        .bind(agent.protocol_version as i32)
        .bind(
            agent
                .capabilities
                .iter()
                .map(|capability| capability.as_str())
                .collect::<Vec<_>>(),
        )
        .bind(sqlx::types::Json(&agent.capacity))
        .bind(sqlx::types::Json(&agent.labels))
        .bind(&agent.public_key_der)
        .bind(&agent.certificate_serial)
        .bind(agent.certificate_expires_at)
        .bind(agent.last_heartbeat_at)
        .bind(agent.last_result_sequence as i64)
        .bind(agent.revoked_at)
        .bind(agent.created_at)
        .bind(agent.updated_at)
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?;

        self.get_agent_for_scope(agent.organization_id.as_ref(), &agent.id)
            .await
    }

    async fn update_agent(&self, agent: ProbeAgent) -> Result<ProbeAgent> {
        let id: Option<String> = sqlx::query_scalar(
            "UPDATE synthetic_probe_agents agent
             SET name = $3, hostname = $4, status = $5, agent_version = $6,
                 protocol_version = $7, capabilities = $8, capacity = $9, labels = $10,
                 certificate_serial = $11, certificate_expires_at = $12,
                 last_heartbeat_at = $13, last_result_sequence = $14,
                 revoked_at = $15, updated_at = $16, feature_level = $17
             FROM synthetic_probe_locations location
             WHERE agent.id = $1 AND location.id = agent.location_id
               AND location.organization_id IS NOT DISTINCT FROM $2
             RETURNING agent.id",
        )
        .bind(&agent.id.0)
        .bind(agent.organization_id.as_ref().map(Id::as_str))
        .bind(&agent.name)
        .bind(&agent.hostname)
        .bind(agent.status.as_str())
        .bind(&agent.agent_version)
        .bind(agent.protocol_version as i32)
        .bind(
            agent
                .capabilities
                .iter()
                .map(|capability| capability.as_str())
                .collect::<Vec<_>>(),
        )
        .bind(sqlx::types::Json(&agent.capacity))
        .bind(sqlx::types::Json(&agent.labels))
        .bind(&agent.certificate_serial)
        .bind(agent.certificate_expires_at)
        .bind(agent.last_heartbeat_at)
        .bind(agent.last_result_sequence as i64)
        .bind(agent.revoked_at)
        .bind(agent.updated_at)
        .bind(i32::try_from(agent.feature_level).unwrap_or(i32::MAX))
        .fetch_optional(&self.pool)
        .await
        .map_err(super::sqlx_err)?;

        if id.is_none() {
            return Err(Error::not_found("Probe Agent"));
        }

        self.get_agent_for_scope(agent.organization_id.as_ref(), &agent.id)
            .await
    }

    async fn touch_agent(&self, agent_id: &Id, now: TimestampMicros) -> Result<()> {
        sqlx::query(
            "UPDATE synthetic_probe_agents
             SET status = 'online', last_heartbeat_at = $2, updated_at = $2
             WHERE id = $1 AND status <> 'revoked'",
        )
        .bind(&agent_id.0)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?;

        Ok(())
    }

    async fn refresh_liveness(
        &self,
        now: TimestampMicros,
        stale_before: TimestampMicros,
    ) -> Result<u64> {
        let gone = sqlx::query(
            "UPDATE synthetic_probe_agents
             SET status = 'offline', updated_at = $1
             WHERE status IN ('online', 'degraded', 'draining')
               AND COALESCE(last_heartbeat_at, TIMESTAMPTZ 'epoch') < $2",
        )
        .bind(now)
        .bind(stale_before)
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?
        .rows_affected();

        // A Location is online when every Agent that should run its checks is, degraded when only
        // some are, offline when none is, and unknown before any Agent has joined it.
        sqlx::query(
            "WITH derived AS (
                 SELECT location.id,
                        CASE
                            WHEN count(agent.id) FILTER (WHERE agent.status <> 'revoked') = 0
                                THEN 'unknown'
                            WHEN count(agent.id) FILTER (WHERE agent.status IN ('online', 'degraded')) = 0
                                THEN 'offline'
                            WHEN count(agent.id) FILTER (WHERE agent.status IN ('online', 'degraded'))
                                 < count(agent.id) FILTER (WHERE agent.status IN ('online', 'degraded', 'offline'))
                                THEN 'degraded'
                            ELSE 'online'
                        END AS health
                 FROM synthetic_probe_locations location
                 LEFT JOIN synthetic_probe_agents agent ON agent.location_id = location.id
                 WHERE location.lifecycle = 'active'
                 GROUP BY location.id
             )
             UPDATE synthetic_probe_locations location
             SET health = derived.health, updated_at = $1
             FROM derived
             WHERE location.id = derived.id AND location.health IS DISTINCT FROM derived.health",
        )
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?;

        Ok(gone)
    }

    async fn update_agent_configuration(
        &self,
        org_id: &Id,
        actor_id: &Id,
        agent_id: &Id,
        name: &str,
        labels: &BTreeMap<String, String>,
        updated_at: TimestampMicros,
    ) -> Result<ProbeAgent> {
        let configured_agent_id: Option<String> = sqlx::query_scalar(
            "INSERT INTO synthetic_probe_agent_configurations AS saved_configuration
                (organization_id, agent_id, name, labels, created_by, updated_by,
                 created_at, updated_at)
             SELECT $1, agent.id, $3, $4, $5, $5, $6, $6
             FROM synthetic_probe_agents agent
             JOIN synthetic_probe_locations location ON location.id = agent.location_id
             WHERE agent.id = $2
               AND (location.organization_id = $1 OR location.organization_id IS NULL)
             ON CONFLICT (organization_id, agent_id) DO UPDATE
             SET name = EXCLUDED.name, labels = EXCLUDED.labels,
                 updated_by = EXCLUDED.updated_by,
                 updated_at = EXCLUDED.updated_at
             RETURNING saved_configuration.agent_id",
        )
        .bind(&org_id.0)
        .bind(&agent_id.0)
        .bind(name)
        .bind(sqlx::types::Json(labels))
        .bind(&actor_id.0)
        .bind(updated_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(super::sqlx_err)?;

        if configured_agent_id.is_none() {
            return Err(Error::not_found("Probe Agent"));
        }

        self.get_agent(org_id, agent_id).await
    }

    async fn get_agent(&self, org_id: &Id, agent_id: &Id) -> Result<ProbeAgent> {
        let row = sqlx::query(&configured_agent_select(
            "agent.id = $2 AND (location.organization_id = $1 OR location.organization_id IS NULL)",
        ))
        .bind(&org_id.0)
        .bind(&agent_id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(super::sqlx_err)?;

        row_to_agent(row)
    }

    async fn get_agent_by_certificate_serial(&self, serial: &str) -> Result<ProbeAgent> {
        let row = sqlx::query(&agent_select(
            "agent.certificate_serial = $1 AND agent.status <> 'revoked'",
        ))
        .bind(serial)
        .fetch_one(&self.pool)
        .await
        .map_err(super::sqlx_err)?;

        row_to_agent(row)
    }

    async fn list_all_agents(&self, org_id: &Id) -> Result<Vec<ProbeAgent>> {
        sqlx::query(&format!(
            "{} ORDER BY location.scope, location.name,
                 COALESCE(configuration.name, agent.name), agent.id",
            configured_agent_select(
                "location.organization_id = $1 OR location.organization_id IS NULL"
            )
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(super::sqlx_err)?
        .into_iter()
        .map(row_to_agent)
        .collect()
    }

    async fn list_agents(&self, org_id: &Id, location_id: &Id) -> Result<Vec<ProbeAgent>> {
        sqlx::query(&format!(
            "{} ORDER BY COALESCE(configuration.name, agent.name), agent.id",
            configured_agent_select(
                "agent.location_id = $2 AND (location.organization_id = $1 OR location.organization_id IS NULL)"
            )
        ))
        .bind(&org_id.0)
        .bind(&location_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(super::sqlx_err)?
        .into_iter()
        .map(row_to_agent)
        .collect()
    }
}

impl PgSyntheticRepository {
    /// Registers the process-local executor as a system-managed Agent identity.
    ///
    /// The identity deliberately has no certificate: it never crosses the Probe mTLS boundary,
    /// but using the normal Agent lease owner preserves the same retry, expiry, and result rules.
    pub async fn ensure_builtin_local_agent(
        &self,
        hostname: &str,
        agent_version: &str,
        capabilities: &[ProbeCapability],
        feature_level: u32,
        now: TimestampMicros,
    ) -> Result<ProbeAgent> {
        let capacity = AgentCapacity {
            max_concurrent: EMBEDDED_MAX_CONCURRENT,
            max_browser_concurrent: 1,
            available: EMBEDDED_MAX_CONCURRENT,
            available_browser: 1,
        };

        let labels = BTreeMap::from([
            ("execution".to_string(), "embedded".to_string()),
            ("system_managed".to_string(), "true".to_string()),
        ]);

        let capabilities = capabilities
            .iter()
            .map(|capability| capability.as_str())
            .collect::<Vec<_>>();

        let rows = sqlx::query(
            "INSERT INTO synthetic_probe_agents AS agent
                (id, location_id, name, hostname, status, agent_version, protocol_version,
                 feature_level, capabilities, capacity, labels, public_key_der,
                 certificate_serial, certificate_expires_at, last_heartbeat_at,
                 last_result_sequence, revoked_at, created_at, updated_at)
             SELECT $1, location.id, 'Standalone Embedded Runner', $2, 'online', $3, 1,
                    $9, $4, $5, $6, decode('', 'hex'), NULL, NULL, $7, 0, NULL, $7, $7
             FROM synthetic_probe_locations location
             WHERE location.id = $8 AND location.execution = 'embedded'
               AND location.lifecycle = 'active' AND location.system_managed
             ON CONFLICT (id) DO UPDATE
             SET location_id = EXCLUDED.location_id, name = EXCLUDED.name,
                 hostname = EXCLUDED.hostname, status = 'online',
                 agent_version = EXCLUDED.agent_version,
                 protocol_version = EXCLUDED.protocol_version,
                 feature_level = EXCLUDED.feature_level,
                 capabilities = EXCLUDED.capabilities, capacity = EXCLUDED.capacity,
                 labels = EXCLUDED.labels, public_key_der = EXCLUDED.public_key_der,
                 certificate_serial = NULL, certificate_expires_at = NULL,
                 last_heartbeat_at = EXCLUDED.last_heartbeat_at,
                 revoked_at = NULL, updated_at = EXCLUDED.updated_at",
        )
        .bind(BUILTIN_LOCAL_AGENT_ID)
        .bind(hostname)
        .bind(agent_version)
        .bind(capabilities)
        .bind(sqlx::types::Json(capacity))
        .bind(sqlx::types::Json(labels))
        .bind(now)
        .bind(BUILTIN_LOCAL_LOCATION_ID)
        .bind(i32::try_from(feature_level).unwrap_or(i32::MAX))
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?
        .rows_affected();

        if rows == 0 {
            return Err(Error::not_found("built-in Local Probe Location"));
        }

        self.get_agent_for_scope(None, &Id::from_string(BUILTIN_LOCAL_AGENT_ID))
            .await
    }

    async fn get_agent_for_scope(
        &self,
        organization_id: Option<&Id>,
        agent_id: &Id,
    ) -> Result<ProbeAgent> {
        let row = sqlx::query(&agent_select(
            "agent.id = $1 AND location.organization_id IS NOT DISTINCT FROM $2",
        ))
        .bind(&agent_id.0)
        .bind(organization_id.map(Id::as_str))
        .fetch_one(&self.pool)
        .await
        .map_err(super::sqlx_err)?;

        row_to_agent(row)
    }
}
