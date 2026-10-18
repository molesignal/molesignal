// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 调用 MCP 工具与 prompt。
//!
//! 参数先按同步时记下的 schema 校验（schema 的 hash 对不上就不调），再去掉模型填的租户、用户
//! 与审批身份字段。服务器要成员凭据而发起人还没存、或远端拒绝了他的凭据时，工具返回
//! `authorization_required`，告诉模型请用户去 Mole Agent 设置里填写。

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

impl McpService {
    pub async fn execute_tool(
        &self,
        server: &McpServer,
        tool: &McpTool,
        member: &Id,
        arguments: Value,
    ) -> Result<ToolResult> {
        if !server.enabled || server.status != "healthy" {
            return Ok(tool_error("MCP Server is not available"));
        }

        if tool.status != ManagedToolStatus::Healthy || tool.unavailable_diagnostic.is_some() {
            return Ok(tool_error(
                "MCP tool schema is unavailable; synchronize the server before execution",
            ));
        }

        let mut runtime = self
            .repo
            .get_mcp_server_runtime(&server.org_id, &server.id, Some(member))
            .await?;

        if runtime.server.auth_type == "member_secret" && runtime.member_credential.is_none() {
            return Ok(authorization_required(&runtime.server, false));
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
            Err(Error::Unauthorized(_)) if server.auth_type == "member_secret" => {
                Ok(authorization_required(server, true))
            }
            Err(Error::Unauthorized(message)) => Ok(tool_error(message)),
            other => other,
        }
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

/// 告诉模型：这台服务器要用户自己的凭据。
fn authorization_required(server: &McpServer, rejected: bool) -> ToolResult {
    let spec = server.member_credential.as_ref();

    let message = if rejected {
        format!(
            "The MCP server `{}` rejected the user's own credential. Ask the user to update it in Mole Agent settings → Tools → MCP servers → {} → My credential, then try again.",
            server.name, server.name
        )
    } else {
        format!(
            "The MCP server `{}` needs the user's own credential. Ask the user to add it in Mole Agent settings → Tools → MCP servers → {} → My credential, then try again.",
            server.name, server.name
        )
    };

    ToolResult {
        content: vec![ToolContent::Json {
            json: json!({
                "error": AUTHORIZATION_REQUIRED,
                "server_id": server.id.0,
                "server": server.name,
                "header": spec.map(|spec| spec.header.clone()),
                "instructions": spec.map(|spec| spec.instructions.clone()),
                "rejected": rejected,
                "message": message,
            }),
        }],
        is_error: true,
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

        let result = authorization_required(&server, false);
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
