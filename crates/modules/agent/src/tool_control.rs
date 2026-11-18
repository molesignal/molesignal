// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent tool control plane domain model.
//!
//! Built-in tool definitions are still registered at compile time by [`crate::tools`]; this
//! module stores only organization-level enablement, execution policies, and explicitly imported
//! MCP Server / tool / prompt metadata. MCP servers have three scope levels: organization, team,
//! and personal ([`Scope`]). The repository stores sensitive headers and credentials encrypted;
//! member credentials are stored per person and isolated from one another.

use async_trait::async_trait;
use kernel::{Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tool_runtime::{RiskLevel, ToolExecutionMode};

use crate::{
    mcp::{McpPrompt, NamedValue, NamedValueInput},
    scope::Scope,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedToolStatus {
    Healthy,
    Degraded,
    Unavailable,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPolicy {
    pub org_id: Id,
    pub tool_name: String,
    pub enabled: bool,
    pub execution_mode: ToolExecutionMode,
    /// Environment-level overrides, such as `{"production":"single_approval"}`.
    pub environment_overrides: Value,
    pub timeout_ms: i64,
    pub max_calls_per_run: i32,
    pub max_response_bytes: i64,
    pub updated_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPolicyDefaults {
    pub org_id: Id,
    /// `l0` ... `l4` -> [`ToolExecutionMode`].
    pub risk_modes: Value,
    /// `development` / `staging` / `production` -> risk policy objects.
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

/// Member credentials: each person provides their own value, placed in this header during calls.
/// Present when `auth_type` is `member_secret`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberCredentialSpec {
    /// A lowercase header name, such as `authorization` or `x-api-key`.
    pub header: String,
    /// Prepends `Bearer ` to the value.
    #[serde(default)]
    pub bearer: bool,
    /// Tells members where to obtain credentials and which permissions are required.
    #[serde(default)]
    pub instructions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub id: Id,
    pub org_id: Id,
    pub scope: Scope,
    /// The prefix for tool names and prompt commands, unique within the organization and unchanged
    /// when renamed.
    pub handle: String,
    pub name: String,
    pub transport: String,
    pub endpoint_url: Option<String>,
    /// The command line for a stdio server, launched by the server using instance-allowed commands.
    pub command_template: Option<String>,
    pub auth_type: String,
    pub auth_header: Option<String>,
    /// Used only for masked display; never contains the full credential.
    pub credential_last4: Option<String>,
    pub credential_set: bool,
    pub member_credential: Option<MemberCredentialSpec>,
    /// Request headers; sensitive values contain only the last four characters.
    pub headers: Vec<NamedValue>,
    /// Environment variables for the stdio server; sensitive values contain only the last four
    /// characters.
    pub env: Vec<NamedValue>,
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
    pub scope: Scope,
    pub handle: String,
    pub name: String,
    pub transport: String,
    pub endpoint_url: Option<String>,
    pub command_template: Option<String>,
    pub auth_type: String,
    pub auth_header: Option<String>,
    pub member_credential: Option<MemberCredentialSpec>,
    /// Keeps the original value when a sensitive value is `None`.
    pub headers: Vec<NamedValueInput>,
    pub env: Vec<NamedValueInput>,
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
    /// Plaintext request headers, with sensitive values decrypted; excludes those with no value
    /// yet.
    pub headers: Vec<(String, String)>,
    /// Plaintext environment variables for the stdio server, with sensitive values decrypted;
    /// excludes those with no value yet.
    pub env: Vec<(String, String)>,
    /// The credentials belonging to the initiator of this call.
    pub member_credential: Option<String>,
}

/// Credentials a member stores for a server, containing only the last four characters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberCredential {
    pub server_id: Id,
    pub user_id: Id,
    pub last4: String,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub id: Id,
    pub org_id: Id,
    pub server_id: Id,
    /// The original tool name returned by the MCP Server.
    pub remote_name: String,
    /// The unique, stable name in the Agent registry.
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
    /// Plaintext for the connection; `member` is the initiator of this call, whose own credentials
    /// are included.
    async fn get_mcp_server_runtime(
        &self,
        org_id: &Id,
        id: &Id,
        member: Option<&Id>,
    ) -> Result<McpServerRuntime>;
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
    async fn set_mcp_server_enabled(
        &self,
        org_id: &Id,
        id: &Id,
        enabled: bool,
    ) -> Result<McpServer>;
    async fn delete_mcp_server(&self, org_id: &Id, id: &Id) -> Result<()>;
    /// Servers across all organizations due for resynchronization: enabled, previously
    /// synchronized, independent of member credentials, and last checked before `before`.
    async fn mcp_servers_due(&self, before: TimestampMicros, limit: i64) -> Result<Vec<McpServer>>;

    async fn list_member_credentials(
        &self,
        org_id: &Id,
        user_id: &Id,
    ) -> Result<Vec<MemberCredential>>;
    async fn put_member_credential(
        &self,
        org_id: &Id,
        server_id: &Id,
        user_id: &Id,
        value: &str,
    ) -> Result<MemberCredential>;
    async fn delete_member_credential(
        &self,
        org_id: &Id,
        server_id: &Id,
        user_id: &Id,
    ) -> Result<bool>;

    async fn list_mcp_prompts(&self, org_id: &Id) -> Result<Vec<McpPrompt>>;
    /// Replaces prompts with those currently listed by the server.
    async fn replace_mcp_prompts(
        &self,
        org_id: &Id,
        server_id: &Id,
        prompts: Vec<McpPrompt>,
    ) -> Result<()>;

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
