// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The OTLP answer to a logs or metrics request: `partial_success` counts the records that were
//! refused (a value that does not fit its field's type), which senders must not retry. A request
//! with nothing refused gets the empty answer.

use domain::intake::IntakeResult;
use opentelemetry_proto::tonic::collector::{
    logs::v1::{ExportLogsPartialSuccess, ExportLogsServiceResponse},
    metrics::v1::{ExportMetricsPartialSuccess, ExportMetricsServiceResponse},
};

pub(crate) fn logs_response(result: &IntakeResult) -> ExportLogsServiceResponse {
    if result.rejected == 0 {
        return ExportLogsServiceResponse::default();
    }

    ExportLogsServiceResponse {
        partial_success: Some(ExportLogsPartialSuccess {
            rejected_log_records: count(result.rejected),
            error_message: refusal(result, "log records"),
        }),
    }
}

pub(crate) fn metrics_response(result: &IntakeResult) -> ExportMetricsServiceResponse {
    if result.rejected == 0 {
        return ExportMetricsServiceResponse::default();
    }

    ExportMetricsServiceResponse {
        partial_success: Some(ExportMetricsPartialSuccess {
            rejected_data_points: count(result.rejected),
            error_message: refusal(result, "data points"),
        }),
    }
}

fn count(rejected: usize) -> i64 {
    i64::try_from(rejected).unwrap_or(i64::MAX)
}

/// How many `records` were refused, and why the first one was.
fn refusal(result: &IntakeResult, records: &str) -> String {
    match result.errors.first() {
        Some(error) => format!("{} {records} rejected: {}", result.rejected, error.reason),
        None => format!("{} {records} rejected", result.rejected),
    }
}

#[cfg(test)]
mod tests {
    use domain::intake::IntakeError;

    use super::*;

    fn result(rejected: usize) -> IntakeResult {
        IntakeResult {
            accepted: 3,
            rejected,
            errors: (0..rejected)
                .map(|index| IntakeError {
                    index,
                    reason: "type mismatch on field `value`: expected Bool, got Number".into(),
                })
                .collect(),
        }
    }

    #[test]
    fn a_fully_accepted_request_answers_without_partial_success() {
        assert!(logs_response(&result(0)).partial_success.is_none());
        assert!(metrics_response(&result(0)).partial_success.is_none());
    }

    #[test]
    fn refused_records_are_reported_as_partial_success() {
        let metrics = metrics_response(&result(2))
            .partial_success
            .expect("partial success");

        assert_eq!(metrics.rejected_data_points, 2);
        assert!(
            metrics
                .error_message
                .starts_with("2 data points rejected: type mismatch")
        );

        let logs = logs_response(&result(1))
            .partial_success
            .expect("partial success");

        assert_eq!(logs.rejected_log_records, 1);
        assert!(logs.error_message.contains("field `value`"));
    }
}
