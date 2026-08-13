// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Protocol-neutral builtin Tool coordinator shared by Mole Agent and inbound MCP adapters.

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{
    ToolAccess, ToolContent, ToolExecutionMode, ToolInvocationContext, ToolResult,
    catalog::BuiltinToolKind,
};

use super::control;
use crate::{
    api::AppState,
    shared::{Error, Result, ids::Id},
};

pub(crate) async fn execute(
    state: &AppState,
    invocation: &ToolInvocationContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    validate_policy(invocation.execution_mode(), kind)?;
    if kind == BuiltinToolKind::ExecuteAgentApproval {
        state.tools.authorize(invocation, kind)?;
        return execute_agent_approval(state, invocation, arguments).await;
    }

    let result = state.tools.execute(invocation, kind, arguments).await?;
    if invocation.execution_mode().automatically_executes()
        && matches!(
            kind.spec().access,
            ToolAccess::ManagedMutation | ToolAccess::CreatesApprovalRequest
        )
    {
        execute_automatic_approval(state, invocation, kind, result).await
    } else {
        Ok(result)
    }
}

fn validate_policy(mode: ToolExecutionMode, kind: BuiltinToolKind) -> Result<()> {
    if mode == ToolExecutionMode::Disabled {
        return Err(Error::forbidden(format!(
            "tool `{}` is disabled by the active Tool Policy",
            kind.name()
        )));
    }
    if !mode.allowed_for_risk(kind.spec().risk) {
        return Err(Error::forbidden(format!(
            "execution mode is below the minimum risk policy for tool `{}`",
            kind.name()
        )));
    }
    match kind.spec().access {
        ToolAccess::ReadOnly | ToolAccess::Preflight if mode != ToolExecutionMode::Automatic => {
            Err(Error::forbidden(format!(
                "tool `{}` cannot run in a non-automatic mode",
                kind.name()
            )))
        }
        ToolAccess::ExecutesApprovedOperation
            if matches!(
                mode,
                ToolExecutionMode::SingleApproval | ToolExecutionMode::DualApproval
            ) =>
        {
            Err(Error::forbidden(
                "execute_agent_approval cannot itself require another approval",
            ))
        }
        _ => Ok(()),
    }
}

async fn execute_automatic_approval(
    state: &AppState,
    invocation: &ToolInvocationContext,
    kind: BuiltinToolKind,
    result: ToolResult,
) -> Result<ToolResult> {
    if result.is_error {
        return Ok(result);
    }
    let approval_id = result
        .content
        .iter()
        .find_map(|content| match content {
            ToolContent::Json { json } => json
                .get("approval")
                .and_then(|approval| approval.get("id"))
                .and_then(Value::as_str),
            ToolContent::Text { .. } => None,
        })
        .ok_or_else(|| {
            Error::internal(format!(
                "managed tool `{}` did not return an approval id",
                kind.name()
            ))
        })?;
    let approval_id = Id::from_string(approval_id);
    let completed = control::execute_approved_operation(
        state,
        invocation.iam_context(),
        &approval_id,
        format!("tool:{}:{}", kind.name(), approval_id.0),
    )
    .await?;
    Ok(execution_result(approval_id, completed))
}

async fn execute_agent_approval(
    state: &AppState,
    invocation: &ToolInvocationContext,
    arguments: Value,
) -> Result<ToolResult> {
    let arguments: ExecuteAgentApprovalArgs = serde_json::from_value(arguments)
        .map_err(|error| Error::invalid(format!("invalid tool arguments: {error}")))?;
    let approval_id = Id::from_string(arguments.approval_id);
    let idempotency_key = arguments
        .idempotency_key
        .unwrap_or_else(|| format!("tool:execute_agent_approval:{}", approval_id.0));
    let completed = control::execute_approved_operation(
        state,
        invocation.iam_context(),
        &approval_id,
        idempotency_key,
    )
    .await?;
    Ok(execution_result(approval_id, completed))
}

fn execution_result(approval_id: Id, completed: control::OperationExecution) -> ToolResult {
    let operation_id = completed.execution.id.clone();
    ToolResult::json(json!({
        "operation_id": operation_id,
        "approval_id": approval_id,
        "status": completed.execution.status,
        "execution": completed.execution,
        "one_time_result": completed.one_time_result,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecuteAgentApprovalArgs {
    approval_id: String,
    #[serde(default)]
    idempotency_key: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approved_execution_cannot_create_a_nested_approval() {
        assert!(
            validate_policy(
                ToolExecutionMode::SingleApproval,
                BuiltinToolKind::ExecuteAgentApproval,
            )
            .is_err()
        );
        assert!(
            validate_policy(
                ToolExecutionMode::Confirmation,
                BuiltinToolKind::ExecuteAgentApproval,
            )
            .is_ok()
        );
    }

    #[test]
    fn read_tools_only_run_automatically() {
        assert!(
            validate_policy(ToolExecutionMode::Confirmation, BuiltinToolKind::QueryLogs).is_err()
        );
        assert!(validate_policy(ToolExecutionMode::Automatic, BuiltinToolKind::QueryLogs).is_ok());
    }
}
