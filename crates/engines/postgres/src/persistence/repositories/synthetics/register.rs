// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;

use super::PgSyntheticRepository;
use crate::{
    domain::synthetics::{
        ProbeAgent, ProbeRegisterToken, ProbeRegistrationGrant, SyntheticAgentRepository,
        SyntheticRegisterRepository,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[async_trait]
impl SyntheticRegisterRepository for PgSyntheticRepository {
    async fn create_register_token(
        &self,
        token: ProbeRegisterToken,
        token_hash: Vec<u8>,
    ) -> Result<ProbeRegisterToken> {
        let rows = sqlx::query(
            "INSERT INTO synthetic_probe_register_tokens
                (id, location_id, token_hash, expires_at_micros, used_at_micros,
                 used_by_agent_id, created_by, created_at_micros)
             SELECT $1, location.id, $4, $5, NULL, NULL, $6, $7
             FROM synthetic_probe_locations location
             WHERE location.id = $3 AND location.organization_id = $2
               AND location.scope = 'organization' AND location.execution = 'agent_pool'
               AND location.lifecycle = 'active'",
        )
        .bind(&token.id.0)
        .bind(&token.organization_id.0)
        .bind(&token.location_id.0)
        .bind(token_hash)
        .bind(token.expires_at.0)
        .bind(&token.created_by.0)
        .bind(token.created_at.0)
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?
        .rows_affected();
        if rows == 0 {
            return Err(Error::not_found("organization AgentPool Location"));
        }
        Ok(token)
    }

    async fn get_registration_grant(
        &self,
        token_hash: &[u8],
        now: TimestampMicros,
    ) -> Result<ProbeRegistrationGrant> {
        use sqlx::Row;

        let row = sqlx::query(
            "SELECT grant.organization_id, grant.location_id
             FROM (
                 SELECT location.organization_id, token.location_id, 1 AS priority
                 FROM synthetic_probe_register_tokens token
                 JOIN synthetic_probe_locations location ON location.id = token.location_id
                 WHERE token.token_hash = $1 AND token.used_at_micros IS NULL
                   AND token.expires_at_micros > $2 AND location.organization_id IS NOT NULL
                   AND location.lifecycle = 'active' AND location.execution = 'agent_pool'
                 UNION ALL
                 SELECT token.organization_id, token.location_id, 2 AS priority
                 FROM synthetic_probe_agent_tokens token
                 JOIN synthetic_probe_locations location
                   ON location.organization_id = token.organization_id
                  AND location.id = token.location_id
                 WHERE token.token_hash = $1 AND token.status = 'active'
                   AND (token.expires_at_micros IS NULL OR token.expires_at_micros > $2)
                   AND location.lifecycle = 'active' AND location.execution = 'agent_pool'
             ) grant
             ORDER BY grant.priority
             LIMIT 1",
        )
        .bind(token_hash)
        .bind(now.0)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| match error {
            sqlx::Error::RowNotFound => {
                Error::unauthorized("invalid, expired, disabled, or already-used Probe credential")
            }
            other => super::sqlx_err(other),
        })?;
        Ok(ProbeRegistrationGrant {
            organization_id: Id(row.try_get("organization_id").map_err(super::sqlx_err)?),
            location_id: Id(row.try_get("location_id").map_err(super::sqlx_err)?),
        })
    }

    async fn register_probe(
        &self,
        token_hash: &[u8],
        agent: ProbeAgent,
        consumed_at: TimestampMicros,
    ) -> Result<ProbeAgent> {
        let Some(org_id) = agent.organization_id.as_ref() else {
            return Err(Error::forbidden(
                "organization register tokens cannot create platform Agents",
            ));
        };
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let register_token_id: Option<String> = sqlx::query_scalar(
            "SELECT token.id
             FROM synthetic_probe_register_tokens token
             JOIN synthetic_probe_locations location ON location.id = token.location_id
             WHERE token.token_hash = $1 AND token.used_at_micros IS NULL
               AND token.expires_at_micros > $2 AND token.location_id = $3
               AND location.organization_id = $4 AND location.lifecycle = 'active'
               AND location.execution = 'agent_pool'
             FOR UPDATE OF token",
        )
        .bind(token_hash)
        .bind(consumed_at.0)
        .bind(&agent.location_id.0)
        .bind(&org_id.0)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        let agent_token_id: Option<String> = if register_token_id.is_none() {
            sqlx::query_scalar(
                "SELECT token.id
                 FROM synthetic_probe_agent_tokens token
                 JOIN synthetic_probe_locations location
                   ON location.organization_id = token.organization_id
                  AND location.id = token.location_id
                 WHERE token.token_hash = $1 AND token.status = 'active'
                   AND (token.expires_at_micros IS NULL OR token.expires_at_micros > $2)
                   AND token.location_id = $3 AND token.organization_id = $4
                   AND location.lifecycle = 'active' AND location.execution = 'agent_pool'
                 FOR UPDATE OF token",
            )
            .bind(token_hash)
            .bind(consumed_at.0)
            .bind(&agent.location_id.0)
            .bind(&org_id.0)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?
        } else {
            None
        };
        if register_token_id.is_none() && agent_token_id.is_none() {
            return Err(Error::unauthorized(
                "invalid, expired, disabled, or already-used Probe credential",
            ));
        }
        sqlx::query(
            "INSERT INTO synthetic_probe_agents
                (id, location_id, name, hostname, status, agent_version, protocol_version,
                 capabilities, capacity, labels, public_key_der, certificate_serial,
                 certificate_expires_at_micros, last_heartbeat_at_micros,
                 last_result_sequence, revoked_at_micros, created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, NULL,
                     0, NULL, $14, $14)",
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
                .map(|capability| capability.as_str().to_string())
                .collect::<Vec<_>>(),
        )
        .bind(sqlx::types::Json(&agent.capacity))
        .bind(sqlx::types::Json(&agent.labels))
        .bind(&agent.public_key_der)
        .bind(&agent.certificate_serial)
        .bind(agent.certificate_expires_at.map(|value| value.0))
        .bind(consumed_at.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        if let Some(token_id) = register_token_id {
            sqlx::query(
                "UPDATE synthetic_probe_register_tokens
                 SET used_at_micros = $2, used_by_agent_id = $3 WHERE id = $1",
            )
            .bind(token_id)
            .bind(consumed_at.0)
            .bind(&agent.id.0)
            .execute(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;
        } else if let Some(token_id) = agent_token_id {
            sqlx::query(
                "UPDATE synthetic_probe_agent_tokens
                 SET last_used_at_micros = $2, updated_at_micros = $2 WHERE id = $1",
            )
            .bind(token_id)
            .bind(consumed_at.0)
            .execute(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;
        }
        transaction.commit().await.map_err(super::sqlx_err)?;
        self.get_agent(org_id, &agent.id).await
    }
}
