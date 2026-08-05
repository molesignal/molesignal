// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence 工具策略与 MCP Server 的 PostgreSQL 实装。
//!
//! MCP 凭据与可序列化元数据分表保存，并使用 [`CipherRootKey`] seal。任何读取列表 /
//! 详情的路径都只返回 `credential_set` 与后四位；明文仅供 MCP runtime 即时构造请求。

use async_trait::async_trait;
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::{
    infra::cipher::CipherRootKey,
    intelligence::{
        model::RiskLevel,
        tool_control::{
            ManagedToolStatus, McpServer, McpServerInput, McpServerRuntime, McpTool,
            ToolControlRepository, ToolExecutionMode, ToolPolicy, ToolPolicyDefaults,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgToolControlRepository {
    pool: PgPool,
    cipher: CipherRootKey,
}

impl PgToolControlRepository {
    pub fn new(pool: PgPool, cipher: CipherRootKey) -> Self {
        Self { pool, cipher }
    }

    async fn seal_server_secret(
        &self,
        org_id: &Id,
        server_id: &Id,
        credential: &str,
    ) -> Result<()> {
        let (nonce, ciphertext) = self
            .cipher
            .seal(credential.as_bytes())
            .map_err(|error| Error::internal(format!("MCP credential seal: {error}")))?;
        let now = TimestampMicros::now();
        sqlx::query(
            "INSERT INTO intelligence_mcp_server_secrets
                (server_id,org_id,ciphertext,nonce,created_at_micros,updated_at_micros)
             VALUES ($1,$2,$3,$4,$5,$5)
             ON CONFLICT (server_id) DO UPDATE SET
                org_id=EXCLUDED.org_id,ciphertext=EXCLUDED.ciphertext,nonce=EXCLUDED.nonce,
                updated_at_micros=EXCLUDED.updated_at_micros",
        )
        .bind(&server_id.0)
        .bind(&org_id.0)
        .bind(ciphertext)
        .bind(nonce)
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn plaintext_server_secret(&self, org_id: &Id, server_id: &Id) -> Result<Option<String>> {
        let row = sqlx::query(
            "SELECT ciphertext,nonce FROM intelligence_mcp_server_secrets
             WHERE org_id=$1 AND server_id=$2",
        )
        .bind(&org_id.0)
        .bind(&server_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let ciphertext: Vec<u8> = row.try_get("ciphertext").map_err(sqlx_err)?;
        let nonce: Vec<u8> = row.try_get("nonce").map_err(sqlx_err)?;
        let plaintext = self
            .cipher
            .open(&nonce, &ciphertext)
            .map_err(|error| Error::internal(format!("MCP credential open: {error}")))?;
        String::from_utf8(plaintext)
            .map(Some)
            .map_err(|error| Error::internal(format!("MCP credential is not UTF-8: {error}")))
    }
}

const POLICY_COLS: &str = "org_id,tool_name,enabled,execution_mode,environment_overrides,\
    timeout_ms,max_calls_per_run,max_response_bytes,updated_by,created_at_micros,updated_at_micros";
const DEFAULTS_COLS: &str = "org_id,risk_modes,environment_overrides,updated_by,\
    created_at_micros,updated_at_micros";
const SERVER_COLS: &str = "id,org_id,name,transport,endpoint_url,command_template,auth_type,\
    auth_header,credential_last4,credential_set,private_only,allowed_domains,allowed_cidrs,\
    follow_redirects,tls_verify,timeout_ms,max_response_bytes,enabled,status,last_error,\
    last_tested_at_micros,last_synced_at_micros,created_by,created_at_micros,updated_at_micros";
const MCP_TOOL_COLS: &str = "id,org_id,server_id,remote_name,name,display_name,description,\
    input_schema,schema_hash,schema_dialect,schema_synced_at_micros,unavailable_diagnostic,\
    output_schema,minimum_risk,risk,execution_mode,capabilities,tags,enabled,status,\
    version,last_synced_at_micros,created_at_micros,updated_at_micros";

fn parse_execution_mode(value: &str) -> Result<ToolExecutionMode> {
    match value {
        "automatic" => Ok(ToolExecutionMode::Automatic),
        "confirmation" => Ok(ToolExecutionMode::Confirmation),
        "single_approval" => Ok(ToolExecutionMode::SingleApproval),
        "dual_approval" => Ok(ToolExecutionMode::DualApproval),
        "disabled" => Ok(ToolExecutionMode::Disabled),
        other => Err(Error::internal(format!(
            "invalid stored tool execution mode `{other}`"
        ))),
    }
}

fn execution_mode_string(value: ToolExecutionMode) -> &'static str {
    match value {
        ToolExecutionMode::Automatic => "automatic",
        ToolExecutionMode::Confirmation => "confirmation",
        ToolExecutionMode::SingleApproval => "single_approval",
        ToolExecutionMode::DualApproval => "dual_approval",
        ToolExecutionMode::Disabled => "disabled",
    }
}

fn parse_risk(value: &str) -> Result<RiskLevel> {
    match value {
        "l0" => Ok(RiskLevel::L0),
        "l1" => Ok(RiskLevel::L1),
        "l2" => Ok(RiskLevel::L2),
        "l3" => Ok(RiskLevel::L3),
        "l4" => Ok(RiskLevel::L4),
        other => Err(Error::internal(format!("invalid stored risk `{other}`"))),
    }
}

fn risk_string(value: RiskLevel) -> &'static str {
    match value {
        RiskLevel::L0 => "l0",
        RiskLevel::L1 => "l1",
        RiskLevel::L2 => "l2",
        RiskLevel::L3 => "l3",
        RiskLevel::L4 => "l4",
    }
}

fn parse_status(value: &str) -> Result<ManagedToolStatus> {
    match value {
        "healthy" => Ok(ManagedToolStatus::Healthy),
        "degraded" => Ok(ManagedToolStatus::Degraded),
        "unavailable" => Ok(ManagedToolStatus::Unavailable),
        "disabled" => Ok(ManagedToolStatus::Disabled),
        other => Err(Error::internal(format!(
            "invalid stored managed tool status `{other}`"
        ))),
    }
}

fn status_string(value: ManagedToolStatus) -> &'static str {
    match value {
        ManagedToolStatus::Healthy => "healthy",
        ManagedToolStatus::Degraded => "degraded",
        ManagedToolStatus::Unavailable => "unavailable",
        ManagedToolStatus::Disabled => "disabled",
    }
}

fn json_string_vec(row: &sqlx::postgres::PgRow, column: &str) -> Result<Vec<String>> {
    let value: Json<Value> = row.try_get(column).map_err(sqlx_err)?;
    serde_json::from_value(value.0)
        .map_err(|error| Error::internal(format!("invalid stored `{column}`: {error}")))
}

fn policy_row(row: sqlx::postgres::PgRow) -> Result<ToolPolicy> {
    let overrides: Json<Value> = row.try_get("environment_overrides").map_err(sqlx_err)?;
    Ok(ToolPolicy {
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        tool_name: row.try_get("tool_name").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        execution_mode: parse_execution_mode(
            row.try_get::<String, _>("execution_mode")
                .map_err(sqlx_err)?
                .as_str(),
        )?,
        environment_overrides: overrides.0,
        timeout_ms: row.try_get("timeout_ms").map_err(sqlx_err)?,
        max_calls_per_run: row.try_get("max_calls_per_run").map_err(sqlx_err)?,
        max_response_bytes: row.try_get("max_response_bytes").map_err(sqlx_err)?,
        updated_by: Id(row.try_get("updated_by").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

fn defaults_row(row: sqlx::postgres::PgRow) -> Result<ToolPolicyDefaults> {
    let risk_modes: Json<Value> = row.try_get("risk_modes").map_err(sqlx_err)?;
    let environment_overrides: Json<Value> =
        row.try_get("environment_overrides").map_err(sqlx_err)?;
    Ok(ToolPolicyDefaults {
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        risk_modes: risk_modes.0,
        environment_overrides: environment_overrides.0,
        updated_by: Id(row.try_get("updated_by").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

fn server_row(row: sqlx::postgres::PgRow) -> Result<McpServer> {
    Ok(McpServer {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        transport: row.try_get("transport").map_err(sqlx_err)?,
        endpoint_url: row.try_get("endpoint_url").map_err(sqlx_err)?,
        command_template: row.try_get("command_template").map_err(sqlx_err)?,
        auth_type: row.try_get("auth_type").map_err(sqlx_err)?,
        auth_header: row.try_get("auth_header").map_err(sqlx_err)?,
        credential_last4: row.try_get("credential_last4").map_err(sqlx_err)?,
        credential_set: row.try_get("credential_set").map_err(sqlx_err)?,
        private_only: row.try_get("private_only").map_err(sqlx_err)?,
        allowed_domains: json_string_vec(&row, "allowed_domains")?,
        allowed_cidrs: json_string_vec(&row, "allowed_cidrs")?,
        follow_redirects: row.try_get("follow_redirects").map_err(sqlx_err)?,
        tls_verify: row.try_get("tls_verify").map_err(sqlx_err)?,
        timeout_ms: row.try_get("timeout_ms").map_err(sqlx_err)?,
        max_response_bytes: row.try_get("max_response_bytes").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        status: row.try_get("status").map_err(sqlx_err)?,
        last_error: row.try_get("last_error").map_err(sqlx_err)?,
        last_tested_at: row
            .try_get::<Option<i64>, _>("last_tested_at_micros")
            .map_err(sqlx_err)?
            .map(TimestampMicros),
        last_synced_at: row
            .try_get::<Option<i64>, _>("last_synced_at_micros")
            .map_err(sqlx_err)?
            .map(TimestampMicros),
        created_by: Id(row.try_get("created_by").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

fn mcp_tool_row(row: sqlx::postgres::PgRow) -> Result<McpTool> {
    let input_schema: Json<Value> = row.try_get("input_schema").map_err(sqlx_err)?;
    let output_schema: Option<Json<Value>> = row.try_get("output_schema").map_err(sqlx_err)?;
    let capabilities: Json<Value> = row.try_get("capabilities").map_err(sqlx_err)?;
    Ok(McpTool {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        server_id: Id(row.try_get("server_id").map_err(sqlx_err)?),
        remote_name: row.try_get("remote_name").map_err(sqlx_err)?,
        name: row.try_get("name").map_err(sqlx_err)?,
        display_name: row.try_get("display_name").map_err(sqlx_err)?,
        description: row.try_get("description").map_err(sqlx_err)?,
        input_schema: input_schema.0,
        schema_hash: row.try_get("schema_hash").map_err(sqlx_err)?,
        schema_dialect: row.try_get("schema_dialect").map_err(sqlx_err)?,
        schema_synced_at: TimestampMicros(
            row.try_get("schema_synced_at_micros").map_err(sqlx_err)?,
        ),
        unavailable_diagnostic: row.try_get("unavailable_diagnostic").map_err(sqlx_err)?,
        output_schema: output_schema.map(|value| value.0),
        minimum_risk: parse_risk(
            row.try_get::<String, _>("minimum_risk")
                .map_err(sqlx_err)?
                .as_str(),
        )?,
        risk: parse_risk(row.try_get::<String, _>("risk").map_err(sqlx_err)?.as_str())?,
        execution_mode: parse_execution_mode(
            row.try_get::<String, _>("execution_mode")
                .map_err(sqlx_err)?
                .as_str(),
        )?,
        capabilities: capabilities.0,
        tags: json_string_vec(&row, "tags")?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        status: parse_status(
            row.try_get::<String, _>("status")
                .map_err(sqlx_err)?
                .as_str(),
        )?,
        version: row.try_get("version").map_err(sqlx_err)?,
        last_synced_at: TimestampMicros(row.try_get("last_synced_at_micros").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

fn credential_last4(value: &str) -> String {
    let characters: Vec<char> = value.chars().collect();
    characters[characters.len().saturating_sub(4)..]
        .iter()
        .collect()
}

#[async_trait]
impl ToolControlRepository for PgToolControlRepository {
    async fn list_policies(&self, org_id: &Id) -> Result<Vec<ToolPolicy>> {
        let rows = sqlx::query(&format!(
            "SELECT {POLICY_COLS} FROM intelligence_tool_policies
             WHERE org_id=$1 ORDER BY tool_name"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(policy_row).collect()
    }

    async fn get_policy(&self, org_id: &Id, tool_name: &str) -> Result<Option<ToolPolicy>> {
        let row = sqlx::query(&format!(
            "SELECT {POLICY_COLS} FROM intelligence_tool_policies
             WHERE org_id=$1 AND tool_name=$2"
        ))
        .bind(&org_id.0)
        .bind(tool_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row.map(policy_row).transpose()
    }

    async fn upsert_policy(&self, policy: ToolPolicy) -> Result<ToolPolicy> {
        sqlx::query(
            "INSERT INTO intelligence_tool_policies
                (org_id,tool_name,enabled,execution_mode,environment_overrides,timeout_ms,
                 max_calls_per_run,max_response_bytes,updated_by,created_at_micros,updated_at_micros)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
             ON CONFLICT (org_id,tool_name) DO UPDATE SET
                enabled=EXCLUDED.enabled,execution_mode=EXCLUDED.execution_mode,
                environment_overrides=EXCLUDED.environment_overrides,
                timeout_ms=EXCLUDED.timeout_ms,max_calls_per_run=EXCLUDED.max_calls_per_run,
                max_response_bytes=EXCLUDED.max_response_bytes,updated_by=EXCLUDED.updated_by,
                updated_at_micros=EXCLUDED.updated_at_micros",
        )
        .bind(&policy.org_id.0)
        .bind(&policy.tool_name)
        .bind(policy.enabled)
        .bind(execution_mode_string(policy.execution_mode))
        .bind(Json(&policy.environment_overrides))
        .bind(policy.timeout_ms)
        .bind(policy.max_calls_per_run)
        .bind(policy.max_response_bytes)
        .bind(&policy.updated_by.0)
        .bind(policy.created_at.0)
        .bind(policy.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        self.get_policy(&policy.org_id, &policy.tool_name)
            .await?
            .ok_or_else(|| Error::internal("saved tool policy disappeared"))
    }

    async fn get_policy_defaults(&self, org_id: &Id) -> Result<Option<ToolPolicyDefaults>> {
        let row = sqlx::query(&format!(
            "SELECT {DEFAULTS_COLS} FROM intelligence_tool_policy_defaults WHERE org_id=$1"
        ))
        .bind(&org_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row.map(defaults_row).transpose()
    }

    async fn upsert_policy_defaults(
        &self,
        defaults: ToolPolicyDefaults,
    ) -> Result<ToolPolicyDefaults> {
        sqlx::query(
            "INSERT INTO intelligence_tool_policy_defaults
                (org_id,risk_modes,environment_overrides,updated_by,created_at_micros,updated_at_micros)
             VALUES ($1,$2,$3,$4,$5,$6)
             ON CONFLICT (org_id) DO UPDATE SET
                risk_modes=EXCLUDED.risk_modes,
                environment_overrides=EXCLUDED.environment_overrides,
                updated_by=EXCLUDED.updated_by,
                updated_at_micros=EXCLUDED.updated_at_micros",
        )
        .bind(&defaults.org_id.0)
        .bind(Json(&defaults.risk_modes))
        .bind(Json(&defaults.environment_overrides))
        .bind(&defaults.updated_by.0)
        .bind(defaults.created_at.0)
        .bind(defaults.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        self.get_policy_defaults(&defaults.org_id)
            .await?
            .ok_or_else(|| Error::internal("saved tool policy defaults disappeared"))
    }

    async fn list_mcp_servers(&self, org_id: &Id) -> Result<Vec<McpServer>> {
        let rows = sqlx::query(&format!(
            "SELECT {SERVER_COLS} FROM intelligence_mcp_servers
             WHERE org_id=$1 ORDER BY updated_at_micros DESC,name"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(server_row).collect()
    }

    async fn get_mcp_server(&self, org_id: &Id, id: &Id) -> Result<McpServer> {
        let row = sqlx::query(&format!(
            "SELECT {SERVER_COLS} FROM intelligence_mcp_servers WHERE org_id=$1 AND id=$2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("MCP Server `{}` not found", id.0)))?;
        server_row(row)
    }

    async fn get_mcp_server_runtime(&self, org_id: &Id, id: &Id) -> Result<McpServerRuntime> {
        Ok(McpServerRuntime {
            server: self.get_mcp_server(org_id, id).await?,
            credential: self.plaintext_server_secret(org_id, id).await?,
        })
    }

    async fn create_mcp_server(
        &self,
        input: McpServerInput,
        credential: Option<&str>,
    ) -> Result<McpServer> {
        let now = TimestampMicros::now();
        sqlx::query(
            "INSERT INTO intelligence_mcp_servers
                (id,org_id,name,transport,endpoint_url,command_template,auth_type,auth_header,
                 credential_last4,credential_set,private_only,allowed_domains,allowed_cidrs,
                 follow_redirects,tls_verify,timeout_ms,max_response_bytes,enabled,status,
                 created_by,created_at_micros,updated_at_micros)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,
                     'unavailable',$19,$20,$20)",
        )
        .bind(&input.id.0)
        .bind(&input.org_id.0)
        .bind(&input.name)
        .bind(&input.transport)
        .bind(&input.endpoint_url)
        .bind(&input.command_template)
        .bind(&input.auth_type)
        .bind(&input.auth_header)
        .bind(credential.map(credential_last4))
        .bind(credential.is_some())
        .bind(input.private_only)
        .bind(Json(&input.allowed_domains))
        .bind(Json(&input.allowed_cidrs))
        .bind(input.follow_redirects)
        .bind(input.tls_verify)
        .bind(input.timeout_ms)
        .bind(input.max_response_bytes)
        .bind(input.enabled)
        .bind(&input.created_by.0)
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if let Some(credential) = credential {
            self.seal_server_secret(&input.org_id, &input.id, credential)
                .await?;
        }
        self.get_mcp_server(&input.org_id, &input.id).await
    }

    async fn update_mcp_server(
        &self,
        input: McpServerInput,
        credential: Option<&str>,
    ) -> Result<McpServer> {
        let now = TimestampMicros::now();
        let result = sqlx::query(
            "UPDATE intelligence_mcp_servers SET
                name=$3,transport=$4,endpoint_url=$5,command_template=$6,auth_type=$7,
                auth_header=$8,private_only=$9,allowed_domains=$10,allowed_cidrs=$11,
                follow_redirects=$12,tls_verify=$13,timeout_ms=$14,max_response_bytes=$15,
                enabled=$16,updated_at_micros=$17
             WHERE org_id=$1 AND id=$2",
        )
        .bind(&input.org_id.0)
        .bind(&input.id.0)
        .bind(&input.name)
        .bind(&input.transport)
        .bind(&input.endpoint_url)
        .bind(&input.command_template)
        .bind(&input.auth_type)
        .bind(&input.auth_header)
        .bind(input.private_only)
        .bind(Json(&input.allowed_domains))
        .bind(Json(&input.allowed_cidrs))
        .bind(input.follow_redirects)
        .bind(input.tls_verify)
        .bind(input.timeout_ms)
        .bind(input.max_response_bytes)
        .bind(input.enabled)
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found(format!(
                "MCP Server `{}` not found",
                input.id.0
            )));
        }
        if let Some(credential) = credential {
            self.seal_server_secret(&input.org_id, &input.id, credential)
                .await?;
            sqlx::query(
                "UPDATE intelligence_mcp_servers SET
                    credential_last4=$3,credential_set=TRUE,updated_at_micros=$4
                 WHERE org_id=$1 AND id=$2",
            )
            .bind(&input.org_id.0)
            .bind(&input.id.0)
            .bind(credential_last4(credential))
            .bind(now.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        }
        self.get_mcp_server(&input.org_id, &input.id).await
    }

    async fn update_mcp_server_runtime_status(
        &self,
        org_id: &Id,
        id: &Id,
        status: &str,
        last_error: Option<&str>,
        tested_at: Option<TimestampMicros>,
        synced_at: Option<TimestampMicros>,
    ) -> Result<McpServer> {
        let now = TimestampMicros::now();
        let result = sqlx::query(
            "UPDATE intelligence_mcp_servers SET
                status=$3,last_error=$4,
                last_tested_at_micros=COALESCE($5,last_tested_at_micros),
                last_synced_at_micros=COALESCE($6,last_synced_at_micros),
                updated_at_micros=$7
             WHERE org_id=$1 AND id=$2",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(status)
        .bind(last_error)
        .bind(tested_at.map(|value| value.0))
        .bind(synced_at.map(|value| value.0))
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found(format!("MCP Server `{}` not found", id.0)));
        }
        self.get_mcp_server(org_id, id).await
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "intelligence_mcp_servers")
    )]
    async fn delete_mcp_server(&self, org_id: &Id, id: &Id) -> Result<()> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        sqlx::query("DELETE FROM intelligence_mcp_tools WHERE org_id=$1 AND server_id=$2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;
        sqlx::query("DELETE FROM intelligence_mcp_server_secrets WHERE org_id=$1 AND server_id=$2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;
        let result = sqlx::query("DELETE FROM intelligence_mcp_servers WHERE org_id=$1 AND id=$2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found(format!("MCP Server `{}` not found", id.0)));
        }
        transaction.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn list_mcp_tools(&self, org_id: &Id, server_id: Option<&Id>) -> Result<Vec<McpTool>> {
        let rows = if let Some(server_id) = server_id {
            sqlx::query(&format!(
                "SELECT {MCP_TOOL_COLS} FROM intelligence_mcp_tools
                 WHERE org_id=$1 AND server_id=$2 ORDER BY name"
            ))
            .bind(&org_id.0)
            .bind(&server_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
        } else {
            sqlx::query(&format!(
                "SELECT {MCP_TOOL_COLS} FROM intelligence_mcp_tools
                 WHERE org_id=$1 ORDER BY name"
            ))
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
        };
        rows.into_iter().map(mcp_tool_row).collect()
    }

    async fn get_mcp_tool_by_name(&self, org_id: &Id, name: &str) -> Result<Option<McpTool>> {
        let row = sqlx::query(&format!(
            "SELECT {MCP_TOOL_COLS} FROM intelligence_mcp_tools
             WHERE org_id=$1 AND name=$2"
        ))
        .bind(&org_id.0)
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row.map(mcp_tool_row).transpose()
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "intelligence_mcp_tools")
    )]
    async fn upsert_mcp_tools(&self, tools: Vec<McpTool>) -> Result<Vec<McpTool>> {
        if tools.is_empty() {
            return Ok(Vec::new());
        }
        let org_id = tools[0].org_id.clone();
        let server_id = tools[0].server_id.clone();
        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        for tool in &tools {
            if tool.org_id != org_id || tool.server_id != server_id {
                return Err(Error::invalid(
                    "MCP tool batch must belong to one organization and server",
                ));
            }
            sqlx::query(
                "INSERT INTO intelligence_mcp_tools
                    (id,org_id,server_id,remote_name,name,display_name,description,input_schema,
                     schema_hash,schema_dialect,schema_synced_at_micros,unavailable_diagnostic,
                     output_schema,minimum_risk,risk,execution_mode,capabilities,tags,enabled,
                     status,version,last_synced_at_micros,created_at_micros,updated_at_micros)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24)
                 ON CONFLICT (server_id,remote_name) DO UPDATE SET
                    name=EXCLUDED.name,display_name=EXCLUDED.display_name,
                    description=EXCLUDED.description,input_schema=EXCLUDED.input_schema,
                    schema_hash=EXCLUDED.schema_hash,schema_dialect=EXCLUDED.schema_dialect,
                    schema_synced_at_micros=EXCLUDED.schema_synced_at_micros,
                    unavailable_diagnostic=EXCLUDED.unavailable_diagnostic,
                    output_schema=EXCLUDED.output_schema,minimum_risk=EXCLUDED.minimum_risk,
                    risk=EXCLUDED.risk,execution_mode=EXCLUDED.execution_mode,
                    capabilities=EXCLUDED.capabilities,tags=EXCLUDED.tags,
                    enabled=EXCLUDED.enabled,status=EXCLUDED.status,version=EXCLUDED.version,
                    last_synced_at_micros=EXCLUDED.last_synced_at_micros,
                    updated_at_micros=EXCLUDED.updated_at_micros",
            )
            .bind(&tool.id.0)
            .bind(&tool.org_id.0)
            .bind(&tool.server_id.0)
            .bind(&tool.remote_name)
            .bind(&tool.name)
            .bind(&tool.display_name)
            .bind(&tool.description)
            .bind(Json(&tool.input_schema))
            .bind(&tool.schema_hash)
            .bind(&tool.schema_dialect)
            .bind(tool.schema_synced_at.0)
            .bind(&tool.unavailable_diagnostic)
            .bind(tool.output_schema.as_ref().map(Json))
            .bind(risk_string(tool.minimum_risk))
            .bind(risk_string(tool.risk))
            .bind(execution_mode_string(tool.execution_mode))
            .bind(Json(&tool.capabilities))
            .bind(Json(&tool.tags))
            .bind(tool.enabled)
            .bind(status_string(tool.status))
            .bind(&tool.version)
            .bind(tool.last_synced_at.0)
            .bind(tool.created_at.0)
            .bind(tool.updated_at.0)
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;
        }
        transaction.commit().await.map_err(sqlx_err)?;
        self.list_mcp_tools(&org_id, Some(&server_id)).await
    }

    async fn update_mcp_tool_policy(&self, tool: McpTool) -> Result<McpTool> {
        let result = sqlx::query(
            "UPDATE intelligence_mcp_tools SET
                risk=$3,execution_mode=$4,enabled=$5,status=$6,updated_at_micros=$7
             WHERE org_id=$1 AND id=$2",
        )
        .bind(&tool.org_id.0)
        .bind(&tool.id.0)
        .bind(risk_string(tool.risk))
        .bind(execution_mode_string(tool.execution_mode))
        .bind(tool.enabled)
        .bind(status_string(tool.status))
        .bind(tool.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found(format!(
                "MCP tool `{}` not found",
                tool.id.0
            )));
        }
        self.get_mcp_tool_by_name(&tool.org_id, &tool.name)
            .await?
            .ok_or_else(|| Error::internal("updated MCP tool disappeared"))
    }
}
