// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Json,
    http::header::{CACHE_CONTROL, PRAGMA},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::Value;

use super::OperationExecution;
use crate::agent::model::{ApprovalRequest, Execution};

const ONE_TIME_RESULT_KEY: &str = "__one_time_result";

#[derive(Serialize)]
struct ExecuteApprovalResponse {
    #[serde(flatten)]
    execution: Execution,
    #[serde(skip_serializing_if = "Option::is_none")]
    one_time_result: Option<Value>,
}

pub(super) fn attach_one_time_result(mut verification: Value, result: Value) -> Value {
    if let Some(object) = verification.as_object_mut() {
        object.insert(ONE_TIME_RESULT_KEY.into(), result);
    }
    verification
}

pub(super) fn take_one_time_result(verification: &mut Value) -> Option<Value> {
    verification
        .as_object_mut()
        .and_then(|object| object.remove(ONE_TIME_RESULT_KEY))
}

pub(super) fn response(result: OperationExecution) -> Response {
    (
        [(CACHE_CONTROL, "private, no-store"), (PRAGMA, "no-cache")],
        Json(ExecuteApprovalResponse {
            execution: result.execution,
            one_time_result: result.one_time_result,
        }),
    )
        .into_response()
}

#[derive(Serialize)]
struct ReviewApprovalResponse {
    approval: ApprovalRequest,
    #[serde(skip_serializing_if = "Option::is_none")]
    execution: Option<ExecuteApprovalResponse>,
}

pub(super) fn review_response(
    approval: ApprovalRequest,
    execution: Option<OperationExecution>,
) -> Response {
    let execution = execution.map(|result| ExecuteApprovalResponse {
        execution: result.execution,
        one_time_result: result.one_time_result,
    });
    (
        [(CACHE_CONTROL, "private, no-store"), (PRAGMA, "no-cache")],
        Json(ReviewApprovalResponse {
            approval,
            execution,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        agent::model::ExecutionStatus,
        shared::{ids::Id, time::TimestampMicros},
    };

    #[test]
    fn one_time_result_is_removed_before_persistence() {
        let mut verification = attach_one_time_result(
            json!({"verified": true}),
            json!({"api_token": {"token": "secret"}}),
        );
        let one_time = take_one_time_result(&mut verification);
        assert_eq!(verification, json!({"verified": true}));
        assert_eq!(one_time, Some(json!({"api_token": {"token": "secret"}})));
    }

    #[test]
    fn execution_responses_are_not_cacheable() {
        let at = TimestampMicros(1);
        let response = response(OperationExecution {
            execution: Execution {
                id: Id::from_string("execution"),
                org_id: Id::from_string("org"),
                approval_request_id: Id::from_string("approval"),
                investigation_id: None,
                action: "create_service_account".into(),
                target: "account".into(),
                parameters: json!({}),
                idempotency_key: "key".into(),
                requested_by: Id::from_string("user"),
                approved_by: Vec::new(),
                status: ExecutionStatus::Succeeded,
                output_summary: Some("created".into()),
                error: None,
                verification: json!({"verified": true}),
                started_at: Some(at),
                finished_at: Some(at),
                created_at: at,
                updated_at: at,
            },
            one_time_result: Some(json!({"api_token": {"token": "secret"}})),
        });
        assert_eq!(response.headers()[CACHE_CONTROL], "private, no-store");
        assert_eq!(response.headers()[PRAGMA], "no-cache");
    }
}
