// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Ingest-time routing configuration and repository port.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    domain::stream::{is_reserved_system_stream, validate_stream_name},
    shared::{Error, Result, ids::Id},
};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputKind {
    #[default]
    Fixed,
    Field,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Routing {
    #[serde(default)]
    pub kind: OutputKind,
    #[serde(default)]
    pub field: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub fallback: String,
    #[serde(default)]
    pub retain_source: bool,
}

pub use super::processing::ProcessingStep as RealtimeStep;

#[derive(Debug, Clone)]
pub struct RealtimePipeline {
    pub id: Id,
    pub org_id: Id,
    pub target: String,
    pub routing: Routing,
    pub steps: Vec<RealtimeStep>,
    pub updated_at: crate::shared::time::TimestampMicros,
}

pub fn is_realtime(config: &Value) -> bool {
    config.get("mode").and_then(Value::as_str) == Some("realtime")
}

pub fn public_stream(name: &str) -> Result<()> {
    validate_stream_name(name)?;
    if is_reserved_system_stream(name) {
        return Err(Error::invalid(
            "pipeline cannot target the protected system stream",
        ));
    }
    Ok(())
}

pub fn validate_config(config: &Value, source: &str, target: &str) -> Result<Option<Routing>> {
    match config.get("mode").and_then(Value::as_str) {
        None | Some("scheduled") => return Ok(None),
        Some("realtime") => (),
        _ => return Err(Error::invalid("unknown pipeline execution mode")),
    }
    public_stream(source)?;
    public_stream(target)?;
    if config
        .get("signal_type")
        .and_then(Value::as_str)
        .unwrap_or("logs")
        != "logs"
    {
        return Err(Error::invalid("realtime pipelines currently support logs"));
    }
    for (key, expected) in [("sources", source), ("sinks", target)] {
        if let Some(value) = config.get(key) {
            let values = value
                .as_array()
                .ok_or_else(|| Error::invalid("pipeline streams must be arrays"))?;
            if values.len() != 1 || values[0].as_str() != Some(expected) {
                return Err(Error::invalid(
                    "realtime pipelines require one source and one output node",
                ));
            }
        }
    }
    if config
        .get("sink_connectors")
        .is_some_and(|v| v.as_array().is_none_or(|a| !a.is_empty()))
    {
        return Err(Error::invalid("realtime output must be a data stream"));
    }
    let mut routing: Routing = serde_json::from_value(
        config
            .get("routing")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    )
    .map_err(|_| Error::invalid("invalid realtime routing configuration"))?;
    routing.validate()?;
    if let Some(retain_source) = config.get("retain_source") {
        routing.retain_source = retain_source
            .as_bool()
            .ok_or_else(|| Error::invalid("retain_source must be a boolean"))?;
    }
    super::processing::parse_processing_steps(config)?;
    Ok(Some(routing))
}

impl Routing {
    pub fn validate(&self) -> Result<()> {
        if self.kind == OutputKind::Field {
            if self.field.trim().is_empty() || self.field.len() > 255 {
                return Err(Error::invalid("routing field is required"));
            }
            public_stream(&self.fallback)?;
            if self.prefix.len() > 254
                || !self
                    .prefix
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
            {
                return Err(Error::invalid("invalid stream prefix"));
            }
        }
        Ok(())
    }

    /// Prefer literal dotted keys, then traverse nested JSON. Invalid values use the fallback.
    pub fn destination(&self, event: &Value, fixed: &str) -> String {
        if self.kind == OutputKind::Fixed {
            return fixed.to_owned();
        }
        let value = event
            .get(&self.field)
            .or_else(|| self.field.split('.').try_fold(event, |v, key| v.get(key)));
        if let Some(value) = value.and_then(Value::as_str).filter(|v| !v.is_empty()) {
            let stream = format!("{}{value}", self.prefix);
            if public_stream(&stream).is_ok() {
                return stream;
            }
        }
        self.fallback.clone()
    }
}

#[async_trait]
pub trait RealtimePipelineRepository: Send + Sync {
    async fn for_source(&self, org: &Id, source: &str) -> Result<Option<RealtimePipeline>>;
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    #[test]
    fn routes_dotted_keys_and_rejects_invalid_or_reserved_values() {
        let route = Routing {
            kind: OutputKind::Field,
            field: "service.name".into(),
            fallback: "default".into(),
            ..Default::default()
        };
        assert_eq!(
            route.destination(
                &json!({"service.name":"literal","service":{"name":"nested"}}),
                "unused"
            ),
            "literal"
        );
        assert_eq!(
            route.destination(&json!({"service":{"name":"nested"}}), "unused"),
            "nested"
        );
        for v in [
            json!(null),
            json!(42),
            json!(true),
            json!("../bad"),
            json!("_molesignal"),
            json!(""),
        ] {
            assert_eq!(
                route.destination(&json!({"service.name":v}), "unused"),
                "default"
            );
        }
    }
    #[test]
    fn realtime_accepts_source_as_output_but_requires_valid_routing() {
        let mut config = json!({"mode":"realtime","signal_type":"logs","steps":[],"routing":{"kind":"field","field":"appname","fallback":"default"}});
        assert!(
            validate_config(&config, "default", "default")
                .unwrap()
                .is_some()
        );
        config["routing"]["fallback"] = json!("_molesignal");
        assert!(validate_config(&config, "default", "default").is_err());
        assert!(validate_config(&json!({"mode":"typo"}), "default", "target").is_err());
        assert!(
            validate_config(
                &json!({"mode":"realtime","signal_type":"metrics"}),
                "default",
                "target"
            )
            .is_err()
        );
    }
}
