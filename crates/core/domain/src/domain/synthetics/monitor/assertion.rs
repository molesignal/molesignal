// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use crate::shared::ids::Id;

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
#[serde(tag = "source", rename_all = "snake_case")]
pub enum ValueSource {
    Literal { value: String },
    Variable { name: String },
    Secret { reference: String, secret_id: Id },
}
