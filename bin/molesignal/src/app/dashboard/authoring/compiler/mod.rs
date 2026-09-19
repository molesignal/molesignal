// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod elements;
mod layout;
mod queries;
mod variables;

use serde_json::{Value, json};

use self::{elements::compile_elements, variables::compile_variables};
use crate::{
    app::dashboard::validation::{DashboardValidationMode, ensure_dashboard_model_with},
    domain::dashboard::authoring::{
        AuthoringRefresh, DashboardAuthoringSpec, VisualizationManifest, visualization_manifest,
    },
    shared::{
        Error, Result,
        contracts::{ContractIssue, ContractValidator, dashboard_model_validator},
    },
};

#[derive(Debug, Clone)]
pub struct CompiledDashboard {
    pub model: Value,
    pub model_hash: String,
    pub compiler_version: String,
    pub dashboard_model_version: u32,
}

pub struct DashboardAuthoringCompiler;

impl DashboardAuthoringCompiler {
    pub fn compile(spec: &DashboardAuthoringSpec) -> Result<CompiledDashboard> {
        Self::compile_with_contracts(spec, visualization_manifest(), dashboard_model_validator())
    }

    pub fn compile_with_contracts(
        spec: &DashboardAuthoringSpec,
        manifest: &VisualizationManifest,
        model_validator: &ContractValidator,
    ) -> Result<CompiledDashboard> {
        if !manifest
            .authoring_versions
            .contains(&spec.authoring_version)
        {
            return Err(Error::validation(
                "unsupported Dashboard authoring contract version",
                vec![ContractIssue::new(
                    "UNSUPPORTED_AUTHORING_VERSION",
                    "/authoringVersion",
                    format!(
                        "supported authoring versions: {:?}",
                        manifest.authoring_versions
                    ),
                    true,
                )],
            ));
        }
        let spec_value = serde_json::to_value(spec)
            .map_err(|error| crate::shared::Error::internal(error.to_string()))?;
        let spec_hash = model_validator.canonical_hash(&spec_value);
        let mut context = CompileContext::default();
        let elements = compile_elements(&spec.elements, &mut context, manifest)?;
        let variables = compile_variables(&spec.variables, &mut context)?;
        let time = spec.time_range.as_ref();
        let refresh = compile_refresh(spec.refresh.as_ref());
        let mut model = json!({
            "engine": "molesignal-dashboard",
            "schemaVersion": manifest.dashboard_model_version,
            "id": "",
            "uid": format!("ai-{}", &spec_hash[..24]),
            "title": spec.title.trim(),
            "tags": spec.tags,
            "editable": true,
            "defaultDashboard": false,
            "timeSettings": {
                "defaultFrom": time.map_or("now-6h", |value| value.from.as_str()),
                "defaultTo": time.map_or("now", |value| value.to.as_str()),
                "timezone": time.and_then(|value| value.timezone.as_deref()).unwrap_or("browser")
            },
            "refreshSettings": refresh,
            "variables": variables,
            "annotations": [],
            "links": [],
            "layout": {
                "type": "grid",
                "columns": manifest.grid.columns,
                "rowHeight": manifest.grid.row_height,
                "gap": manifest.grid.gap
            },
            "elements": elements,
            "version": 1
        });
        if let Some(description) = spec
            .description
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            model["description"] = Value::String(description.to_string());
        }
        if let Some(folder_id) = spec.folder_id.as_deref() {
            model["folderId"] = Value::String(folder_id.to_string());
        }
        ensure_dashboard_model_with(
            &model,
            DashboardValidationMode::Native,
            model_validator,
            manifest,
        )?;
        let model_hash = model_validator.canonical_hash(&model);
        Ok(CompiledDashboard {
            model,
            model_hash,
            compiler_version: manifest.compiler_version.clone(),
            dashboard_model_version: manifest.dashboard_model_version,
        })
    }
}

#[derive(Default)]
struct CompileContext {
    next_element_id: usize,
    next_variable_id: usize,
}

impl CompileContext {
    fn element_id(&mut self, prefix: &str) -> String {
        self.next_element_id += 1;
        format!("{prefix}-{:03}", self.next_element_id)
    }

    fn variable_id(&mut self) -> String {
        self.next_variable_id += 1;
        format!("variable-{:03}", self.next_variable_id)
    }
}

fn compile_refresh(refresh: Option<&AuthoringRefresh>) -> Value {
    let default_refresh;
    let refresh = match refresh {
        Some(refresh) => refresh,
        None => {
            default_refresh = AuthoringRefresh::Interval {
                interval: "30s".into(),
            };
            &default_refresh
        }
    };
    match refresh {
        AuthoringRefresh::Off => json!({
            "enabled": false,
            "mode": "off",
            "allowedIntervals": ["off", "5s", "10s", "30s", "1m", "5m"]
        }),
        AuthoringRefresh::Interval { interval } => {
            let mut allowed = vec!["off", "5s", "10s", "30s", "1m", "5m"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>();
            if !allowed.contains(interval) {
                allowed.push(interval.clone());
            }
            json!({
                "enabled": true,
                "mode": "interval",
                "defaultInterval": interval,
                "allowedIntervals": allowed
            })
        }
        AuthoringRefresh::Live => json!({
            "enabled": true,
            "mode": "live",
            "allowedIntervals": ["off"]
        }),
    }
}

#[cfg(test)]
mod tests;
