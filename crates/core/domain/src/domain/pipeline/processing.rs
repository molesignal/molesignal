// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Ordered built-in and VRL processing steps. Routing metadata never enters event fields.
use serde::Deserialize;
use serde_json::Value;

use super::realtime::{OutputKind, Routing, public_stream};
use crate::shared::{Error, Result};

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingKind {
    #[default]
    Vrl,
    Builtin,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProcessingStep {
    #[serde(default, alias = "function_name", alias = "name")]
    pub transform_name: String,
    #[serde(default)]
    pub kind: ProcessingKind,
    #[serde(default)]
    pub script: String,
    #[serde(default)]
    pub operation: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub routing: Routing,
}

pub fn parse_processing_steps(config: &Value) -> Result<Vec<ProcessingStep>> {
    let raw = if config.is_array() {
        config.clone()
    } else if let Some(steps) = config.get("steps") {
        steps.clone()
    } else if config.get("script").is_some() {
        serde_json::json!([config])
    } else if let Some(script) = config.as_str() {
        serde_json::json!([{"script":script}])
    } else {
        serde_json::json!([])
    };
    let steps: Vec<ProcessingStep> = serde_json::from_value(raw)
        .map_err(|_| Error::invalid("invalid pipeline processing steps"))?;
    for step in &steps {
        match step.kind {
            ProcessingKind::Vrl if step.script.trim().is_empty() => {
                return Err(Error::invalid("VRL script is required"));
            }
            ProcessingKind::Builtin => {
                if step.operation != "route" {
                    return Err(Error::invalid("unknown built-in pipeline operation"));
                }
                if step.routing.kind == OutputKind::Fixed {
                    public_stream(&step.target)?;
                }
                step.routing.validate()?;
            }
            _ => (),
        }
    }
    Ok(steps)
}
