// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::trace_policy::{
        PersistedTracePolicy, TraceDebugToken, TraceDebugTokenRepository, TracePolicyRepository,
    },
    shared::{Error, Result, ids::Id, tail_sampling::TraceRuntimePolicy, time::TimestampMicros},
};

pub struct PgTracePolicyRepository {
    pool: PgPool,
}

impl PgTracePolicyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_policy(row: sqlx::postgres::PgRow) -> Result<PersistedTracePolicy> {
    Ok(PersistedTracePolicy {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        system_org_id: Id(row.try_get("system_org_id").map_err(sqlx_err)?),
        policy: row
            .try_get::<Json<TraceRuntimePolicy>, _>("policy")
            .map_err(sqlx_err)?
            .0,
        created_by: row
            .try_get::<Option<String>, _>("created_by")
            .map_err(sqlx_err)?
            .map(Id),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl TracePolicyRepository for PgTracePolicyRepository {
    async fn active(&self) -> Result<Option<PersistedTracePolicy>> {
        let row = sqlx::query(
            "SELECT p.id, p.system_org_id, p.policy, p.created_by, p.created_at
             FROM instance_settings s
             JOIN trace_runtime_policies p ON p.id = s.trace_policy_id
             WHERE s.id = 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row.map(row_to_policy).transpose()
    }

    async fn history(&self) -> Result<Vec<PersistedTracePolicy>> {
        sqlx::query(
            "SELECT id, system_org_id, policy, created_by, created_at
             FROM trace_runtime_policies
             ORDER BY version DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?
        .into_iter()
        .map(row_to_policy)
        .collect()
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "trace_runtime_policies")
    )]
    async fn publish(
        &self,
        system_org_id: &Id,
        mut policy: TraceRuntimePolicy,
        actor_id: &Id,
    ) -> Result<PersistedTracePolicy> {
        policy.validate().map_err(Error::invalid)?;
        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('molesignal.trace.policy'))")
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;

        let version: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) + 1 FROM trace_runtime_policies")
                .fetch_one(&mut *transaction)
                .await
                .map_err(sqlx_err)?;

        policy.version = version.max(1) as u64;

        let persisted = PersistedTracePolicy {
            id: Id::new(),
            system_org_id: system_org_id.clone(),
            policy,
            created_by: Some(actor_id.clone()),
            created_at: TimestampMicros::now(),
        };

        sqlx::query(
            "INSERT INTO trace_runtime_policies
                (id, system_org_id, version, policy, created_by, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&persisted.id.0)
        .bind(&persisted.system_org_id.0)
        .bind(version)
        .bind(Json(&persisted.policy))
        .bind(&actor_id.0)
        .bind(persisted.created_at)
        .execute(&mut *transaction)
        .await
        .map_err(sqlx_err)?;

        sqlx::query(
            "UPDATE instance_settings
             SET trace_policy_id = $1,
                 trace_policy_activated_by = $2,
                 trace_policy_activated_at = $3
             WHERE id = 1",
        )
        .bind(&persisted.id.0)
        .bind(&actor_id.0)
        .bind(persisted.created_at)
        .execute(&mut *transaction)
        .await
        .map_err(sqlx_err)?;

        transaction.commit().await.map_err(sqlx_err)?;
        Ok(persisted)
    }
}

pub struct PgTraceDebugTokenRepository {
    pool: PgPool,
}

impl PgTraceDebugTokenRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_debug_token(row: sqlx::postgres::PgRow) -> Result<TraceDebugToken> {
    Ok(TraceDebugToken {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        token_hash: row.try_get("token_hash").map_err(sqlx_err)?,
        organization_id: row
            .try_get::<Option<String>, _>("organization_id")
            .map_err(sqlx_err)?
            .map(Id),
        route_pattern: row.try_get("route_pattern").map_err(sqlx_err)?,
        expires_at: row
            .try_get::<TimestampMicros, _>("expires_at")
            .map_err(sqlx_err)?,
        max_uses: row.try_get::<i64, _>("max_uses").map_err(sqlx_err)?.max(0) as u64,
        used_count: row
            .try_get::<i64, _>("used_count")
            .map_err(sqlx_err)?
            .max(0) as u64,
        revoked_at: row
            .try_get::<Option<TimestampMicros>, _>("revoked_at")
            .map_err(sqlx_err)?,
        created_by: Id(row.try_get("created_by").map_err(sqlx_err)?),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
    })
}

const DEBUG_COLS: &str = "id, token_hash, organization_id, route_pattern, expires_at,
    max_uses, used_count, revoked_at, created_by, created_at";

#[async_trait]
impl TraceDebugTokenRepository for PgTraceDebugTokenRepository {
    async fn list(&self) -> Result<Vec<TraceDebugToken>> {
        sqlx::query(&format!(
            "SELECT {DEBUG_COLS} FROM trace_debug_tokens ORDER BY created_at DESC"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?
        .into_iter()
        .map(row_to_debug_token)
        .collect()
    }

    async fn create(&self, token: TraceDebugToken) -> Result<TraceDebugToken> {
        sqlx::query(
            "INSERT INTO trace_debug_tokens
                (id, token_hash, organization_id, route_pattern, expires_at,
                 max_uses, used_count, revoked_at, created_by, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, 0, NULL, $7, $8)",
        )
        .bind(&token.id.0)
        .bind(&token.token_hash)
        .bind(token.organization_id.as_ref().map(|id| id.0.as_str()))
        .bind(&token.route_pattern)
        .bind(token.expires_at)
        .bind(i64::try_from(token.max_uses).unwrap_or(i64::MAX))
        .bind(&token.created_by.0)
        .bind(token.created_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(token)
    }

    async fn revoke(&self, id: &Id, revoked_at: TimestampMicros) -> Result<()> {
        let result = sqlx::query(
            "UPDATE trace_debug_tokens
             SET revoked_at = COALESCE(revoked_at, $2)
             WHERE id = $1",
        )
        .bind(&id.0)
        .bind(revoked_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if result.rows_affected() == 0 {
            return Err(Error::not_found("trace debug token"));
        }

        Ok(())
    }

    async fn consume(
        &self,
        token_hash: &str,
        organization_id: Option<&Id>,
        route: Option<&str>,
        now: TimestampMicros,
    ) -> Result<Option<TraceDebugToken>> {
        let row = sqlx::query(&format!(
            "UPDATE trace_debug_tokens
             SET used_count = used_count + 1
             WHERE token_hash = $1
               AND revoked_at IS NULL
               AND expires_at > $2
               AND used_count < max_uses
               AND (organization_id IS NULL OR organization_id = $3)
               AND (route_pattern IS NULL OR $4 LIKE route_pattern)
             RETURNING {DEBUG_COLS}"
        ))
        .bind(token_hash)
        .bind(now)
        .bind(organization_id.map(|id| id.0.as_str()))
        .bind(route)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row.map(row_to_debug_token).transpose()
    }
}
