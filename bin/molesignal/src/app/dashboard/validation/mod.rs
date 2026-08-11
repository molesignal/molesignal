// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod semantic;

pub use semantic::{validate_dashboard_semantics, validate_dashboard_semantics_with};
use serde_json::Value;

use crate::{
    domain::dashboard::authoring::{VisualizationManifest, visualization_manifest},
    shared::{
        Error,
        contracts::{ContractIssue, ContractValidator, dashboard_model_validator},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardValidationMode {
    Native,
    GrafanaImport,
}

#[must_use]
pub fn validate_dashboard_model(
    model: &Value,
    mode: DashboardValidationMode,
) -> Vec<ContractIssue> {
    validate_dashboard_model_with(
        model,
        mode,
        dashboard_model_validator(),
        visualization_manifest(),
    )
}

#[must_use]
pub fn validate_dashboard_model_with(
    model: &Value,
    mode: DashboardValidationMode,
    validator: &ContractValidator,
    manifest: &VisualizationManifest,
) -> Vec<ContractIssue> {
    let mut issues = match mode {
        DashboardValidationMode::Native => validator.validate(model),
        DashboardValidationMode::GrafanaImport => validate_import_envelope(model),
    };
    if issues.is_empty() {
        issues.extend(validate_dashboard_semantics_with(model, manifest));
    }
    issues.truncate(50);
    issues
}

pub fn ensure_dashboard_model(model: &Value, mode: DashboardValidationMode) -> Result<(), Error> {
    let issues = validate_dashboard_model(model, mode);
    if issues.is_empty() {
        Ok(())
    } else {
        Err(Error::validation("dashboard model is invalid", issues))
    }
}

pub fn ensure_dashboard_model_with(
    model: &Value,
    mode: DashboardValidationMode,
    validator: &ContractValidator,
    manifest: &VisualizationManifest,
) -> Result<(), Error> {
    let issues = validate_dashboard_model_with(model, mode, validator, manifest);
    if issues.is_empty() {
        Ok(())
    } else {
        Err(Error::validation("dashboard model is invalid", issues))
    }
}

fn validate_import_envelope(model: &Value) -> Vec<ContractIssue> {
    let Some(object) = model.as_object() else {
        return vec![ContractIssue::new(
            "CONTRACT_TYPE",
            "",
            "dashboard model must be an object",
            true,
        )];
    };
    let mut issues = Vec::new();
    if object.get("engine").and_then(Value::as_str) != Some("molesignal-dashboard") {
        issues.push(ContractIssue::new(
            "INVALID_DASHBOARD_ENGINE",
            "/engine",
            "dashboard engine must be molesignal-dashboard",
            true,
        ));
    }
    if object.get("schemaVersion").and_then(Value::as_u64) != Some(2) {
        issues.push(ContractIssue::new(
            "UNSUPPORTED_DASHBOARD_VERSION",
            "/schemaVersion",
            "dashboard schemaVersion must be 2",
            true,
        ));
    }
    for field in ["variables", "annotations", "links", "elements"] {
        if !object.get(field).is_some_and(Value::is_array) {
            issues.push(ContractIssue::new(
                "CONTRACT_TYPE",
                format!("/{field}"),
                format!("{field} must be an array"),
                true,
            ));
        }
    }
    issues
}
