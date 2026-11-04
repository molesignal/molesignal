// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use super::MonitorKind;
use crate::shared::ids::Id;

/// Whether an assertion of a `kind` check can look at `source`: what the Agent reads from the
/// reply of that kind of check.
pub fn assertion_source_allowed(kind: MonitorKind, source: &str) -> bool {
    match kind {
        MonitorKind::Http => {
            matches!(
                source,
                "status"
                    | "status_code"
                    | "body"
                    | "duration_ms"
                    | "size_bytes"
                    | "timing.dns_ms"
                    | "timing.connect_ms"
                    | "timing.tls_ms"
                    | "timing.first_byte_ms"
            ) || source
                .strip_prefix("header:")
                .is_some_and(|name| !name.trim().is_empty())
                || source
                    .strip_prefix("json:")
                    .is_some_and(|path| !path.trim().is_empty())
        }
        MonitorKind::Tcp => matches!(source, "response" | "duration_ms"),
        MonitorKind::Dns => matches!(source, "answers" | "answer_count" | "rcode" | "duration_ms"),
        MonitorKind::Grpc => matches!(source, "status" | "duration_ms"),
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertionSeverity {
    Warning,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operator", rename_all = "snake_case")]
pub enum AssertionOperator {
    Equals { expected: String },
    NotEquals { expected: String },
    Contains { expected: String },
    NotContains { expected: String },
    Matches { pattern: String },
    GreaterThan { expected: f64 },
    LessThan { expected: f64 },
    JsonSchema { schema: serde_json::Value },
    Exists,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonitorAssertion {
    pub id: Id,
    pub name: String,
    pub source: String,
    pub severity: AssertionSeverity,
    pub operator: AssertionOperator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extraction {
    pub variable: String,
    pub source: String,
    pub expression: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "source",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ValueSource {
    Literal { value: String },
    Variable { name: String },
    Secret { reference: String, secret_id: Id },
}
