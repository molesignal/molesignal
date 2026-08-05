// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::LazyLock;

use jsonschema::{Draft, error::ValidationErrorKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use super::{canonical_json_bytes, sha256_hex};

pub const DASHBOARD_MODEL_V2_SCHEMA: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/contracts/dashboard/model/v2.schema.json"
));
pub const DASHBOARD_AUTHORING_V1_SCHEMA: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/contracts/dashboard/authoring/v1.schema.json"
));
pub const DASHBOARD_VISUALIZATIONS_V1: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/contracts/dashboard/visualizations/v1.json"
));

const DEFAULT_MAX_ISSUES: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractIssue {
    pub code: String,
    pub path: String,
    pub message: String,
    pub retryable: bool,
}

impl ContractIssue {
    #[must_use]
    pub fn new(
        code: impl Into<String>,
        path: impl Into<String>,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            code: code.into(),
            path: path.into(),
            message: message.into(),
            retryable,
        }
    }
}

#[derive(Debug, Error)]
pub enum ContractSchemaError {
    #[error("schema JSON is invalid: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("schema cannot be compiled: {0}")]
    InvalidSchema(String),
}

pub struct ContractValidator {
    validator: jsonschema::Validator,
    schema: Value,
    schema_hash: String,
    max_issues: usize,
}

impl ContractValidator {
    /// Compiles a Draft 2020-12 schema for repeated validation.
    pub fn compile(schema: Value) -> Result<Self, ContractSchemaError> {
        Self::compile_bounded(schema, DEFAULT_MAX_ISSUES)
    }

    pub fn compile_bounded(schema: Value, max_issues: usize) -> Result<Self, ContractSchemaError> {
        let validator = jsonschema::options()
            .with_draft(Draft::Draft202012)
            .build(&schema)
            .map_err(|error| ContractSchemaError::InvalidSchema(error.to_string()))?;
        let schema_hash = sha256_hex(canonical_json_bytes(&schema));
        Ok(Self {
            validator,
            schema,
            schema_hash,
            max_issues: max_issues.clamp(1, 100),
        })
    }

    pub fn from_json(schema: &str) -> Result<Self, ContractSchemaError> {
        Self::compile(serde_json::from_str(schema)?)
    }

    #[must_use]
    pub fn validate(&self, instance: &Value) -> Vec<ContractIssue> {
        self.validator
            .iter_errors(instance)
            .take(self.max_issues)
            .map(|error| {
                let path = error.instance_path().to_string();
                ContractIssue::new(issue_code(error.kind()), path, error.to_string(), true)
            })
            .collect()
    }

    #[must_use]
    pub fn is_valid(&self, instance: &Value) -> bool {
        self.validator.is_valid(instance)
    }

    #[must_use]
    pub fn schema(&self) -> &Value {
        &self.schema
    }

    #[must_use]
    pub fn schema_hash(&self) -> &str {
        &self.schema_hash
    }

    #[must_use]
    pub fn canonical_hash(&self, instance: &Value) -> String {
        sha256_hex(canonical_json_bytes(instance))
    }
}

static DASHBOARD_MODEL_VALIDATOR: LazyLock<ContractValidator> = LazyLock::new(|| {
    ContractValidator::from_json(DASHBOARD_MODEL_V2_SCHEMA)
        .expect("embedded Dashboard model v2 schema must compile")
});

static DASHBOARD_AUTHORING_VALIDATOR: LazyLock<ContractValidator> = LazyLock::new(|| {
    ContractValidator::from_json(DASHBOARD_AUTHORING_V1_SCHEMA)
        .expect("embedded Dashboard authoring v1 schema must compile")
});

#[must_use]
pub fn dashboard_model_validator() -> &'static ContractValidator {
    &DASHBOARD_MODEL_VALIDATOR
}

#[must_use]
pub fn dashboard_authoring_validator() -> &'static ContractValidator {
    &DASHBOARD_AUTHORING_VALIDATOR
}

fn issue_code(kind: &ValidationErrorKind) -> &'static str {
    match kind {
        ValidationErrorKind::AdditionalItems { .. }
        | ValidationErrorKind::AdditionalProperties { .. }
        | ValidationErrorKind::UnevaluatedItems { .. }
        | ValidationErrorKind::UnevaluatedProperties { .. } => "CONTRACT_ADDITIONAL_PROPERTY",
        ValidationErrorKind::Constant { .. } => "CONTRACT_CONST",
        ValidationErrorKind::Enum { .. } => "CONTRACT_ENUM",
        ValidationErrorKind::Required { .. } => "CONTRACT_REQUIRED",
        ValidationErrorKind::Type { .. } => "CONTRACT_TYPE",
        ValidationErrorKind::MaxItems { .. }
        | ValidationErrorKind::MaxLength { .. }
        | ValidationErrorKind::MaxProperties { .. }
        | ValidationErrorKind::Maximum { .. }
        | ValidationErrorKind::ExclusiveMaximum { .. } => "CONTRACT_MAXIMUM",
        ValidationErrorKind::MinItems { .. }
        | ValidationErrorKind::MinLength { .. }
        | ValidationErrorKind::MinProperties { .. }
        | ValidationErrorKind::Minimum { .. }
        | ValidationErrorKind::ExclusiveMinimum { .. } => "CONTRACT_MINIMUM",
        ValidationErrorKind::OneOfMultipleValid { .. }
        | ValidationErrorKind::OneOfNotValid { .. } => "CONTRACT_ONE_OF",
        ValidationErrorKind::AnyOf { .. } => "CONTRACT_ANY_OF",
        ValidationErrorKind::Pattern { .. } => "CONTRACT_PATTERN",
        ValidationErrorKind::UniqueItems => "CONTRACT_UNIQUE_ITEMS",
        ValidationErrorKind::Referencing(_) => "CONTRACT_REFERENCE",
        _ => "CONTRACT_VALIDATION_FAILED",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    const VALID_DASHBOARD: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/contracts/dashboard/fixtures/valid/dashboard-v2-nested.json"
    ));
    const INVALID_UNKNOWN_FIELD: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/contracts/dashboard/fixtures/invalid/dashboard-v2-unknown-field.json"
    ));
    const VALID_AUTHORING: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/contracts/dashboard/fixtures/valid/authoring-v1-promql.json"
    ));
    const FIXTURE_MANIFEST: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/contracts/dashboard/fixtures/manifest.json"
    ));

    #[test]
    fn embedded_contracts_compile_and_validate_shared_fixtures() {
        let dashboard: Value = serde_json::from_str(VALID_DASHBOARD).unwrap();
        let authoring: Value = serde_json::from_str(VALID_AUTHORING).unwrap();
        assert!(dashboard_model_validator().validate(&dashboard).is_empty());
        assert!(
            dashboard_authoring_validator()
                .validate(&authoring)
                .is_empty()
        );
    }

    #[test]
    fn issues_are_bounded_and_use_json_pointer_paths() {
        let invalid: Value = serde_json::from_str(INVALID_UNKNOWN_FIELD).unwrap();
        let issues = dashboard_model_validator().validate(&invalid);
        assert!(!issues.is_empty());
        assert!(issues.len() <= DEFAULT_MAX_ISSUES);
        assert!(
            issues
                .iter()
                .all(|issue| issue.path.is_empty() || issue.path.starts_with('/'))
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "CONTRACT_ADDITIONAL_PROPERTY")
        );
    }

    #[test]
    fn every_shared_fixture_has_the_expected_schema_validity() {
        let cases = [
            ("dashboard", true, VALID_DASHBOARD),
            ("authoring", true, VALID_AUTHORING),
            (
                "authoring",
                true,
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/contracts/dashboard/fixtures/valid/authoring-v1-typed-queries.json"
                )),
            ),
            ("dashboard", false, INVALID_UNKNOWN_FIELD),
            (
                // Duplicate IDs are a semantic, not JSON Schema, violation.
                "dashboard",
                true,
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/contracts/dashboard/fixtures/invalid/dashboard-v2-duplicate-id.json"
                )),
            ),
            (
                "dashboard",
                false,
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/contracts/dashboard/fixtures/invalid/dashboard-v3.json"
                )),
            ),
            (
                "authoring",
                false,
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/contracts/dashboard/fixtures/invalid/authoring-v2.json"
                )),
            ),
            (
                "authoring",
                false,
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/contracts/dashboard/fixtures/invalid/authoring-v1-unknown-field.json"
                )),
            ),
            (
                // Visualization/query compatibility is checked by the compiler.
                "authoring",
                true,
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/contracts/dashboard/fixtures/invalid/authoring-v1-incompatible-visualization.json"
                )),
            ),
        ];
        let manifest: Value = serde_json::from_str(FIXTURE_MANIFEST).unwrap();
        assert_eq!(manifest["cases"].as_array().unwrap().len(), cases.len());
        for (contract, expected, source) in cases {
            let value: Value = serde_json::from_str(source).unwrap();
            let actual = match contract {
                "dashboard" => dashboard_model_validator().is_valid(&value),
                "authoring" => dashboard_authoring_validator().is_valid(&value),
                _ => unreachable!(),
            };
            assert_eq!(actual, expected, "unexpected validity for {source}");
        }
    }
}
