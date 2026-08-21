// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::Row as _;

use super::PgSyntheticRepository;
use crate::{
    domain::synthetics::{ProbeAgentToken, ProbeAgentTokenStatus, SyntheticAgentTokenRepository},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const TOKEN_COLS: &str = "token.id, token.organization_id, token.location_id, token.name,
    token.token_prefix, token.status, token.expires_at_micros, token.last_used_at_micros,
    token.created_by, token.created_at_micros, token.rotated_at_micros,
    token.disabled_at_micros, token.updated_at_micros";

pub(super) fn row_to_agent_token(row: sqlx::postgres::PgRow) -> Result<ProbeAgentToken> {
    let status: String = row.try_get("status").map_err(super::sqlx_err)?;
    Ok(ProbeAgentToken {
        id: Id(row.try_get("id").map_err(super::sqlx_err)?),
        organization_id: Id(row.try_get("organization_id").map_err(super::sqlx_err)?),
        location_id: Id(row.try_get("location_id").map_err(super::sqlx_err)?),
        name: row.try_get("name").map_err(super::sqlx_err)?,
        token_prefix: row.try_get("token_prefix").map_err(super::sqlx_err)?,
        status: ProbeAgentTokenStatus::parse(&status).ok_or_else(|| {
            Error::internal(format!("unknown Probe Agent Token status: {status}"))
        })?,
        expires_at: row
            .try_get::<Option<i64>, _>("expires_at_micros")
            .map_err(super::sqlx_err)?
            .map(TimestampMicros),
        last_used_at: row
            .try_get::<Option<i64>, _>("last_used_at_micros")
            .map_err(super::sqlx_err)?
            .map(TimestampMicros),
        created_by: Id(row.try_get("created_by").map_err(super::sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(super::sqlx_err)?),
        rotated_at: row
            .try_get::<Option<i64>, _>("rotated_at_micros")
            .map_err(super::sqlx_err)?
            .map(TimestampMicros),
        disabled_at: row
            .try_get::<Option<i64>, _>("disabled_at_micros")
            .map_err(super::sqlx_err)?
            .map(TimestampMicros),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(super::sqlx_err)?),
    })
}

#[async_trait]
impl SyntheticAgentTokenRepository for PgSyntheticRepository {
    async fn create_agent_token(
        &self,
        token: ProbeAgentToken,
        token_hash: Vec<u8>,
    ) -> Result<ProbeAgentToken> {
        let query = format!(
            "INSERT INTO synthetic_probe_agent_tokens AS token
                (id, organization_id, location_id, name, token_hash, token_prefix, status,
                 expires_at_micros, last_used_at_micros, created_by, created_at_micros,
                 rotated_at_micros, disabled_at_micros, updated_at_micros)
             SELECT $1, location.organization_id, location.id, $4, $5, $6, 'active', $7,
                    NULL, $8, $9, NULL, NULL, $9
             FROM synthetic_probe_locations location
             WHERE location.organization_id = $2 AND location.id = $3
               AND location.scope = 'organization' AND location.execution = 'agent_pool'
               AND location.lifecycle = 'active'
             RETURNING {TOKEN_COLS}"
        );
        let row = sqlx::query(&query)
            .bind(&token.id.0)
            .bind(&token.organization_id.0)
            .bind(&token.location_id.0)
            .bind(&token.name)
            .bind(token_hash)
            .bind(&token.token_prefix)
            .bind(token.expires_at.map(|value| value.0))
            .bind(&token.created_by.0)
            .bind(token.created_at.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .ok_or_else(|| Error::not_found("active organization AgentPool Location"))?;
        row_to_agent_token(row)
    }

    async fn list_agent_tokens(&self, org_id: &Id) -> Result<Vec<ProbeAgentToken>> {
        let query = format!(
            "SELECT {TOKEN_COLS} FROM synthetic_probe_agent_tokens token
             WHERE token.organization_id = $1
             ORDER BY token.created_at_micros DESC, token.id DESC"
        );
        sqlx::query(&query)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(row_to_agent_token)
            .collect()
    }

    async fn rotate_agent_token(
        &self,
        org_id: &Id,
        token_id: &Id,
        token_hash: Vec<u8>,
        token_prefix: &str,
        expires_at: Option<TimestampMicros>,
        rotated_at: TimestampMicros,
    ) -> Result<ProbeAgentToken> {
        let query = format!(
            "UPDATE synthetic_probe_agent_tokens AS token
             SET token_hash = $3, token_prefix = $4, status = 'active',
                 expires_at_micros = $5, rotated_at_micros = $6,
                 disabled_at_micros = NULL, updated_at_micros = $6
             WHERE token.organization_id = $1 AND token.id = $2
             RETURNING {TOKEN_COLS}"
        );
        let row = sqlx::query(&query)
            .bind(&org_id.0)
            .bind(&token_id.0)
            .bind(token_hash)
            .bind(token_prefix)
            .bind(expires_at.map(|value| value.0))
            .bind(rotated_at.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .ok_or_else(|| Error::not_found("Probe Agent Token"))?;
        row_to_agent_token(row)
    }

    async fn disable_agent_token(
        &self,
        org_id: &Id,
        token_id: &Id,
        disabled_at: TimestampMicros,
    ) -> Result<ProbeAgentToken> {
        let query = format!(
            "UPDATE synthetic_probe_agent_tokens AS token
             SET status = 'disabled', disabled_at_micros = COALESCE(disabled_at_micros, $3),
                 updated_at_micros = $3
             WHERE token.organization_id = $1 AND token.id = $2
             RETURNING {TOKEN_COLS}"
        );
        let row = sqlx::query(&query)
            .bind(&org_id.0)
            .bind(&token_id.0)
            .bind(disabled_at.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .ok_or_else(|| Error::not_found("Probe Agent Token"))?;
        row_to_agent_token(row)
    }
}
