// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Enrollment tokens and the node inventory.

use domain::node_agent::{ConfigStatus, EnrollmentToken, NodeAgent, SessionReport};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde_json::Value;
use sqlx::types::Json;

use super::{
    PgNodeAgentRepository,
    rows::{NODE_COLUMNS, TOKEN_COLUMNS, end_node_tasks, node_row, token_row},
};
use crate::persistence::sqlx_err;

/// A value bound to an `UPDATE node_agents` after the organization, node and now.
pub(super) enum Bind {
    Text(String),
    Time(Option<TimestampMicros>),
    Json(Value),
}

fn node_not_found(node_id: &Id) -> Error {
    Error::not_found(format!("node `{}` not found", node_id.as_str()))
}

impl PgNodeAgentRepository {
    pub(super) async fn insert_token(
        &self,
        token: &EnrollmentToken,
        token_hash: &[u8],
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO node_agent_enrollment_tokens
                (id, org_id, token_hash, description, labels, max_uses, uses, expires_at,
                 created_by, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, 0, $7, $8, $9)",
        )
        .bind(&token.id.0)
        .bind(&token.org_id.0)
        .bind(token_hash)
        .bind(&token.description)
        .bind(Json(&token.labels))
        .bind(token.max_uses as i32)
        .bind(token.expires_at)
        .bind(&token.created_by)
        .bind(token.created_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    pub(super) async fn tokens(&self, org_id: &Id) -> Result<Vec<EnrollmentToken>> {
        sqlx::query(&format!(
            "SELECT {TOKEN_COLUMNS} FROM node_agent_enrollment_tokens
             WHERE org_id = $1 ORDER BY created_at DESC LIMIT 500"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?
        .iter()
        .map(token_row)
        .collect()
    }

    pub(super) async fn revoke_token(
        &self,
        org_id: &Id,
        token_id: &Id,
        now: TimestampMicros,
    ) -> Result<EnrollmentToken> {
        let row = sqlx::query(&format!(
            "UPDATE node_agent_enrollment_tokens SET revoked_at = COALESCE(revoked_at, $3)
             WHERE org_id = $1 AND id = $2 RETURNING {TOKEN_COLUMNS}"
        ))
        .bind(&org_id.0)
        .bind(&token_id.0)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found("enrollment token not found"))?;

        token_row(&row)
    }

    pub(super) async fn token_by_hash(&self, token_hash: &[u8]) -> Result<Option<EnrollmentToken>> {
        sqlx::query(&format!(
            "SELECT {TOKEN_COLUMNS} FROM node_agent_enrollment_tokens WHERE token_hash = $1"
        ))
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .as_ref()
        .map(token_row)
        .transpose()
    }

    pub(super) async fn enroll(
        &self,
        token_hash: &[u8],
        node: &NodeAgent,
        now: TimestampMicros,
    ) -> Result<NodeAgent> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let token_id: Option<String> = sqlx::query_scalar(
            "UPDATE node_agent_enrollment_tokens SET uses = uses + 1, last_used_at = $3
             WHERE token_hash = $1 AND org_id = $2 AND revoked_at IS NULL AND expires_at > $3
               AND uses < max_uses
             RETURNING id",
        )
        .bind(token_hash)
        .bind(&node.org_id.0)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        let Some(token_id) = token_id else {
            return Err(Error::unauthorized("invalid or expired enrollment token"));
        };

        // The same host again keeps its node id and desired configuration; its certificate,
        // revocation and drain start over.
        let row = sqlx::query(&format!(
            "INSERT INTO node_agents
                (id, org_id, host_id, boot_id, hostname, labels, agent_version, protocol_version,
                 capabilities, certificate_serial, certificate_expires_at, enrollment_token_id,
                 enrolled_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $13, $13)
             ON CONFLICT (org_id, host_id) DO UPDATE SET
                boot_id = EXCLUDED.boot_id,
                hostname = EXCLUDED.hostname,
                labels = EXCLUDED.labels,
                agent_version = EXCLUDED.agent_version,
                protocol_version = EXCLUDED.protocol_version,
                capabilities = EXCLUDED.capabilities,
                certificate_serial = EXCLUDED.certificate_serial,
                previous_certificate_serial = NULL,
                certificate_expires_at = EXCLUDED.certificate_expires_at,
                enrollment_token_id = EXCLUDED.enrollment_token_id,
                drain_requested_at = NULL, drain_deadline = NULL, drained_at = NULL,
                revoked_at = NULL, revoke_reason = NULL,
                enrolled_at = EXCLUDED.enrolled_at,
                updated_at = EXCLUDED.updated_at
             RETURNING {NODE_COLUMNS}"
        ))
        .bind(&node.id.0)
        .bind(&node.org_id.0)
        .bind(&node.host_id)
        .bind(&node.boot_id)
        .bind(&node.hostname)
        .bind(Json(&node.labels))
        .bind(&node.agent_version)
        .bind(&node.protocol_version)
        .bind(Json(&node.capabilities))
        .bind(&node.certificate_serial)
        .bind(node.certificate_expires_at)
        .bind(&token_id)
        .bind(now)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        let stored = node_row(&row)?;
        end_node_tasks(
            &mut tx,
            &stored.org_id,
            &stored.id,
            "the node enrolled again",
            now,
        )
        .await?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok(stored)
    }

    pub(super) async fn node_where(
        &self,
        condition: &str,
        values: &[&str],
    ) -> Result<Option<NodeAgent>> {
        let sql = format!("SELECT {NODE_COLUMNS} FROM node_agents WHERE {condition}");
        let mut query = sqlx::query(&sql);

        for value in values {
            query = query.bind(*value);
        }

        query
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .as_ref()
            .map(node_row)
            .transpose()
    }

    pub(super) async fn nodes(&self, org_id: &Id) -> Result<Vec<NodeAgent>> {
        sqlx::query(&format!(
            "SELECT {NODE_COLUMNS} FROM node_agents WHERE org_id = $1
             ORDER BY hostname, host_id LIMIT 10000"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?
        .iter()
        .map(node_row)
        .collect()
    }

    pub(super) async fn session(
        &self,
        org_id: &Id,
        node_id: &Id,
        report: &SessionReport,
        now: TimestampMicros,
    ) -> Result<NodeAgent> {
        let row = sqlx::query(&format!(
            "UPDATE node_agents SET
                agent_version = COALESCE($3, agent_version),
                protocol_version = COALESCE($4, protocol_version),
                capabilities = COALESCE($5::JSONB, capabilities),
                applied_config_revision = $6,
                health = COALESCE($7::JSONB, health),
                collector_states = COALESCE($8::JSONB, collector_states),
                last_heartbeat_at = $9,
                last_connected_at = CASE WHEN $10 THEN $9 ELSE last_connected_at END,
                previous_certificate_serial =
                    CASE WHEN $11 THEN NULL ELSE previous_certificate_serial END,
                updated_at = $9
             WHERE org_id = $1 AND id = $2 AND revoked_at IS NULL
             RETURNING {NODE_COLUMNS}"
        ))
        .bind(&org_id.0)
        .bind(&node_id.0)
        .bind(&report.agent_version)
        .bind(&report.protocol_version)
        .bind(report.capabilities.as_ref().map(Json))
        .bind(i64::try_from(report.applied_config_revision).unwrap_or(i64::MAX))
        .bind(report.health.as_ref().map(Json))
        .bind(report.collector_states.as_ref().map(Json))
        .bind(now)
        .bind(report.connected)
        .bind(report.certificate_confirmed)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| node_not_found(node_id))?;

        node_row(&row)
    }

    /// Runs an `UPDATE node_agents SET <assignments>` on a node that is not revoked. `$1` is the
    /// organization, `$2` the node, `$3` now; `extra` binds `$4` onwards.
    pub(super) async fn update_node(
        &self,
        org_id: &Id,
        node_id: &Id,
        now: TimestampMicros,
        assignments: &str,
        extra: Vec<Bind>,
    ) -> Result<NodeAgent> {
        let sql = format!(
            "UPDATE node_agents SET {assignments}, updated_at = $3
             WHERE org_id = $1 AND id = $2 AND revoked_at IS NULL
             RETURNING {NODE_COLUMNS}"
        );

        let mut query = sqlx::query(&sql).bind(&org_id.0).bind(&node_id.0).bind(now);

        for value in extra {
            query = match value {
                Bind::Text(value) => query.bind(value),
                Bind::Time(value) => query.bind(value),
                Bind::Json(value) => query.bind(Json(value)),
            };
        }

        let row = query
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| node_not_found(node_id))?;

        node_row(&row)
    }

    pub(super) async fn revoke(
        &self,
        org_id: &Id,
        node_id: &Id,
        reason: &str,
        now: TimestampMicros,
    ) -> Result<(NodeAgent, u64)> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let row = sqlx::query(&format!(
            "UPDATE node_agents
             SET revoked_at = COALESCE(revoked_at, $3),
                 revoke_reason = COALESCE(revoke_reason, $4),
                 updated_at = $3
             WHERE org_id = $1 AND id = $2
             RETURNING {NODE_COLUMNS}"
        ))
        .bind(&org_id.0)
        .bind(&node_id.0)
        .bind(now)
        .bind(reason)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| node_not_found(node_id))?;

        let ended = end_node_tasks(&mut tx, org_id, node_id, "the node was revoked", now).await?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok((node_row(&row)?, ended))
    }

    pub(super) async fn config_status(
        &self,
        org_id: &Id,
        node_id: &Id,
        status: &ConfigStatus,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE node_agents SET config_status = $3
             WHERE org_id = $1 AND id = $2 AND revoked_at IS NULL",
        )
        .bind(&org_id.0)
        .bind(&node_id.0)
        .bind(Json(status))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }
}
