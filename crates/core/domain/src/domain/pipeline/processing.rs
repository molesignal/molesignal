// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Ordered built-in and VRL processing steps. Routing metadata never enters event fields.
//!
//! Three kinds keep count across events and so run only as events arrive, in a realtime pipeline:
//! `generate_metric` ([`super::metric`]), `quota` and `throttle` ([`super::limit`]). They apply to
//! the events their `conditions` pick and change no field.
use serde::Deserialize;
use serde_json::Value;

use super::{
    condition::{Condition, FieldCondition},
    limit::{OverAction, QuotaSpec, ThrottleSpec},
    metric::MetricSpec,
    realtime::{OutputKind, Routing, writable_stream},
};
use crate::shared::{Error, Result};

/// The name of a step that keeps count is at most this long: what it counted is stored under it.
const MAX_NAME_LENGTH: usize = 255;

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingKind {
    #[default]
    Vrl,
    Builtin,
    GenerateMetric,
    Quota,
    Throttle,
}

impl ProcessingKind {
    /// Whether the step keeps count across events: it runs only in a realtime pipeline.
    pub fn keeps_count(self) -> bool {
        matches!(self, Self::GenerateMetric | Self::Quota | Self::Throttle)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    /// The events a step that keeps count applies to; every event when there are none.
    #[serde(default)]
    pub conditions: Vec<FieldCondition>,
    #[serde(default)]
    pub metric: Option<MetricSpec>,
    #[serde(default)]
    pub quota: Option<QuotaSpec>,
    #[serde(default)]
    pub throttle: Option<ThrottleSpec>,
    /// `conditions`, compiled by [`parse_processing_steps`].
    #[serde(skip)]
    pub condition: Condition,
}

impl ProcessingStep {
    /// The stream a quota or a throttle sends what goes past its limit to, when it sends it
    /// anywhere.
    pub fn over_limit_target(&self) -> Option<&str> {
        let (action, target) = match self.kind {
            ProcessingKind::Quota => self
                .quota
                .as_ref()
                .map(|spec| (spec.action, &spec.target))?,
            ProcessingKind::Throttle => self
                .throttle
                .as_ref()
                .map(|spec| (spec.action, &spec.target))?,
            _ => return None,
        };

        (action == OverAction::Route).then_some(target.as_str())
    }
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

    let mut steps: Vec<ProcessingStep> = serde_json::from_value(raw)
        .map_err(|error| Error::invalid(format!("invalid pipeline processing steps: {error}")))?;

    for step in &mut steps {
        match step.kind {
            ProcessingKind::Vrl if step.script.trim().is_empty() => {
                return Err(Error::invalid("VRL script is required"));
            }
            ProcessingKind::Builtin => {
                if step.operation != "route" {
                    return Err(Error::invalid("unknown built-in pipeline operation"));
                }

                if step.routing.kind == OutputKind::Fixed {
                    writable_stream(&step.target)?;
                }

                step.routing.validate()?;
            }
            kind if kind.keeps_count() => prepare_counting(step)?,
            _ => (),
        }
    }

    unique_counting_names(&steps)?;
    Ok(steps)
}

/// Checks a step that keeps count and compiles its condition.
fn prepare_counting(step: &mut ProcessingStep) -> Result<()> {
    let name = step.transform_name.trim().to_owned();

    let checked = match step.kind {
        ProcessingKind::GenerateMetric => step
            .metric
            .as_ref()
            .ok_or_else(|| Error::invalid(format!("step `{name}` names no metric")))
            .and_then(MetricSpec::validate),
        ProcessingKind::Quota => step
            .quota
            .as_mut()
            .ok_or_else(|| Error::invalid(format!("step `{name}` sets no quota")))
            .and_then(QuotaSpec::prepare),
        _ => step
            .throttle
            .as_ref()
            .ok_or_else(|| Error::invalid(format!("step `{name}` sets no throttle")))
            .and_then(ThrottleSpec::validate),
    };

    checked.map_err(|error| match error {
        Error::InvalidArgument(message) => Error::invalid(format!("step `{name}`: {message}")),
        other => other,
    })?;

    step.condition = Condition::compile(&step.conditions)?;
    Ok(())
}

/// What a step that keeps count has counted is kept under its name, so each has its own.
fn unique_counting_names(steps: &[ProcessingStep]) -> Result<()> {
    let names: Vec<&str> = steps
        .iter()
        .filter(|step| step.kind.keeps_count())
        .map(|step| step.transform_name.trim())
        .collect();

    for (index, name) in names.iter().enumerate() {
        if name.is_empty() {
            return Err(Error::invalid(
                "a step that keeps count needs a name: what it counted is kept under it",
            ));
        }

        if name.len() > MAX_NAME_LENGTH {
            return Err(Error::invalid(format!(
                "the name of step `{name}` is longer than {MAX_NAME_LENGTH} bytes"
            )));
        }

        if names[..index].contains(name) {
            return Err(Error::invalid(format!(
                "two steps are named `{name}`: a step that keeps count needs a name of its own"
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn counting_steps_are_checked_and_their_conditions_compiled() {
        let steps = parse_processing_steps(&json!({ "steps": [
            {
                "transformName": "errors",
                "kind": "generate_metric",
                "conditions": [{ "field": "level", "test": "equals", "value": "error" }],
                "metric": { "name": "app_errors", "type": "count", "stream": "app" },
            },
            {
                "transformName": "daily",
                "kind": "quota",
                "quota": { "unit": "bytes", "limit": 1048576, "action": "route", "target": "cold" },
            },
            {
                "transformName": "burst",
                "kind": "throttle",
                "throttle": { "limit": 10, "windowSecs": 1 },
            },
        ]}))
        .unwrap();

        assert!(steps.iter().all(|step| step.kind.keeps_count()));
        assert!(steps[0].condition.matches(&json!({ "level": "error" })));
        assert!(!steps[0].condition.matches(&json!({ "level": "info" })));
        assert_eq!(steps[1].over_limit_target(), Some("cold"));
        assert_eq!(steps[2].over_limit_target(), None);
    }

    #[test]
    fn a_counting_step_needs_its_settings_and_a_name_of_its_own() {
        for steps in [
            json!([{ "transformName": "m", "kind": "generate_metric" }]),
            json!([{ "transformName": "q", "kind": "quota" }]),
            json!([{ "transformName": "t", "kind": "throttle" }]),
            json!([{ "kind": "throttle", "throttle": { "limit": 1, "windowSecs": 1 } }]),
            json!([
                { "transformName": "t", "kind": "throttle", "throttle": { "limit": 1, "windowSecs": 1 } },
                { "transformName": "t", "kind": "quota", "quota": { "unit": "events", "limit": 1 } },
            ]),
            json!([{
                "transformName": "m",
                "kind": "generate_metric",
                "conditions": [{ "field": "status", "test": "gt", "value": "high" }],
                "metric": { "name": "m", "type": "count", "stream": "app" },
            }]),
        ] {
            assert!(
                parse_processing_steps(&json!({ "steps": steps })).is_err(),
                "{steps}"
            );
        }

        let error = parse_processing_steps(&json!({ "steps": [{
            "transformName": "latency",
            "kind": "generate_metric",
            "metric": { "name": "latency", "type": "gauge", "stream": "app" },
        }]}))
        .unwrap_err();

        assert!(error.to_string().contains("step `latency`"), "{error}");
    }
}
