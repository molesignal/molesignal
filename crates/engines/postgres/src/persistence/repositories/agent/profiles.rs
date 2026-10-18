// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `agent_profiles`：Agent Profile 的读写。同一组织最多一个默认配置，设新的默认时在同一个
//! 事务里取消旧的。

use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use super::{enum_string, parse_enum, sqlx_err};
use crate::{
    agent::model::{AgentProfile, NetworkAccess},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

fn profile_row(row: sqlx::postgres::PgRow) -> Result<AgentProfile> {
    let allowed_tools: Json<Vec<String>> = row.try_get("allowed_tools").map_err(sqlx_err)?;
    let data_scope: Json<Value> = row.try_get("data_scope").map_err(sqlx_err)?;
    let risk_policy: Json<Value> = row.try_get("risk_policy").map_err(sqlx_err)?;

    Ok(AgentProfile {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        description: row.try_get("description").map_err(sqlx_err)?,
        model_provider_id: row
            .try_get::<Option<String>, _>("model_provider_id")
            .map_err(sqlx_err)?
            .map(Id),
        model: row.try_get("model").map_err(sqlx_err)?,
        allowed_tools: allowed_tools.0,
        data_scope: data_scope.0,
        risk_policy: risk_policy.0,
        network_access: parse_enum::<NetworkAccess>(
            row.try_get("network_access").map_err(sqlx_err)?,
            "agent profile network access",
        )?,
        max_context_tokens: row.try_get("max_context_tokens").map_err(sqlx_err)?,
        max_investigation_secs: row.try_get("max_investigation_secs").map_err(sqlx_err)?,
        max_tool_calls: row.try_get("max_tool_calls").map_err(sqlx_err)?,
        temperature: row.try_get("temperature").map_err(sqlx_err)?,
        allow_bypass_permissions: row.try_get("allow_bypass_permissions").map_err(sqlx_err)?,
        is_default: row.try_get("is_default").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        created_by: Id(row.try_get("created_by").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

pub(super) async fn list(pool: &PgPool, org_id: &Id) -> Result<Vec<AgentProfile>> {
    let rows = sqlx::query(
        "SELECT * FROM agent_profiles
         WHERE org_id=$1 ORDER BY is_default DESC, updated_at_micros DESC",
    )
    .bind(&org_id.0)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    rows.into_iter().map(profile_row).collect()
}

pub(super) async fn get(pool: &PgPool, org_id: &Id, id: &Id) -> Result<AgentProfile> {
    sqlx::query("SELECT * FROM agent_profiles WHERE org_id=$1 AND id=$2")
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("agent profile `{}` not found", id.0)))
        .and_then(profile_row)
}

#[tracing::instrument(
    name = "db.transaction",
    skip_all,
    fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "agent_profiles")
)]
pub(super) async fn create(pool: &PgPool, item: AgentProfile) -> Result<AgentProfile> {
    let mut tx = sqlx::begin(pool).await.map_err(sqlx_err)?;

    if item.is_default {
        sqlx::query(
            "UPDATE agent_profiles SET is_default=FALSE
             WHERE org_id=$1 AND is_default=TRUE",
        )
        .bind(&item.org_id.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
    }

    sqlx::query(
        "INSERT INTO agent_profiles
         (id,org_id,name,description,model_provider_id,model,allowed_tools,data_scope,
          risk_policy,network_access,max_context_tokens,max_investigation_secs,max_tool_calls,
          temperature,is_default,enabled,created_by,created_at_micros,updated_at_micros,
          allow_bypass_permissions)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)",
    )
    .bind(&item.id.0)
    .bind(&item.org_id.0)
    .bind(&item.name)
    .bind(&item.description)
    .bind(item.model_provider_id.as_ref().map(|id| &id.0))
    .bind(&item.model)
    .bind(Json(&item.allowed_tools))
    .bind(Json(&item.data_scope))
    .bind(Json(&item.risk_policy))
    .bind(enum_string(&item.network_access))
    .bind(item.max_context_tokens)
    .bind(item.max_investigation_secs)
    .bind(item.max_tool_calls)
    .bind(item.temperature)
    .bind(item.is_default)
    .bind(item.enabled)
    .bind(&item.created_by.0)
    .bind(item.created_at.0)
    .bind(item.updated_at.0)
    .bind(item.allow_bypass_permissions)
    .execute(&mut *tx)
    .await
    .map_err(sqlx_err)?;

    tx.commit().await.map_err(sqlx_err)?;
    Ok(item)
}

#[tracing::instrument(
    name = "db.transaction",
    skip_all,
    fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "agent_profiles")
)]
pub(super) async fn update(pool: &PgPool, item: AgentProfile) -> Result<AgentProfile> {
    let mut tx = sqlx::begin(pool).await.map_err(sqlx_err)?;

    if item.is_default {
        sqlx::query(
            "UPDATE agent_profiles SET is_default=FALSE
             WHERE org_id=$1 AND id<>$2 AND is_default=TRUE",
        )
        .bind(&item.org_id.0)
        .bind(&item.id.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
    }

    let result = sqlx::query(
        "UPDATE agent_profiles SET
          name=$3,description=$4,model_provider_id=$5,model=$6,allowed_tools=$7,
          data_scope=$8,risk_policy=$9,network_access=$10,max_context_tokens=$11,
          max_investigation_secs=$12,max_tool_calls=$13,temperature=$14,is_default=$15,
          enabled=$16,updated_at_micros=$17,allow_bypass_permissions=$18
         WHERE org_id=$1 AND id=$2",
    )
    .bind(&item.org_id.0)
    .bind(&item.id.0)
    .bind(&item.name)
    .bind(&item.description)
    .bind(item.model_provider_id.as_ref().map(|id| &id.0))
    .bind(&item.model)
    .bind(Json(&item.allowed_tools))
    .bind(Json(&item.data_scope))
    .bind(Json(&item.risk_policy))
    .bind(enum_string(&item.network_access))
    .bind(item.max_context_tokens)
    .bind(item.max_investigation_secs)
    .bind(item.max_tool_calls)
    .bind(item.temperature)
    .bind(item.is_default)
    .bind(item.enabled)
    .bind(item.updated_at.0)
    .bind(item.allow_bypass_permissions)
    .execute(&mut *tx)
    .await
    .map_err(sqlx_err)?;

    if result.rows_affected() == 0 {
        return Err(Error::not_found(format!(
            "agent profile `{}` not found",
            item.id.0
        )));
    }

    tx.commit().await.map_err(sqlx_err)?;
    Ok(item)
}
