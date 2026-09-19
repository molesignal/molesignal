// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use rmcp::{
    ErrorData, RoleServer,
    model::{
        CancelTaskParams, CreateTaskResult, DetailedTask, GetTaskParams, GetTaskResult,
        InputRequests, JsonObject, Task, TaskPayload, TaskStatus, UpdateTaskParams,
    },
    service::RequestContext,
};
use serde_json::{Value, json};
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

use super::{
    handler::{InboundMcpHandler, InboundRequestData, protocol_error, request_data},
    tools::{ResolvedTool, execution},
};
use crate::{
    agent::inbound_mcp::{InboundMcpTask, InboundMcpTaskStatus},
    shared::{Result, ids::Id, time::TimestampMicros},
};

const TASK_TTL_MICROS: i64 = 24 * 60 * 60 * 1_000_000;
const TASK_POLL_INTERVAL_MS: u64 = 1_000;

pub(super) async fn spawn_read(
    handler: &InboundMcpHandler,
    data: &InboundRequestData,
    resolved: ResolvedTool,
    arguments: Value,
) -> Result<CreateTaskResult, ErrorData> {
    let now = TimestampMicros::now();
    let task = handler
        .state
        .agent
        .inbound_mcp
        .create_task(InboundMcpTask {
            id: Id::new(),
            org_id: data.iam.org_id.clone(),
            principal_type: data.iam.principal_type().as_str().into(),
            principal_id: data.iam.principal_id().clone(),
            tool_name: resolved.spec.name.clone(),
            request: arguments.clone(),
            status: InboundMcpTaskStatus::Working,
            status_message: Some("Tool call is running".into()),
            input_requests: None,
            input_responses: None,
            result: None,
            error: None,
            cancel_requested: false,
            expires_at: TimestampMicros(now.0.saturating_add(TASK_TTL_MICROS)),
            created_at: now,
            updated_at: now,
        })
        .await
        .map_err(protocol_error)?;
    let cancellation = CancellationToken::new();
    handler
        .runtime
        .task_cancellations
        .insert(task.id.0.clone(), cancellation.clone());
    let worker_handler = handler.clone();
    let worker_data = data.clone();
    let worker_task = task.clone();
    crate::shared::trace_context::spawn_with_current_trace_context(async move {
        run_task(
            worker_handler,
            worker_data,
            worker_task,
            resolved,
            arguments,
            cancellation,
        )
        .await;
    });
    Ok(CreateTaskResult::new(base_task(&task)))
}

async fn run_task(
    handler: InboundMcpHandler,
    data: InboundRequestData,
    mut task: InboundMcpTask,
    resolved: ResolvedTool,
    arguments: Value,
    cancellation: CancellationToken,
) {
    let execution =
        execution::execute_resolved(&handler, &data, resolved, arguments, cancellation.clone());
    tokio::pin!(execution);
    let mut cancellation_poll =
        tokio::time::interval(std::time::Duration::from_millis(TASK_POLL_INTERVAL_MS));
    cancellation_poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let result = loop {
        tokio::select! {
            result = &mut execution => break result,
            _ = cancellation_poll.tick(), if !cancellation.is_cancelled() => {
                match handler
                    .state
                    .agent
                    .inbound_mcp
                    .get_task(
                        &task.org_id,
                        &task.principal_type,
                        &task.principal_id,
                        &task.id,
                    )
                    .await
                {
                    Ok(current) if current.cancel_requested => cancellation.cancel(),
                    Ok(_) => {}
                    Err(error) => tracing::warn!(
                        error = %error,
                        task_id = %task.id.0,
                        "failed to poll Inbound MCP task cancellation",
                    ),
                }
            }
        }
    };
    task.updated_at = TimestampMicros::now();
    if cancellation.is_cancelled() {
        task.status = InboundMcpTaskStatus::Cancelled;
        task.status_message = Some("Task was cancelled".into());
    } else {
        match result {
            Ok(result) if result.is_error => {
                task.status = InboundMcpTaskStatus::Failed;
                task.status_message = Some("Tool call failed".into());
                task.error = Some(json!({
                    "code": -32603,
                    "message": result.first_text().unwrap_or_else(|| "tool call failed".into()),
                }));
            }
            Ok(result) => {
                task.status = InboundMcpTaskStatus::Completed;
                task.status_message = Some("Tool call completed".into());
                task.result = serde_json::to_value(execution::into_mcp_result(result)).ok();
            }
            Err(error) => {
                task.status = InboundMcpTaskStatus::Failed;
                task.status_message = Some("Tool call failed".into());
                task.error = Some(json!({ "code": -32603, "message": error.to_string() }));
            }
        }
    }
    if let Err(error) = handler
        .state
        .agent
        .inbound_mcp
        .update_task(task.clone())
        .await
    {
        tracing::warn!(error = %error, task_id = %task.id.0, "failed to persist Inbound MCP task result");
    }
    handler.runtime.task_cancellations.remove(&task.id.0);
}

pub(super) async fn get(
    handler: &InboundMcpHandler,
    request: GetTaskParams,
    context: &RequestContext<RoleServer>,
) -> Result<GetTaskResult, ErrorData> {
    validate_task_id(&request.task_id)?;
    let data = request_data(handler, context)?;
    let task = load(handler, &data, &request.task_id).await?;
    let payload_bytes = serde_json::to_vec(&(&task.input_requests, &task.result, &task.error))
        .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
    if payload_bytes.len() > data.settings.max_response_bytes.max(1) as usize {
        return Err(ErrorData::invalid_request(
            "task payload exceeds the current response limit",
            None,
        ));
    }
    Ok(GetTaskResult::new(detailed_task(&task)?))
}

pub(super) async fn update(
    handler: &InboundMcpHandler,
    request: UpdateTaskParams,
    context: &RequestContext<RoleServer>,
) -> Result<(), ErrorData> {
    validate_task_id(&request.task_id)?;
    let data = request_data(handler, context)?;
    let mut task = load(handler, &data, &request.task_id).await?;
    if task.status != InboundMcpTaskStatus::InputRequired {
        return Err(ErrorData::invalid_request(
            "task is not waiting for input",
            None,
        ));
    }
    let outstanding = task
        .input_requests
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| ErrorData::internal_error("task input requests are invalid", None))?;
    if request.input_responses.is_empty()
        || request
            .input_responses
            .keys()
            .any(|key| !outstanding.contains_key(key))
    {
        return Err(ErrorData::invalid_params(
            "inputResponses must match outstanding task input request keys",
            None,
        ));
    }
    task.input_responses = serde_json::to_value(request.input_responses).ok();
    task.updated_at = TimestampMicros::now();
    handler
        .state
        .agent
        .inbound_mcp
        .update_task(task)
        .await
        .map_err(protocol_error)?;
    Ok(())
}

pub(super) async fn cancel(
    handler: &InboundMcpHandler,
    request: CancelTaskParams,
    context: &RequestContext<RoleServer>,
) -> Result<(), ErrorData> {
    validate_task_id(&request.task_id)?;
    let data = request_data(handler, context)?;
    let task = load(handler, &data, &request.task_id).await?;
    handler
        .state
        .agent
        .inbound_mcp
        .request_task_cancellation(
            &data.iam.org_id,
            data.iam.principal_type().as_str(),
            data.iam.principal_id(),
            &task.id,
        )
        .await
        .map_err(protocol_error)?;
    if let Some(cancellation) = handler.runtime.task_cancellations.get(&task.id.0) {
        cancellation.cancel();
    }
    Ok(())
}

async fn load(
    handler: &InboundMcpHandler,
    data: &InboundRequestData,
    task_id: &str,
) -> Result<InboundMcpTask, ErrorData> {
    let task = handler
        .state
        .agent
        .inbound_mcp
        .get_task(
            &data.iam.org_id,
            data.iam.principal_type().as_str(),
            data.iam.principal_id(),
            &Id::from_string(task_id),
        )
        .await
        .map_err(protocol_error)?;
    if task.expires_at.0 <= TimestampMicros::now().0 {
        return Err(ErrorData::resource_not_found("task has expired", None));
    }
    Ok(task)
}

fn base_task(task: &InboundMcpTask) -> Task {
    Task::new(
        task.id.0.clone(),
        task_status(task.status),
        iso_timestamp(task.created_at),
        iso_timestamp(task.updated_at),
    )
    .with_status_message(task.status_message.clone().unwrap_or_default())
    .with_ttl_ms(
        u64::try_from(task.expires_at.0.saturating_sub(task.created_at.0) / 1_000)
            .unwrap_or_default(),
    )
    .with_poll_interval_ms(TASK_POLL_INTERVAL_MS)
}

fn detailed_task(task: &InboundMcpTask) -> Result<DetailedTask, ErrorData> {
    let payload = match task.status {
        InboundMcpTaskStatus::Working => TaskPayload::Working,
        InboundMcpTaskStatus::Cancelled => TaskPayload::Cancelled,
        InboundMcpTaskStatus::InputRequired => TaskPayload::InputRequired {
            input_requests: serde_json::from_value::<InputRequests>(
                task.input_requests.clone().unwrap_or_else(|| json!({})),
            )
            .map_err(|error| ErrorData::internal_error(error.to_string(), None))?,
        },
        InboundMcpTaskStatus::Completed => TaskPayload::Completed {
            result: object_payload(task.result.clone(), "result")?,
        },
        InboundMcpTaskStatus::Failed => TaskPayload::Failed {
            error: object_payload(task.error.clone(), "error")?,
        },
    };
    Ok(DetailedTask::new(base_task(task), payload))
}

fn object_payload(value: Option<Value>, field: &str) -> Result<JsonObject, ErrorData> {
    value
        .and_then(|value| value.as_object().cloned())
        .ok_or_else(|| ErrorData::internal_error(format!("task {field} is invalid"), None))
}

fn task_status(status: InboundMcpTaskStatus) -> TaskStatus {
    match status {
        InboundMcpTaskStatus::Working => TaskStatus::Working,
        InboundMcpTaskStatus::InputRequired => TaskStatus::InputRequired,
        InboundMcpTaskStatus::Completed => TaskStatus::Completed,
        InboundMcpTaskStatus::Failed => TaskStatus::Failed,
        InboundMcpTaskStatus::Cancelled => TaskStatus::Cancelled,
    }
}

fn iso_timestamp(timestamp: TimestampMicros) -> String {
    chrono::DateTime::from_timestamp_micros(timestamp.0)
        .unwrap_or(chrono::DateTime::UNIX_EPOCH)
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

fn validate_task_id(task_id: &str) -> Result<(), ErrorData> {
    if task_id.is_empty() || task_id.len() > 128 {
        Err(ErrorData::invalid_params("invalid taskId", None))
    } else {
        Ok(())
    }
}
