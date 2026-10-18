// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 工具策略与 MCP Server 的 PostgreSQL 实装。
//!
//! MCP 凭据、敏感请求头与环境变量、成员凭据都和可序列化元数据分表保存，并使用
//! [`CipherRootKey`] seal。任何读取列表 / 详情的路径都只返回是否已设与后四位；明文仅供 MCP
//! runtime 即时构造请求。服务器在 [`servers`]，成员凭据与 prompt 在 [`members`]。

mod members;
mod rows;
mod servers;

use async_trait::async_trait;
use sqlx::{PgPool, types::Json};

use self::rows::{
    DEFAULTS_COLS, MCP_TOOL_COLS, POLICY_COLS, defaults_row, execution_mode_string, mcp_tool_row,
    policy_row, risk_string, status_string,
};
use super::super::sqlx_err;
use crate::{
    agent::{
        mcp::McpPrompt,
        tool_control::{
            McpServer, McpServerInput, McpServerRuntime, McpTool, MemberCredential,
            ToolControlRepository, ToolPolicy, ToolPolicyDefaults,
        },
    },
    infra::cipher::CipherRootKey,
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

    /// 加密一段明文，返回 `(nonce, ciphertext)`。
    fn seal(&self, plaintext: &str) -> Result<(Vec<u8>, Vec<u8>)> {
        self.cipher
            .seal(plaintext.as_bytes())
            .map_err(|error| Error::internal(format!("MCP secret seal: {error}")))
    }

    fn open(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<String> {
        let plaintext = self
            .cipher
            .open(nonce, ciphertext)
            .map_err(|error| Error::internal(format!("MCP secret open: {error}")))?;

        String::from_utf8(plaintext)
            .map_err(|error| Error::internal(format!("MCP secret is not UTF-8: {error}")))
    }
}

#[async_trait]
impl ToolControlRepository for PgToolControlRepository {
    async fn list_policies(&self, org_id: &Id) -> Result<Vec<ToolPolicy>> {
        let rows = sqlx::query(&format!(
            "SELECT {POLICY_COLS} FROM agent_tool_policies
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
            "SELECT {POLICY_COLS} FROM agent_tool_policies
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
            "INSERT INTO agent_tool_policies
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
            "SELECT {DEFAULTS_COLS} FROM agent_tool_policy_defaults WHERE org_id=$1"
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
            "INSERT INTO agent_tool_policy_defaults
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
        self.servers_in(org_id).await
    }

    async fn get_mcp_server(&self, org_id: &Id, id: &Id) -> Result<McpServer> {
        self.server(org_id, id).await
    }

    async fn get_mcp_server_runtime(
        &self,
        org_id: &Id,
        id: &Id,
        member: Option<&Id>,
    ) -> Result<McpServerRuntime> {
        self.server_runtime(org_id, id, member).await
    }

    async fn create_mcp_server(
        &self,
        input: McpServerInput,
        credential: Option<&str>,
    ) -> Result<McpServer> {
        self.insert_server(input, credential).await
    }

    async fn update_mcp_server(
        &self,
        input: McpServerInput,
        credential: Option<&str>,
    ) -> Result<McpServer> {
        self.rewrite_server(input, credential).await
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
        self.set_status(org_id, id, status, last_error, tested_at, synced_at)
            .await
    }

    async fn set_mcp_server_enabled(
        &self,
        org_id: &Id,
        id: &Id,
        enabled: bool,
    ) -> Result<McpServer> {
        self.set_enabled(org_id, id, enabled).await
    }

    async fn delete_mcp_server(&self, org_id: &Id, id: &Id) -> Result<()> {
        self.remove_server(org_id, id).await
    }

    async fn mcp_servers_due(&self, before: TimestampMicros, limit: i64) -> Result<Vec<McpServer>> {
        self.servers_due(before, limit).await
    }

    async fn list_member_credentials(
        &self,
        org_id: &Id,
        user_id: &Id,
    ) -> Result<Vec<MemberCredential>> {
        self.member_credentials(org_id, user_id).await
    }

    async fn put_member_credential(
        &self,
        org_id: &Id,
        server_id: &Id,
        user_id: &Id,
        value: &str,
    ) -> Result<MemberCredential> {
        self.save_member_credential(org_id, server_id, user_id, value)
            .await
    }

    async fn delete_member_credential(
        &self,
        org_id: &Id,
        server_id: &Id,
        user_id: &Id,
    ) -> Result<bool> {
        self.remove_member_credential(org_id, server_id, user_id)
            .await
    }

    async fn list_mcp_prompts(&self, org_id: &Id) -> Result<Vec<McpPrompt>> {
        self.prompts_in(org_id).await
    }

    async fn replace_mcp_prompts(
        &self,
        org_id: &Id,
        server_id: &Id,
        prompts: Vec<McpPrompt>,
    ) -> Result<()> {
        self.put_prompts(org_id, server_id, prompts).await
    }

    async fn list_mcp_tools(&self, org_id: &Id, server_id: Option<&Id>) -> Result<Vec<McpTool>> {
        let rows = if let Some(server_id) = server_id {
            sqlx::query(&format!(
                "SELECT {MCP_TOOL_COLS} FROM agent_mcp_tools
                 WHERE org_id=$1 AND server_id=$2 ORDER BY name"
            ))
            .bind(&org_id.0)
            .bind(&server_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
        } else {
            sqlx::query(&format!(
                "SELECT {MCP_TOOL_COLS} FROM agent_mcp_tools
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
            "SELECT {MCP_TOOL_COLS} FROM agent_mcp_tools
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
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "agent_mcp_tools")
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
                "INSERT INTO agent_mcp_tools
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
            "UPDATE agent_mcp_tools SET
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
