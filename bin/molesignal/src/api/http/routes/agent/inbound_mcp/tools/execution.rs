// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    sync::LazyLock,
    time::{Duration, Instant},
};

use regex::Regex;
use rmcp::{
    ErrorData, RoleServer,
    model::{CallToolResponse, CallToolResult, ContentBlock, ProgressNotificationParam},
    service::RequestContext,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tool_runtime::{ToolAccess, ToolContent, ToolResult};

use super::{ResolvedTool, catalog, visible_error};
use crate::{
    agent::model::ToolCallRecord,
    api::http::routes::agent::{builtin_execution, tool_dispatcher::redact_sensitive_value},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadCallArgs {
    name: String,
    #[serde(default)]
    arguments: Value,
    #[serde(default)]
    as_task: bool,
}

pub(super) async fn call_read(
    handler: &super::super::handler::InboundMcpHandler,
    context: &RequestContext<RoleServer>,
    data: &super::super::handler::InboundRequestData,
    arguments: Value,
) -> Result<CallToolResponse, ErrorData> {
    let args: ReadCallArgs = serde_json::from_value(arguments)
        .map_err(|error| ErrorData::invalid_params(error.to_string(), None))?;
    let resolved = match catalog::resolve(handler, &data.iam, &args.name).await {
        Ok(resolved) => resolved,
        Err(error) => return Ok(visible_error(error)),
    };
    if !matches!(
        resolved.spec.access,
        ToolAccess::ReadOnly | ToolAccess::Preflight
    ) {
        return Ok(visible_error(format!(
            "tool `{}` is a managed change; use call_managed_tool",
            args.name
        )));
    }
    if args.as_task {
        if !context
            .client_capabilities()
            .is_some_and(|capabilities| capabilities.supports_tasks())
        {
            return Err(ErrorData::invalid_params(
                "as_task requires the MCP Tasks client capability",
                None,
            ));
        }
        return super::super::tasks::spawn_read(handler, data, resolved, args.arguments)
            .await
            .map(Into::into);
    }
    execute_for_request(handler, context, data, resolved, args.arguments).await
}

pub(super) async fn call_direct(
    handler: &super::super::handler::InboundMcpHandler,
    context: &RequestContext<RoleServer>,
    data: &super::super::handler::InboundRequestData,
    name: &str,
    arguments: Value,
) -> Result<CallToolResponse, ErrorData> {
    let resolved = match catalog::resolve(handler, &data.iam, name).await {
        Ok(resolved) => resolved,
        Err(error) => return Ok(visible_error(error)),
    };
    if !matches!(
        resolved.spec.access,
        ToolAccess::ReadOnly | ToolAccess::Preflight
    ) {
        return Ok(visible_error(format!(
            "managed tool `{name}` must be called through call_managed_tool"
        )));
    }
    execute_for_request(handler, context, data, resolved, arguments).await
}

async fn execute_for_request(
    handler: &super::super::handler::InboundMcpHandler,
    context: &RequestContext<RoleServer>,
    data: &super::super::handler::InboundRequestData,
    resolved: ResolvedTool,
    arguments: Value,
) -> Result<CallToolResponse, ErrorData> {
    let cancellation = context.ct.child_token();
    let result = execute_resolved(handler, data, resolved, arguments, cancellation).await;
    Ok(match result {
        Ok(result) => into_mcp_result(result).into(),
        Err(error) => visible_error(error),
    })
}

pub(in crate::api::http::routes::agent::inbound_mcp) async fn execute_resolved(
    handler: &super::super::handler::InboundMcpHandler,
    data: &super::super::handler::InboundRequestData,
    resolved: ResolvedTool,
    arguments: Value,
    cancellation: CancellationToken,
) -> Result<ToolResult> {
    let started = Instant::now();
    let timeout_ms = resolved
        .timeout_ms
        .unwrap_or(data.settings.read_timeout_ms)
        .clamp(1, 300_000);
    let tool_name = resolved.spec.name.clone();
    let execution_mode = resolved.invocation.execution_mode();
    let execution = builtin_execution::execute(
        &handler.state,
        &resolved.invocation,
        resolved.kind,
        arguments.clone(),
    );
    let outcome = tokio::select! {
        () = cancellation.cancelled() => Err(Error::cancelled(format!("tool `{tool_name}` was cancelled"))),
        result = tokio::time::timeout(Duration::from_millis(timeout_ms as u64), execution) => {
            result
                .map_err(|_| Error::unavailable(format!("tool `{tool_name}` timed out")))?
        }
    }
    .map(sanitize_result)
    .map(|result| {
        let configured = resolved
            .max_response_bytes
            .unwrap_or(data.settings.max_response_bytes)
            .min(data.settings.max_response_bytes)
            .max(1) as usize;
        if result.serialized_len() > configured {
            ToolResult::error(format!(
                "tool `{tool_name}` response exceeded the configured {configured} byte limit"
            ))
        } else {
            result
        }
    });
    if let Err(error) = audit(
        handler,
        data,
        &resolved,
        arguments,
        started,
        execution_mode,
        &outcome,
    )
    .await
    {
        tracing::warn!(error = %error, tool = %tool_name, "failed to record Inbound MCP tool audit");
    }
    outcome
}

async fn audit(
    handler: &super::super::handler::InboundMcpHandler,
    data: &super::super::handler::InboundRequestData,
    resolved: &ResolvedTool,
    arguments: Value,
    started: Instant,
    execution_mode: tool_runtime::ToolExecutionMode,
    outcome: &Result<ToolResult>,
) -> Result<()> {
    let (status, error, output_summary, approval_id) = match outcome {
        Ok(result) => {
            let summary = result
                .content
                .first()
                .and_then(|content| match content {
                    ToolContent::Text { text } => Some(redact_credentials_in_text(text)),
                    ToolContent::Json { json } => {
                        serde_json::to_string(&redact_sensitive_value(json)).ok()
                    }
                })
                .map(|value| value.chars().take(1_000).collect());
            (
                if result.is_error { "error" } else { "success" },
                result.is_error.then(|| result.first_text()).flatten(),
                summary,
                approval_id(result),
            )
        }
        Err(error) => ("error", Some(error.to_string()), None, None),
    };
    handler
        .state
        .agent
        .repository
        .record_tool_call(ToolCallRecord {
            id: Id::new(),
            org_id: data.iam.org_id.clone(),
            chat_id: None,
            investigation_id: None,
            step_id: None,
            tool_name: resolved.spec.name.clone(),
            risk: resolved.spec.risk,
            input: redact_sensitive_value(&arguments),
            output_summary,
            status: status.into(),
            error,
            duration_ms: i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX),
            called_by: data.iam.principal_id().clone(),
            call_source: "inbound_mcp".into(),
            profile_id: None,
            approval_id,
            policy_decision: json!({
                "surface": "inbound_mcp",
                "execution_mode": execution_mode,
                "agent_profile_applied": false,
                "outbound_mcp_allowed": false,
            }),
            audit_id: None,
            created_at: TimestampMicros::now(),
        })
        .await?;
    Ok(())
}

pub(super) fn approval_id(result: &ToolResult) -> Option<Id> {
    result.content.iter().find_map(|content| match content {
        ToolContent::Json { json } => json
            .pointer("/approval/id")
            .or_else(|| json.get("approval_id"))
            .and_then(Value::as_str)
            .map(Id::from_string),
        ToolContent::Text { .. } => None,
    })
}

pub(super) fn sanitize_result(mut result: ToolResult) -> ToolResult {
    result.content = result
        .content
        .into_iter()
        .map(|content| match content {
            ToolContent::Text { text } => ToolContent::Text {
                text: redact_credentials_in_text(&text),
            },
            ToolContent::Json { json } => ToolContent::Json {
                json: sanitize_value(json),
            },
        })
        .collect();
    result
}

fn sanitize_value(value: Value) -> Value {
    match value {
        Value::Object(mut map) => {
            let withheld = map.remove("one_time_result").is_some();
            for (key, value) in &mut map {
                if is_secret_key(key) {
                    *value =
                        Value::String("<withheld: available only in MoleSignal Web UI>".into());
                } else {
                    *value = sanitize_value(std::mem::take(value));
                }
            }
            if withheld {
                map.insert(
                    "credential_delivery".into(),
                    Value::String("Credential created successfully. View or copy it only in MoleSignal Web UI.".into()),
                );
            }
            Value::Object(map)
        }
        Value::Array(values) => Value::Array(values.into_iter().map(sanitize_value).collect()),
        Value::String(text) => Value::String(redact_credentials_in_text(&text)),
        other => other,
    }
}

fn redact_credentials_in_text(text: &str) -> String {
    static CREDENTIAL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?x)
            (?:ms_|msrum_)[A-Za-z0-9]{16}_[A-Za-z0-9]{32}
            |(?:msoauth_|msrefresh_|mscode_|msmcp_secret_)[A-Za-z0-9_-]{32,128}
            |ms_sp_[A-Fa-f0-9]{16}
            ",
        )
        .expect("credential redaction regex must compile")
    });
    CREDENTIAL.replace_all(text, "<withheld>").into_owned()
}

fn is_secret_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    matches!(
        normalized.as_str(),
        "token"
            | "apitoken"
            | "accesstoken"
            | "refreshtoken"
            | "clientsecret"
            | "secret"
            | "plaintext"
            | "credential"
            | "password"
    )
}

pub(in crate::api::http::routes::agent::inbound_mcp) fn into_mcp_result(
    result: ToolResult,
) -> CallToolResult {
    let mut content = Vec::new();
    let mut structured = Vec::new();
    for item in result.content {
        match item {
            ToolContent::Text { text } => content.push(ContentBlock::text(text)),
            ToolContent::Json { json } => {
                content.push(ContentBlock::text(json.to_string()));
                structured.push(json);
            }
        }
    }
    let structured = match structured.len() {
        0 => None,
        1 => structured.pop(),
        _ => Some(json!({ "items": structured })),
    };
    let mut response = if result.is_error {
        CallToolResult::error(content)
    } else {
        CallToolResult::success(content)
    };
    response.structured_content = structured;
    response
}

pub(super) async fn notify_progress(
    context: &RequestContext<RoleServer>,
    progress: f64,
    message: &str,
) {
    if let Some(token) = context.meta.get_progress_token() {
        let _ = context
            .peer
            .notify_progress(
                ProgressNotificationParam::new(token, progress)
                    .with_total(1.0)
                    .with_message(message),
            )
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_credentials_without_reformatting_text() {
        let api_token = "ms_aB3kZ1xT9pQrU7nM_dFgHjKl8eRvNcWxYz4tBmEqPaS2vG6Qz";
        let oauth_token = format!("msoauth_{}", "x".repeat(43));
        let input = format!("token=\"{api_token}\"\nAuthorization: Bearer {oauth_token}.");
        assert_eq!(
            redact_credentials_in_text(&input),
            "token=\"<withheld>\"\nAuthorization: Bearer <withheld>."
        );
    }

    #[test]
    fn redacts_secret_fields_in_common_naming_styles() {
        let sanitized = sanitize_value(json!({
            "clientSecret": "value",
            "nested": { "api-token": "value" },
            "token_kind": "access",
        }));
        assert_eq!(
            sanitized["clientSecret"],
            "<withheld: available only in MoleSignal Web UI>"
        );
        assert_eq!(
            sanitized["nested"]["api-token"],
            "<withheld: available only in MoleSignal Web UI>"
        );
        assert_eq!(sanitized["token_kind"], "access");
    }
}
