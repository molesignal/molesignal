// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 调用 MCP 工具与 prompt。
//!
//! 参数先按同步时记下的 schema 校验（schema 的 hash 对不上就不调），再去掉模型填的租户、用户
//! 与审批身份字段。服务器要成员凭据而发起人还没存、或远端拒绝了他的凭据时，调用回
//! [`McpToolCall::NeedsCredential`]：对话里请他存下凭据再调一次（「授权以继续」），没法问人时
//! 交给模型的结果是 `authorization_required`，告诉模型请用户去 Mole Agent 设置里填写。

use std::future::Future;

use serde_json::{Map, Value, json};

use super::{McpService, schema, transport::Session};
use crate::{
    agent::{
        tool_control::{ManagedToolStatus, McpServer, McpTool},
        tools::{ToolContent, ToolResult},
    },
    shared::{Error, Result, ids::Id},
};

/// 工具结果与命令拒绝里用的错误码。
pub const AUTHORIZATION_REQUIRED: &str = "authorization_required";

/// 一次工具调用：远端的结果，或者要发起人先存下自己的凭据。
#[derive(Debug)]
pub enum McpToolCall {
    Done(ToolResult),
    NeedsCredential(CredentialNeed),
}

impl McpToolCall {
    /// 没法问人时交给模型的结果。
    pub fn into_result(self) -> ToolResult {
        match self {
            Self::Done(result) => result,
            Self::NeedsCredential(need) => need.into_result(),
        }
    }
}

/// 一台要成员凭据的服务器缺发起人的凭据，或拒绝了他存的那个：「授权以继续」卡片上说的东西。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialNeed {
    pub server_id: Id,
    pub server: String,
    /// 凭据放进的请求头，小写。
    pub header: String,
    /// 值前面加 `Bearer `。
    pub bearer: bool,
    /// 管理员写的：去哪里拿凭据、要哪些权限。
    pub instructions: String,
    /// 存过的凭据被远端拒绝了。
    pub rejected: bool,
}

impl CredentialNeed {
    fn of(server: &McpServer, rejected: bool) -> Self {
        let spec = server.member_credential.as_ref();

        Self {
            server_id: server.id.clone(),
            server: server.name.clone(),
            header: spec.map_or_else(|| "authorization".into(), |spec| spec.header.clone()),
            bearer: spec.is_some_and(|spec| spec.bearer),
            instructions: spec
                .map(|spec| spec.instructions.clone())
                .unwrap_or_default(),
            rejected,
        }
    }

    /// 告诉模型：这台服务器要用户自己的凭据，请他去设置里填写。
    pub fn into_result(self) -> ToolResult {
        let message = if self.rejected {
            format!(
                "The MCP server `{}` rejected the user's own credential. Ask the user to update it in Mole Agent settings → Tools → MCP servers → {} → My credential, then try again.",
                self.server, self.server
            )
        } else {
            format!(
                "The MCP server `{}` needs the user's own credential. Ask the user to add it in Mole Agent settings → Tools → MCP servers → {} → My credential, then try again.",
                self.server, self.server
            )
        };

        ToolResult {
            content: vec![ToolContent::Json {
                json: json!({
                    "error": AUTHORIZATION_REQUIRED,
                    "serverId": self.server_id.0,
                    "server": self.server,
                    "header": self.header,
                    "instructions": self.instructions,
                    "rejected": self.rejected,
                    "message": message,
                }),
            }],
            is_error: true,
        }
    }
}

impl McpService {
    pub async fn execute_tool(
        &self,
        server: &McpServer,
        tool: &McpTool,
        member: &Id,
        arguments: Value,
    ) -> Result<McpToolCall> {
        if !server.enabled || server.status != "healthy" {
            return Ok(McpToolCall::Done(tool_error("MCP Server is not available")));
        }

        if tool.status != ManagedToolStatus::Healthy || tool.unavailable_diagnostic.is_some() {
            return Ok(McpToolCall::Done(tool_error(
                "MCP tool schema is unavailable; synchronize the server before execution",
            )));
        }

        let mut runtime = self
            .repo
            .get_mcp_server_runtime(&server.org_id, &server.id, Some(member))
            .await?;

        if runtime.server.auth_type == "member_secret" && runtime.member_credential.is_none() {
            return Ok(McpToolCall::NeedsCredential(CredentialNeed::of(
                &runtime.server,
                false,
            )));
        }

        // 调用方可能按工具策略收紧了时限与应答上限。
        runtime.server.timeout_ms = server.timeout_ms;
        runtime.server.max_response_bytes = server.max_response_bytes;

        let policy = self.policy_for(&runtime.server).await?;
        let stdio_commands = self.stdio_commands.clone();

        let outcome = call_with_validated_arguments(
            &tool.input_schema,
            &tool.schema_hash,
            arguments,
            |arguments| async move {
                let (mut session, _) = Session::open(&runtime, &policy, &stdio_commands).await?;

                session
                    .call(
                        "tools/call",
                        json!({"name": tool.remote_name, "arguments": arguments}),
                    )
                    .await
            },
        )
        .await;

        match outcome {
            Err(Error::Unauthorized(_)) if server.auth_type == "member_secret" => Ok(
                McpToolCall::NeedsCredential(CredentialNeed::of(server, true)),
            ),
            Err(Error::Unauthorized(message)) => Ok(McpToolCall::Done(tool_error(message))),
            other => other.map(McpToolCall::Done),
        }
    }

    /// 发起人给这台服务器存过自己的凭据。
    pub async fn has_my_credential(
        &self,
        org_id: &Id,
        member: &Id,
        server_id: &Id,
    ) -> Result<bool> {
        Ok(self
            .repo
            .list_member_credentials(org_id, member)
            .await?
            .iter()
            .any(|credential| credential.server_id == *server_id))
    }

    /// `prompts/get`：用发起人自己的凭据取一个 prompt 的消息。
    pub async fn get_prompt(
        &self,
        server: &McpServer,
        member: &Id,
        name: &str,
        arguments: Map<String, Value>,
    ) -> Result<Value> {
        let runtime = self
            .repo
            .get_mcp_server_runtime(&server.org_id, &server.id, Some(member))
            .await?;

        if runtime.server.auth_type == "member_secret" && runtime.member_credential.is_none() {
            return Err(Error::unauthorized("your own MCP credential is not saved"));
        }

        let policy = self.policy_for(&runtime.server).await?;
        let (mut session, _) = Session::open(&runtime, &policy, &self.stdio_commands).await?;

        session
            .call(
                "prompts/get",
                json!({"name": name, "arguments": Value::Object(strip_untrusted_identity_fields_map(arguments))}),
            )
            .await
    }
}

async fn call_with_validated_arguments<F, Fut>(
    schema: &Value,
    expected_hash: &str,
    arguments: Value,
    call: F,
) -> Result<ToolResult>
where
    F: FnOnce(Value) -> Fut,
    Fut: Future<Output = Result<Value>>,
{
    schema::validate_schema_revision(schema, expected_hash, &arguments)?;
    let result = call(strip_untrusted_identity_fields(&arguments)).await?;
    let is_error = result["isError"].as_bool().unwrap_or(false);

    Ok(ToolResult {
        content: vec![ToolContent::Json { json: result }],
        is_error,
    })
}

fn tool_error(message: impl Into<String>) -> ToolResult {
    ToolResult {
        content: vec![ToolContent::Text {
            text: message.into(),
        }],
        is_error: true,
    }
}

const BLOCKED: [&str; 8] = [
    "org_id",
    "organization_id",
    "tenant_id",
    "user_id",
    "requested_by",
    "approved_by",
    "approval_id",
    "actor_id",
];

fn strip_untrusted_identity_fields_map(map: Map<String, Value>) -> Map<String, Value> {
    map.into_iter()
        .filter(|(key, _)| !BLOCKED.contains(&key.to_ascii_lowercase().as_str()))
        .collect()
}

fn strip_untrusted_identity_fields(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| !BLOCKED.contains(&key.to_ascii_lowercase().as_str()))
                .map(|(key, value)| (key.clone(), strip_untrusted_identity_fields(value)))
                .collect(),
        ),
        Value::Array(values) => {
            Value::Array(values.iter().map(strip_untrusted_identity_fields).collect())
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;
    use crate::agent::{scope::Scope, tool_control::MemberCredentialSpec};

    #[tokio::test]
    async fn invalid_arguments_never_invoke_the_remote_call() {
        let synchronized = schema::synchronize(json!({
            "type": "object",
            "required": ["query"],
            "additionalProperties": false,
            "properties": {"query": {"type": "string", "minLength": 1}}
        }))
        .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let result = call_with_validated_arguments(
            &synchronized.schema,
            &synchronized.hash,
            json!({"query": "", "extra": true}),
            move |_| async move {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(json!({"isError": false}))
            },
        )
        .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn valid_arguments_are_sanitized_before_the_remote_call() {
        let synchronized = schema::synchronize(json!({
            "type": "object",
            "required": ["query"],
            "additionalProperties": true,
            "properties": {"query": {"type": "string"}}
        }))
        .unwrap();
        let result = call_with_validated_arguments(
            &synchronized.schema,
            &synchronized.hash,
            json!({
                "query": "up",
                "org_id": "attacker-org",
                "nested": {"approval_id": "fake", "safe": true}
            }),
            |arguments| async move {
                assert_eq!(arguments["query"], "up");
                assert!(arguments.get("org_id").is_none());
                assert!(arguments["nested"].get("approval_id").is_none());
                assert_eq!(arguments["nested"]["safe"], true);
                Ok(json!({"isError": false, "content": []}))
            },
        )
        .await
        .unwrap();
        assert!(!result.is_error);
    }

    #[test]
    fn a_missing_member_credential_says_where_to_add_it() {
        let server = McpServer {
            id: Id::from_string("server-1"),
            org_id: Id::from_string("org-1"),
            scope: Scope::Org,
            handle: "linear".into(),
            name: "Linear".into(),
            transport: "streamable_http".into(),
            endpoint_url: Some("https://mcp.linear.app/mcp".into()),
            command_template: None,
            auth_type: "member_secret".into(),
            auth_header: None,
            credential_last4: None,
            credential_set: false,
            member_credential: Some(MemberCredentialSpec {
                header: "authorization".into(),
                bearer: true,
                instructions: "Create a personal API key in Linear settings.".into(),
            }),
            headers: Vec::new(),
            env: Vec::new(),
            private_only: false,
            allowed_domains: Vec::new(),
            allowed_cidrs: Vec::new(),
            follow_redirects: false,
            tls_verify: true,
            timeout_ms: 10_000,
            max_response_bytes: 1_048_576,
            enabled: true,
            status: "healthy".into(),
            last_error: None,
            last_tested_at: None,
            last_synced_at: None,
            created_by: Id::from_string("user-1"),
            created_at: crate::shared::time::TimestampMicros(1),
            updated_at: crate::shared::time::TimestampMicros(1),
        };

        let need = CredentialNeed::of(&server, false);
        assert_eq!(need.header, "authorization");
        assert!(need.bearer);

        let result = need.into_result();
        assert!(result.is_error);

        let ToolContent::Json { json } = &result.content[0] else {
            panic!("expected a JSON result");
        };

        assert_eq!(json["error"], AUTHORIZATION_REQUIRED);
        assert_eq!(
            json["instructions"],
            "Create a personal API key in Linear settings."
        );
        assert!(json["message"].as_str().unwrap().contains("My credential"));
    }
}
