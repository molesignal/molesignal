// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Ingest-time routing configuration and repository port.
//!
//! A realtime pipeline changes logs, metrics, traces or profile metadata as they arrive. Logs may be
//! routed to other streams; the others are changed in place, in the stream they arrive in, and keep
//! the fields [`super::protected`] names.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    alert::ALERT_MODE,
    processing::{ProcessingKind, parse_processing_steps},
    protected::Guard,
};
use crate::{
    domain::{
        rum::event::RumStream,
        stream::{
            DEFAULT_PROFILE_STREAM, StreamType, is_platform_stream, is_reserved_system_stream,
            validate_stream_name,
        },
    },
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    /// What its source stream holds: logs, metrics, traces or profile metadata.
    pub signal: StreamType,
    pub target: String,
    pub routing: Routing,
    pub steps: Vec<RealtimeStep>,
    pub updated_at: crate::shared::time::TimestampMicros,
}

/// How a saved realtime pipeline is set up: the data it changes and where its events go.
#[derive(Debug, Clone)]
pub struct RealtimeConfig {
    pub signal: StreamType,
    pub routing: Routing,
}

pub fn is_realtime(config: &Value) -> bool {
    config.get("mode").and_then(Value::as_str) == Some("realtime")
}

/// A stream a pipeline may read: a valid name, not the protected system stream.
pub fn public_stream(name: &str) -> Result<()> {
    validate_stream_name(name)?;

    if is_reserved_system_stream(name) {
        return Err(Error::invalid(
            "pipeline cannot target the protected system stream",
        ));
    }

    Ok(())
}

/// A stream a pipeline may write, or change as it arrives: a [`public_stream`] that is neither one
/// of the RUM streams nor one the platform writes ([`is_platform_stream`]: synthetic runs, the
/// audit trail, the agent's traces). RUM rows follow the RUM contract the RUM pages read, so only
/// RUM intake writes them; a pipeline may still read either, to copy them elsewhere.
pub fn writable_stream(name: &str) -> Result<()> {
    public_stream(name)?;

    if is_platform_stream(name) {
        return Err(Error::invalid(format!(
            "`{name}` is written by the platform only; a pipeline can read it but not write it"
        )));
    }

    if RumStream::from_name(name).is_some() {
        return Err(Error::invalid(format!(
            "`{name}` is written by RUM intake only; a pipeline can read it but not write it"
        )));
    }

    Ok(())
}

/// The data a realtime pipeline changes, as its configuration names it; logs when it names none.
fn realtime_signal(config: &Value) -> Result<StreamType> {
    match config
        .get("signalType")
        .and_then(Value::as_str)
        .unwrap_or("logs")
    {
        "logs" => Ok(StreamType::LOGS),
        "metrics" => Ok(StreamType::METRICS),
        "traces" => Ok(StreamType::TRACES),
        "profiles" => Ok(StreamType::PROFILES),
        _ => Err(Error::invalid(
            "realtime pipelines change logs, metrics, traces or profiles",
        )),
    }
}

pub fn validate_config(
    config: &Value,
    source: &str,
    target: &str,
) -> Result<Option<RealtimeConfig>> {
    match config.get("mode").and_then(Value::as_str) {
        None | Some("scheduled" | ALERT_MODE) => return Ok(None),
        Some("realtime") => (),
        _ => return Err(Error::invalid("unknown pipeline execution mode")),
    }

    let signal = realtime_signal(config)?;
    let rum = RumStream::from_name(source);

    // Profiles of an organization all arrive in one stream.
    if signal == StreamType::PROFILES && source != DEFAULT_PROFILE_STREAM {
        return Err(Error::invalid(format!(
            "profiles arrive in the `{DEFAULT_PROFILE_STREAM}` stream"
        )));
    }

    match rum {
        // Sessions are built from the views as they arrive: no pipeline changes them.
        Some(RumStream::Sessions) => {
            return Err(Error::invalid(
                "`rum_sessions` is built from the views; no pipeline changes it",
            ));
        }
        // A RUM stream is changed in place, in its custom parts (see `protected`).
        Some(_) if signal == StreamType::LOGS => public_stream(source)?,
        Some(_) => return Err(Error::invalid("RUM streams hold logs")),
        None => {
            writable_stream(source)?;
            writable_stream(target)?;
        }
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
        .get("sinkConnectors")
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

    if let Some(retain_source) = config.get("retainSource") {
        routing.retain_source = retain_source
            .as_bool()
            .ok_or_else(|| Error::invalid("retainSource must be a boolean"))?;
    }

    let steps = parse_processing_steps(config)?;

    check_counting(&steps, signal, rum, source)?;

    // Metrics, traces and RUM rows stay where they arrive: a series, a trace or a session split
    // over streams would no longer be one.
    if (signal != StreamType::LOGS || rum.is_some())
        && (target != source
            || routing.kind != OutputKind::Fixed
            || routing.retain_source
            || steps.iter().any(|step| {
                step.kind == ProcessingKind::Builtin || step.over_limit_target().is_some()
            }))
    {
        return Err(Error::invalid(
            "a realtime pipeline on metrics, traces, profiles or RUM changes its stream in place: \
             it writes back to the stream it reads and routes nothing elsewhere",
        ));
    }

    Ok(Some(RealtimeConfig { signal, routing }))
}

/// Where the steps that keep count can run. Metrics are made of logs, RUM events and spans. A quota
/// or a throttle holds back logs and the RUM events a step may drop: holding back samples or spans
/// would leave gaps in series and traces.
fn check_counting(
    steps: &[RealtimeStep],
    signal: StreamType,
    rum: Option<RumStream>,
    source: &str,
) -> Result<()> {
    for step in steps.iter().filter(|step| step.kind.keeps_count()) {
        let name = step.transform_name.trim();

        if step.kind == ProcessingKind::GenerateMetric {
            if signal != StreamType::LOGS && signal != StreamType::TRACES {
                return Err(Error::invalid(format!(
                    "step `{name}` makes metrics of logs, RUM events or spans, not of {}",
                    signal.external_name()
                )));
            }

            continue;
        }

        if signal != StreamType::LOGS {
            return Err(Error::invalid(format!(
                "step `{name}` limits logs and RUM events; holding back {} would leave gaps",
                signal.external_name()
            )));
        }

        if rum.is_some_and(|stream| !Guard::Rum(stream).drops()) {
            return Err(Error::invalid(format!(
                "`{source}` keeps every row, so step `{name}` cannot hold any back"
            )));
        }

        if step.over_limit_target() == Some(source) {
            return Err(Error::invalid(format!(
                "step `{name}` would send what goes past its limit back into `{source}`"
            )));
        }
    }

    Ok(())
}

impl Routing {
    pub fn validate(&self) -> Result<()> {
        if self.kind == OutputKind::Field {
            if self.field.trim().is_empty() || self.field.len() > 255 {
                return Err(Error::invalid("routing field is required"));
            }

            writable_stream(&self.fallback)?;

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

            if writable_stream(&stream).is_ok() {
                return stream;
            }
        }

        self.fallback.clone()
    }
}

#[async_trait]
pub trait RealtimePipelineRepository: Send + Sync {
    /// The pipeline enabled on the organization's `source` stream of this kind, if any.
    async fn for_source(
        &self,
        org: &Id,
        signal: StreamType,
        source: &str,
    ) -> Result<Option<RealtimePipeline>>;
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
        let mut config = json!({"mode":"realtime","signalType":"logs","steps":[],"routing":{"kind":"field","field":"appname","fallback":"default"}});
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
                &json!({"mode":"realtime","signalType":"metrics"}),
                "default",
                "target"
            )
            .is_err()
        );
    }

    #[test]
    fn an_alert_pipeline_has_no_realtime_setup_and_no_streams() {
        let config = json!({ "mode": "alert", "alert": { "sources": ["rule"] }, "steps": [] });

        assert!(validate_config(&config, "", "").unwrap().is_none());
    }

    #[test]
    fn metrics_and_traces_are_changed_in_the_stream_they_arrive_in() {
        for signal in ["metrics", "traces"] {
            let config = json!({ "mode": "realtime", "signalType": signal, "steps": [] });
            let setup = validate_config(&config, "prod", "prod").unwrap().unwrap();

            assert_eq!(setup.signal.external_name(), signal);
            assert!(validate_config(&config, "prod", "prod_copy").is_err());

            let mut retained = config.clone();
            retained["retainSource"] = json!(true);
            assert!(validate_config(&retained, "prod", "prod").is_err());

            let mut routed = config.clone();
            routed["steps"] = json!([{ "kind": "builtin", "target": "prod" }]);
            assert!(validate_config(&routed, "prod", "prod").is_err());
        }

        let extend = json!({ "mode": "realtime", "signalType": "extend", "steps": [] });
        assert!(validate_config(&extend, "prod", "prod").is_err());

        let logs = json!({ "mode": "realtime", "steps": [] });
        let setup = validate_config(&logs, "app", "app_copy").unwrap().unwrap();
        assert_eq!(setup.signal, StreamType::LOGS);
    }

    #[test]
    fn profile_metadata_is_changed_where_it_arrives() {
        let config = json!({ "mode": "realtime", "signalType": "profiles", "steps": [] });
        let setup = validate_config(&config, "default", "default")
            .unwrap()
            .unwrap();

        assert_eq!(setup.signal, StreamType::PROFILES);
        assert!(validate_config(&config, "default", "copy").is_err());
        assert!(validate_config(&config, "other", "other").is_err());
    }

    #[test]
    fn rum_streams_are_changed_only_in_place_and_never_written_from_elsewhere() {
        let config = json!({"mode":"realtime","signalType":"logs","steps":[]});

        assert!(validate_config(&config, "rum_views", "rum_views").is_ok());
        assert!(validate_config(&config, "rum_actions", "rum_actions").is_ok());
        assert!(validate_config(&config, "rum_views", "rum_copy").is_err());
        assert!(validate_config(&config, "rum_sessions", "rum_sessions").is_err());
        assert!(validate_config(&config, "web_logs", "rum_errors").is_err());

        let mut retained = config.clone();
        retained["retainSource"] = json!(true);
        assert!(validate_config(&retained, "rum_errors", "rum_errors").is_err());
        assert!(validate_config(&config, "web_logs", "rum_mirror").is_ok());

        let route = Routing {
            kind: OutputKind::Field,
            field: "kind".into(),
            prefix: "rum_".into(),
            fallback: "default".into(),
            ..Default::default()
        };

        assert_eq!(
            route.destination(&json!({"kind":"errors"}), "unused"),
            "default"
        );
        assert!(public_stream("rum_errors").is_ok());
        assert!(writable_stream("rum_errors").is_err());
    }

    #[test]
    fn counting_steps_run_where_their_data_allows() {
        let metric = json!({
            "transformName": "errors",
            "kind": "generate_metric",
            "metric": { "name": "errors", "type": "count", "stream": "app" },
        });
        let quota = json!({
            "transformName": "daily",
            "kind": "quota",
            "quota": { "unit": "events", "limit": 10 },
        });
        let overflow = json!({
            "transformName": "burst",
            "kind": "throttle",
            "throttle": { "limit": 10, "windowSecs": 60, "action": "route", "target": "cold" },
        });
        let config = |signal: &str, steps: Vec<Value>| json!({ "mode": "realtime", "signalType": signal, "steps": steps });

        assert!(validate_config(&config("logs", vec![metric.clone()]), "app", "app").is_ok());
        assert!(validate_config(&config("traces", vec![metric.clone()]), "app", "app").is_ok());
        assert!(
            validate_config(
                &config("logs", vec![metric.clone()]),
                "rum_views",
                "rum_views"
            )
            .is_ok()
        );
        assert!(validate_config(&config("metrics", vec![metric]), "app", "app").is_err());

        assert!(validate_config(&config("logs", vec![quota.clone()]), "app", "out").is_ok());
        assert!(
            validate_config(
                &config("logs", vec![quota.clone()]),
                "rum_errors",
                "rum_errors"
            )
            .is_ok()
        );
        assert!(
            validate_config(
                &config("logs", vec![quota.clone()]),
                "rum_views",
                "rum_views"
            )
            .is_err()
        );
        assert!(validate_config(&config("traces", vec![quota]), "app", "app").is_err());

        assert!(validate_config(&config("logs", vec![overflow.clone()]), "app", "out").is_ok());
        assert!(validate_config(&config("logs", vec![overflow.clone()]), "cold", "out").is_err());
        assert!(
            validate_config(&config("logs", vec![overflow]), "rum_errors", "rum_errors").is_err(),
            "RUM events stay in their stream"
        );
    }

    #[test]
    fn platform_streams_are_read_but_never_written() {
        for stream in ["_synthetic_results", "_audit", "_agent_model_traces"] {
            assert!(public_stream(stream).is_ok());
            assert!(writable_stream(stream).is_err());
        }

        assert!(public_stream("_molesignal").is_err());
        assert!(writable_stream("_my_scratch").is_ok());
    }
}
