// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::future::Future;

use serde_json::{Value, json};

use super::schema;
use crate::{
    api::AppState,
    intelligence::{
        tool_control::{ManagedToolStatus, McpServer, McpTool},
        tools::{ToolContent, ToolResult},
    },
    shared::{Result, ids::Id},
};

pub(crate) async fn execute_tool(
    state: &AppState,
    org_id: &Id,
    server: &McpServer,
    tool: &McpTool,
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
    call_with_validated_arguments(
        &tool.input_schema,
        &tool.schema_hash,
        arguments,
        |arguments| async move {
            let runtime = state
                .intelligence
                .tool_control
                .get_mcp_server_runtime(org_id, &server.id)
                .await?;
            super::runtime::rpc_request(
                &runtime,
                "tools/call",
                json!({
                    "name": tool.remote_name,
                    "arguments": arguments
                }),
            )
            .await
        },
    )
    .await
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

fn strip_untrusted_identity_fields(value: &Value) -> Value {
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
}
