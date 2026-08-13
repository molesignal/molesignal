// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use rmcp::{
    ErrorData, RoleServer,
    model::{
        CallToolRequestParams, CallToolResponse, ElicitRequest, ElicitRequestParams, InputRequest,
        InputRequests, InputRequiredResult,
    },
    service::RequestContext,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tool_runtime::{ToolAccess, ToolResult};

use super::{ResolvedTool, catalog, execution, visible_error};
use crate::{
    agent::{
        inbound_mcp::{InboundMcpIdempotencyInput, InboundMcpIdempotencyReservation},
        model::ApprovalStatus,
    },
    shared::{ids::Id, time::TimestampMicros},
};

const IDEMPOTENCY_LEASE_MICROS: i64 = 5 * 60 * 1_000_000;
const CONFIRMATION_TTL_MICROS: i64 = 10 * 60 * 1_000_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagedCallArgs {
    name: String,
    #[serde(default)]
    arguments: Value,
    idempotency_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecuteApprovalArgs {
    approval_id: String,
    idempotency_key: String,
}

#[derive(Serialize, Deserialize)]
struct ConfirmationState {
    purpose: String,
    org_id: String,
    principal_type: String,
    principal_id: String,
    approval_id: String,
    idempotency_key: String,
    issued_at_micros: i64,
}

pub(super) async fn call_managed(
    handler: &super::super::handler::InboundMcpHandler,
    context: &RequestContext<RoleServer>,
    data: &super::super::handler::InboundRequestData,
    arguments: Value,
) -> Result<CallToolResponse, ErrorData> {
    let args: ManagedCallArgs = serde_json::from_value(arguments)
        .map_err(|error| ErrorData::invalid_params(error.to_string(), None))?;
    validate_key(&args.idempotency_key)?;
    let resolved = match catalog::resolve(handler, &data.iam, &args.name).await {
        Ok(resolved) => resolved,
        Err(error) => return Ok(visible_error(error)),
    };
    if !matches!(
        resolved.spec.access,
        ToolAccess::ManagedMutation | ToolAccess::CreatesApprovalRequest
    ) {
        return Ok(visible_error(format!(
            "tool `{}` is not a managed change tool",
            args.name
        )));
    }
    execute_reserved(
        handler,
        context,
        data,
        resolved,
        args.arguments,
        args.idempotency_key,
    )
    .await
}

pub(super) async fn execute_approval(
    handler: &super::super::handler::InboundMcpHandler,
    context: &RequestContext<RoleServer>,
    data: &super::super::handler::InboundRequestData,
    request: CallToolRequestParams,
) -> Result<CallToolResponse, ErrorData> {
    let arguments = Value::Object(request.arguments.clone().unwrap_or_default());
    let args: ExecuteApprovalArgs = serde_json::from_value(arguments.clone())
        .map_err(|error| ErrorData::invalid_params(error.to_string(), None))?;
    validate_key(&args.idempotency_key)?;
    if args.approval_id.is_empty() || args.approval_id.len() > 128 {
        return Err(ErrorData::invalid_params("invalid approval_id", None));
    }
    let approval = handler
        .state
        .agent
        .repository
        .get_approval(&data.iam.org_id, &Id::from_string(&args.approval_id))
        .await
        .map_err(super::super::handler::protocol_error)?;
    if requires_web_credential_delivery(&approval.action) {
        return Ok(visible_error(
            "credential-producing approvals must be executed in MoleSignal Web UI so the one-time secret can be delivered safely",
        ));
    }
    if approval.status != ApprovalStatus::Executed && approval.required_approvals == 0 {
        if !supports_form_mrtr(context) {
            return Err(ErrorData::invalid_request(
                "this confirmation approval requires MCP 2026-07-28 form elicitation support",
                None,
            ));
        }
        if request.request_state.is_none() {
            if request.input_responses.is_some() {
                return Err(ErrorData::invalid_params(
                    "inputResponses requires requestState",
                    None,
                ));
            }
            return confirmation_required(handler, data, &approval, &args);
        }
        verify_confirmation(handler, data, &args, &request)?;
    } else if request.request_state.is_some() || request.input_responses.is_some() {
        return Err(ErrorData::invalid_params(
            "unexpected confirmation round-trip fields",
            None,
        ));
    }
    if !matches!(
        approval.status,
        ApprovalStatus::Approved | ApprovalStatus::Executed
    ) {
        return Ok(visible_error("approval has not reached approved status"));
    }
    let resolved = match catalog::resolve(handler, &data.iam, "execute_agent_approval").await {
        Ok(resolved) => resolved,
        Err(error) => return Ok(visible_error(error)),
    };
    execute_reserved(
        handler,
        context,
        data,
        resolved,
        arguments,
        args.idempotency_key,
    )
    .await
}

fn supports_form_mrtr(context: &RequestContext<RoleServer>) -> bool {
    context
        .protocol_version()
        .is_some_and(|version| version.as_str() >= "2026-07-28")
        && context
            .client_capabilities()
            .and_then(|capabilities| capabilities.elicitation)
            .and_then(|elicitation| elicitation.form)
            .is_some()
}

fn confirmation_required(
    handler: &super::super::handler::InboundMcpHandler,
    data: &super::super::handler::InboundRequestData,
    approval: &crate::agent::model::ApprovalRequest,
    args: &ExecuteApprovalArgs,
) -> Result<CallToolResponse, ErrorData> {
    let now = TimestampMicros::now();
    let state = ConfirmationState {
        purpose: "execute_agent_approval".into(),
        org_id: data.iam.org_id.0.clone(),
        principal_type: data.iam.principal_type().as_str().into(),
        principal_id: data.iam.principal_id().0.clone(),
        approval_id: args.approval_id.clone(),
        idempotency_key: args.idempotency_key.clone(),
        issued_at_micros: now.0,
    };
    let sealed = handler
        .runtime
        .request_state
        .seal_json(&state)
        .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
    let schema = serde_json::from_value(json!({
        "type": "object",
        "properties": {
            "confirmed": {
                "type": "boolean",
                "title": "Confirm execution",
                "description": "Execute this approved MoleSignal operation now."
            }
        },
        "required": ["confirmed"]
    }))
    .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
    let mut requests = InputRequests::new();
    requests.insert(
        "confirmation".into(),
        InputRequest::Elicitation(ElicitRequest::new(
            ElicitRequestParams::FormElicitationParams {
                meta: None,
                message: format!(
                    "Confirm execution of `{}` on `{}`. Reason: {} Impact: {}",
                    approval.action, approval.target, approval.reason, approval.impact
                ),
                requested_schema: schema,
            },
        )),
    );
    let input_required = InputRequiredResult::new(Some(requests), Some(sealed));
    let within_limit = serde_json::to_vec(&input_required)
        .is_ok_and(|encoded| encoded.len() <= data.settings.max_response_bytes.max(1) as usize);
    if within_limit {
        Ok(input_required.into())
    } else {
        Ok(visible_error(
            "confirmation request exceeds the configured response limit",
        ))
    }
}

fn verify_confirmation(
    handler: &super::super::handler::InboundMcpHandler,
    data: &super::super::handler::InboundRequestData,
    args: &ExecuteApprovalArgs,
    request: &CallToolRequestParams,
) -> Result<(), ErrorData> {
    let sealed = request
        .request_state
        .as_deref()
        .ok_or_else(|| ErrorData::invalid_params("missing confirmation requestState", None))?;
    let state: ConfirmationState = handler
        .runtime
        .request_state
        .open_json(sealed)
        .map_err(|_| ErrorData::invalid_params("tampered or expired requestState", None))?;
    let now = TimestampMicros::now();
    let matches = state.purpose == "execute_agent_approval"
        && state.org_id == data.iam.org_id.0
        && state.principal_type == data.iam.principal_type().as_str()
        && state.principal_id == data.iam.principal_id().0
        && state.approval_id == args.approval_id
        && state.idempotency_key == args.idempotency_key
        && now.0.saturating_sub(state.issued_at_micros) <= CONFIRMATION_TTL_MICROS;
    if !matches {
        return Err(ErrorData::invalid_params(
            "confirmation state does not match this identity or approval",
            None,
        ));
    }
    let response = request
        .input_responses
        .as_ref()
        .and_then(|responses| responses.get("confirmation"))
        .ok_or_else(|| ErrorData::invalid_params("missing confirmation response", None))?;
    let accepted = response.get("action").and_then(Value::as_str) == Some("accept")
        && response
            .pointer("/content/confirmed")
            .and_then(Value::as_bool)
            == Some(true);
    if accepted {
        Ok(())
    } else {
        Err(ErrorData::invalid_request(
            "the user declined or cancelled execution",
            None,
        ))
    }
}

async fn execute_reserved(
    handler: &super::super::handler::InboundMcpHandler,
    context: &RequestContext<RoleServer>,
    data: &super::super::handler::InboundRequestData,
    resolved: ResolvedTool,
    arguments: Value,
    idempotency_key: String,
) -> Result<CallToolResponse, ErrorData> {
    let input = InboundMcpIdempotencyInput {
        org_id: data.iam.org_id.clone(),
        principal_type: data.iam.principal_type().as_str().into(),
        principal_id: data.iam.principal_id().clone(),
        idempotency_key,
        tool_name: resolved.spec.name.clone(),
        request_hash: request_hash(&resolved.spec.name, &arguments),
        lease_expires_at: TimestampMicros(
            TimestampMicros::now()
                .0
                .saturating_add(IDEMPOTENCY_LEASE_MICROS),
        ),
    };
    match handler
        .state
        .agent
        .inbound_mcp
        .reserve_idempotency(input.clone())
        .await
        .map_err(super::super::handler::protocol_error)?
    {
        InboundMcpIdempotencyReservation::Existing(existing) => {
            return Ok(replay(*existing, data.settings.max_response_bytes));
        }
        InboundMcpIdempotencyReservation::Acquired => {}
    }
    let result =
        execution::execute_resolved(handler, data, resolved, arguments, context.ct.child_token())
            .await;
    let (tool_result, status) = match result {
        Ok(result) if result.is_error => (result, "failed"),
        Ok(result) => (result, "completed"),
        Err(error) => (ToolResult::error(error.to_string()), "failed"),
    };
    let persisted = serde_json::to_value(&tool_result)
        .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
    let approval_id = execution::approval_id(&tool_result);
    handler
        .state
        .agent
        .inbound_mcp
        .complete_idempotency(&input, approval_id.as_ref(), &persisted, status)
        .await
        .map_err(super::super::handler::protocol_error)?;
    Ok(execution::into_mcp_result(tool_result).into())
}

fn replay(
    record: crate::agent::inbound_mcp::InboundMcpIdempotencyRecord,
    max_response_bytes: i64,
) -> CallToolResponse {
    if let Some(result) = record.result
        && let Ok(result) = serde_json::from_value::<ToolResult>(result)
    {
        let result = execution::sanitize_result(result);
        if result.serialized_len() > max_response_bytes.max(1) as usize {
            return visible_error("persisted idempotent result exceeds the current response limit");
        }
        return execution::into_mcp_result(result).into();
    }
    rmcp::model::CallToolResult::structured(json!({
        "status": "pending",
        "idempotency_key": record.idempotency_key,
        "tool_name": record.tool_name,
        "approval_id": record.approval_id,
        "message": "An identical request is already in progress. Retry with the same idempotency_key to retrieve its result."
    }))
    .into()
}

fn validate_key(key: &str) -> Result<(), ErrorData> {
    if key.is_empty() || key.len() > 128 || key.trim() != key || key.chars().any(char::is_control) {
        Err(ErrorData::invalid_params(
            "idempotency_key must contain 1 to 128 bytes without surrounding whitespace or control characters",
            None,
        ))
    } else {
        Ok(())
    }
}

fn request_hash(tool_name: &str, arguments: &Value) -> String {
    let canonical = canonical_json(arguments);
    let bytes = serde_json::to_vec(&(tool_name, canonical)).unwrap_or_default();
    hex::encode(Sha256::digest(bytes))
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), canonical_json(&map[key])))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
        other => other.clone(),
    }
}

fn requires_web_credential_delivery(action: &str) -> bool {
    matches!(action, "create_api_token" | "create_service_account")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_hash_ignores_json_object_key_order() {
        assert_eq!(
            request_hash("tool", &json!({"a": 1, "b": 2})),
            request_hash("tool", &json!({"b": 2, "a": 1}))
        );
    }

    #[test]
    fn idempotency_keys_are_bounded_and_canonical() {
        assert!(validate_key("managed-call-01").is_ok());
        assert!(validate_key("").is_err());
        assert!(validate_key(" padded").is_err());
        assert!(validate_key("line\nbreak").is_err());
        assert!(validate_key(&"x".repeat(129)).is_err());
    }

    #[test]
    fn credential_creating_approvals_stay_on_the_web_surface() {
        assert!(requires_web_credential_delivery("create_api_token"));
        assert!(requires_web_credential_delivery("create_service_account"));
        assert!(!requires_web_credential_delivery("revoke_api_token"));
    }
}
