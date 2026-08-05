// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 工具控制面领域模型。
//!
//! 内置工具定义仍由 [`crate::intelligence::tools`] 编译期注册；本模块只保存
//! 组织级启用状态、执行策略，以及经管理员显式导入的 MCP Server / tool 元数据。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    intelligence::model::RiskLevel,
    shared::{Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecutionMode {
    Automatic,
    Confirmation,
    SingleApproval,
    DualApproval,
    Disabled,
}

impl ToolExecutionMode {
    pub const fn default_for_risk(risk: RiskLevel) -> Self {
        match risk {
            RiskLevel::L0 => Self::Automatic,
            RiskLevel::L1 => Self::Confirmation,
            RiskLevel::L2 => Self::SingleApproval,
            RiskLevel::L3 => Self::DualApproval,
            RiskLevel::L4 => Self::Disabled,
        }
    }

    /// 系统硬限制：策略只允许收紧风险要求，不能把高风险工具降为自动执行。
    pub const fn allowed_for_risk(self, risk: RiskLevel) -> bool {
        match risk {
            RiskLevel::L0 | RiskLevel::L1 => true,
            RiskLevel::L2 => matches!(
                self,
                Self::SingleApproval | Self::DualApproval | Self::Disabled
            ),
            RiskLevel::L3 => matches!(
                self,
                Self::SingleApproval | Self::DualApproval | Self::Disabled
            ),
            RiskLevel::L4 => matches!(self, Self::DualApproval | Self::Disabled),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedToolStatus {
    Healthy,
    Degraded,
    Unavailable,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPolicy {
    pub org_id: Id,
    pub tool_name: String,
    pub enabled: bool,
    pub execution_mode: ToolExecutionMode,
    /// 环境级覆盖，例如 `{"production":"single_approval"}`。
    pub environment_overrides: Value,
    pub timeout_ms: i64,
    pub max_calls_per_run: i32,
    pub max_response_bytes: i64,
    pub updated_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPolicyDefaults {
    pub org_id: Id,
    /// `l0` ... `l4` → [`ToolExecutionMode`]。
    pub risk_modes: Value,
    /// `development` / `staging` / `production` → 风险策略对象。
    pub environment_overrides: Value,
    pub updated_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl ToolPolicyDefaults {
    pub fn system_defaults(org_id: Id, actor: Id) -> Self {
        let now = TimestampMicros::now();
        Self {
            org_id,
            risk_modes: serde_json::json!({
                "l0": "automatic",
                "l1": "confirmation",
                "l2": "single_approval",
                "l3": "dual_approval",
                "l4": "disabled"
            }),
            environment_overrides: serde_json::json!({}),
            updated_by: actor,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub transport: String,
    pub endpoint_url: Option<String>,
    pub command_template: Option<String>,
    pub auth_type: String,
    pub auth_header: Option<String>,
    /// 仅用于掩码展示；永远不包含完整凭据。
    pub credential_last4: Option<String>,
    pub credential_set: bool,
    pub private_only: bool,
    pub allowed_domains: Vec<String>,
    pub allowed_cidrs: Vec<String>,
    pub follow_redirects: bool,
    pub tls_verify: bool,
    pub timeout_ms: i64,
    pub max_response_bytes: i64,
    pub enabled: bool,
    pub status: String,
    pub last_error: Option<String>,
    pub last_tested_at: Option<TimestampMicros>,
    pub last_synced_at: Option<TimestampMicros>,
    pub created_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone)]
pub struct McpServerInput {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub transport: String,
    pub endpoint_url: Option<String>,
    pub command_template: Option<String>,
    pub auth_type: String,
    pub auth_header: Option<String>,
    pub private_only: bool,
    pub allowed_domains: Vec<String>,
    pub allowed_cidrs: Vec<String>,
    pub follow_redirects: bool,
    pub tls_verify: bool,
    pub timeout_ms: i64,
    pub max_response_bytes: i64,
    pub enabled: bool,
    pub created_by: Id,
}

#[derive(Debug, Clone)]
pub struct McpServerRuntime {
    pub server: McpServer,
    pub credential: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub id: Id,
    pub org_id: Id,
    pub server_id: Id,
    /// MCP Server 返回的原始 tool name。
    pub remote_name: String,
    /// Agent registry 中的唯一、稳定名称。
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub input_schema: Value,
    /// Canonical SHA-256 of the exact schema advertised and used for execution.
    pub schema_hash: String,
    pub schema_dialect: String,
    pub schema_synced_at: TimestampMicros,
    pub unavailable_diagnostic: Option<String>,
    pub output_schema: Option<Value>,
    pub minimum_risk: RiskLevel,
    pub risk: RiskLevel,
    pub execution_mode: ToolExecutionMode,
    pub capabilities: Value,
    pub tags: Vec<String>,
    pub enabled: bool,
    pub status: ManagedToolStatus,
    pub version: Option<String>,
    pub last_synced_at: TimestampMicros,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait ToolControlRepository: Send + Sync {
    async fn list_policies(&self, org_id: &Id) -> Result<Vec<ToolPolicy>>;
    async fn get_policy(&self, org_id: &Id, tool_name: &str) -> Result<Option<ToolPolicy>>;
    async fn upsert_policy(&self, policy: ToolPolicy) -> Result<ToolPolicy>;

    async fn get_policy_defaults(&self, org_id: &Id) -> Result<Option<ToolPolicyDefaults>>;
    async fn upsert_policy_defaults(
        &self,
        defaults: ToolPolicyDefaults,
    ) -> Result<ToolPolicyDefaults>;

    async fn list_mcp_servers(&self, org_id: &Id) -> Result<Vec<McpServer>>;
    async fn get_mcp_server(&self, org_id: &Id, id: &Id) -> Result<McpServer>;
    async fn get_mcp_server_runtime(&self, org_id: &Id, id: &Id) -> Result<McpServerRuntime>;
    async fn create_mcp_server(
        &self,
        input: McpServerInput,
        credential: Option<&str>,
    ) -> Result<McpServer>;
    async fn update_mcp_server(
        &self,
        input: McpServerInput,
        credential: Option<&str>,
    ) -> Result<McpServer>;
    async fn update_mcp_server_runtime_status(
        &self,
        org_id: &Id,
        id: &Id,
        status: &str,
        last_error: Option<&str>,
        tested_at: Option<TimestampMicros>,
        synced_at: Option<TimestampMicros>,
    ) -> Result<McpServer>;
    async fn delete_mcp_server(&self, org_id: &Id, id: &Id) -> Result<()>;

    async fn list_mcp_tools(&self, org_id: &Id, server_id: Option<&Id>) -> Result<Vec<McpTool>>;
    async fn get_mcp_tool_by_name(&self, org_id: &Id, name: &str) -> Result<Option<McpTool>>;
    async fn upsert_mcp_tools(&self, tools: Vec<McpTool>) -> Result<Vec<McpTool>>;
    async fn update_mcp_tool_policy(&self, tool: McpTool) -> Result<McpTool>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_risk_floors_reject_unsafe_execution_modes() {
        assert!(ToolExecutionMode::Automatic.allowed_for_risk(RiskLevel::L0));
        assert!(!ToolExecutionMode::Automatic.allowed_for_risk(RiskLevel::L2));
        assert!(!ToolExecutionMode::SingleApproval.allowed_for_risk(RiskLevel::L4));
        assert!(ToolExecutionMode::DualApproval.allowed_for_risk(RiskLevel::L4));
    }
}
