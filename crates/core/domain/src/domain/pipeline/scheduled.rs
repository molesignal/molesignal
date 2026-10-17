// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a scheduled pipeline reads, writes and does when something fails, as its saved
//! configuration (`function_steps`) says. Saving checks it and every run reads it through
//! [`ScheduledConfig::parse`], so a pipeline that saves is one that runs as it was saved.
use serde_json::Value;

use super::{
    processing::{ProcessingKind, parse_processing_steps},
    realtime::{OutputKind, public_stream},
};
use crate::shared::{Error, Result};

/// At most this many streams on either side of a pipeline.
pub const MAX_STREAMS: usize = 8;
/// How long a run leaves the latest stretch of time for late events, unless the pipeline says.
pub const DEFAULT_DELAY_SECS: i64 = 60;
pub const MAX_DELAY_SECS: i64 = 3_600;
pub const DEFAULT_RUN_RETRIES: u32 = 3;
pub const DEFAULT_DELIVERY_RETRIES: u32 = 3;
pub const MAX_RETRIES: u32 = 10;
const MAX_FIELD_LENGTH: usize = 255;

/// How often something that failed is tried again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// More attempts at a window whose read or write failed, after a backoff, before it is skipped.
    pub run_retries: u32,
    /// More attempts at a connector delivery within the run, before its events become dead letters.
    pub delivery_retries: u32,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            run_retries: DEFAULT_RUN_RETRIES,
            delivery_retries: DEFAULT_DELIVERY_RETRIES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledConfig {
    /// The streams read, the pipeline's `source_stream` first.
    pub sources: Vec<String>,
    /// The streams written, the pipeline's `target_stream` first. Each takes every event, except
    /// that a routing step decides where the first one's events go.
    pub sinks: Vec<String>,
    /// Connectors every event is delivered to.
    pub connectors: Vec<String>,
    /// Within a run, an event whose value of this field was already seen is dropped.
    pub dedupe_field: Option<String>,
    /// How long the latest stretch of time is left for late events before a run reads it.
    pub delay_secs: i64,
    pub retry: RetryPolicy,
    /// The logs stream that events which could not be processed, written or delivered go to.
    pub dead_letter_stream: Option<String>,
}

impl ScheduledConfig {
    pub fn parse(config: &Value, source_stream: &str, target_stream: &str) -> Result<Self> {
        if source_stream.trim().is_empty() || target_stream.trim().is_empty() {
            return Err(Error::invalid(
                "pipeline source_stream and target_stream cannot be empty",
            ));
        }

        let sources = stream_list(config, "sources", source_stream)?;
        let sinks = stream_list(config, "sinks", target_stream)?;

        if let Some(name) = sinks.iter().find(|sink| sources.contains(sink)) {
            return Err(Error::conflict(format!(
                "pipeline cannot write to `{name}`, which it reads"
            )));
        }

        for step in parse_processing_steps(config)? {
            if step.kind != ProcessingKind::Builtin {
                continue;
            }

            let stream = match step.routing.kind {
                OutputKind::Fixed => &step.target,
                OutputKind::Field => &step.routing.fallback,
            };

            if sources.contains(stream) {
                return Err(Error::invalid(format!(
                    "a routing step cannot write to `{stream}`, which the pipeline reads"
                )));
            }
        }

        let dead_letter_stream = optional_text(config, "dead_letter_stream")?;

        if let Some(name) = &dead_letter_stream {
            public_stream(name)?;

            if sources.contains(name) || sinks.contains(name) {
                return Err(Error::invalid(format!(
                    "dead letter stream `{name}` cannot be a stream the pipeline reads or writes"
                )));
            }
        }

        Ok(Self {
            sources,
            sinks,
            connectors: connectors(config)?,
            dedupe_field: optional_text(config, "dedupe_field")?,
            delay_secs: bounded(config.get("delay_secs"), DEFAULT_DELAY_SECS, MAX_DELAY_SECS)
                .ok_or_else(|| {
                    Error::invalid(format!(
                        "delay_secs must be a whole number from 0 to {MAX_DELAY_SECS}"
                    ))
                })?,
            retry: retry_policy(config.get("retry"))?,
            dead_letter_stream,
        })
    }
}

/// The pipeline's own stream, then the names the list adds: trimmed, without blanks or repeats.
fn stream_list(config: &Value, key: &str, first: &str) -> Result<Vec<String>> {
    let mut names = vec![first.trim().to_owned()];

    if let Some(value) = config.get(key) {
        let values = value
            .as_array()
            .ok_or_else(|| Error::invalid("pipeline streams must be arrays"))?;

        for value in values {
            let name = value
                .as_str()
                .ok_or_else(|| Error::invalid("pipeline streams must be stream names"))?
                .trim();

            if !name.is_empty() && !names.iter().any(|known| known == name) {
                names.push(name.to_owned());
            }
        }
    }

    if names.len() > MAX_STREAMS {
        return Err(Error::invalid(format!(
            "a pipeline reads at most {MAX_STREAMS} streams and writes at most {MAX_STREAMS}"
        )));
    }

    for name in &names {
        public_stream(name)?;
    }

    Ok(names)
}

fn connectors(config: &Value) -> Result<Vec<String>> {
    let Some(value) = config.get("sink_connectors") else {
        return Ok(Vec::new());
    };

    let values = value
        .as_array()
        .ok_or_else(|| Error::invalid("sink_connectors must be an array of connector ids"))?;

    let mut ids: Vec<String> = Vec::with_capacity(values.len());

    for value in values {
        let id = value
            .as_str()
            .ok_or_else(|| Error::invalid("sink_connectors must be an array of connector ids"))?
            .trim();

        if !id.is_empty() && !ids.iter().any(|known| known == id) {
            ids.push(id.to_owned());
        }
    }

    Ok(ids)
}

/// A text setting that may be left out or blank.
fn optional_text(config: &Value, key: &str) -> Result<Option<String>> {
    let text = match config.get(key) {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(text)) => text.trim(),
        Some(_) => return Err(Error::invalid(format!("{key} must be text"))),
    };

    if text.len() > MAX_FIELD_LENGTH {
        return Err(Error::invalid(format!(
            "{key} must be at most {MAX_FIELD_LENGTH} characters"
        )));
    }

    Ok((!text.is_empty()).then(|| text.to_owned()))
}

/// A whole number from 0 to `max`, `default` when it is left out; `None` when it is anything else.
fn bounded(value: Option<&Value>, default: i64, max: i64) -> Option<i64> {
    match value {
        None | Some(Value::Null) => Some(default),
        Some(value) => value.as_i64().filter(|number| (0..=max).contains(number)),
    }
}

fn retry_policy(value: Option<&Value>) -> Result<RetryPolicy> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(RetryPolicy::default());
    };

    if !value.is_object() {
        return Err(Error::invalid("retry must be an object"));
    }

    let count = |key: &str, default: u32| {
        bounded(value.get(key), i64::from(default), i64::from(MAX_RETRIES))
            .and_then(|count| u32::try_from(count).ok())
            .ok_or_else(|| {
                Error::invalid(format!(
                    "retry.{key} must be a whole number from 0 to {MAX_RETRIES}"
                ))
            })
    };

    Ok(RetryPolicy {
        run_retries: count("run_retries", DEFAULT_RUN_RETRIES)?,
        delivery_retries: count("delivery_retries", DEFAULT_DELIVERY_RETRIES)?,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_pipeline_streams_lead_their_lists_and_repeats_are_dropped() {
        let config = ScheduledConfig::parse(
            &json!({
                "sources": ["nginx", "app", " app ", ""],
                "sinks": ["archive", "clean"],
                "sink_connectors": ["c1", "c1"],
            }),
            "app",
            "clean",
        )
        .unwrap();

        assert_eq!(config.sources, vec!["app", "nginx"]);
        assert_eq!(config.sinks, vec!["clean", "archive"]);
        assert_eq!(config.connectors, vec!["c1"]);
    }

    #[test]
    fn what_is_left_out_takes_the_defaults() {
        let config = ScheduledConfig::parse(&json!([]), "app", "clean").unwrap();

        assert_eq!(config.sources, vec!["app"]);
        assert_eq!(config.sinks, vec!["clean"]);
        assert!(config.connectors.is_empty());
        assert_eq!(config.dedupe_field, None);
        assert_eq!(config.delay_secs, DEFAULT_DELAY_SECS);
        assert_eq!(config.retry, RetryPolicy::default());
        assert_eq!(config.dead_letter_stream, None);
    }

    #[test]
    fn a_stream_both_read_and_written_is_a_conflict() {
        for (config, source, target) in [
            (json!({}), "app", "app"),
            (
                json!({ "sinks": ["clean", "nginx"], "sources": ["nginx"] }),
                "app",
                "clean",
            ),
        ] {
            let err = ScheduledConfig::parse(&config, source, target).unwrap_err();

            assert_eq!(err.http_status_code(), 409, "{err}");
        }
    }

    #[test]
    fn a_route_into_a_source_is_refused_when_saved() {
        let config = json!({
            "sources": ["nginx"],
            "steps": [{ "kind": "builtin", "operation": "route", "target": "nginx" }],
        });

        assert!(ScheduledConfig::parse(&config, "app", "clean").is_err());

        let config = json!({
            "steps": [{
                "kind": "builtin",
                "operation": "route",
                "routing": { "kind": "field", "field": "appname", "fallback": "app" },
            }],
        });

        assert!(ScheduledConfig::parse(&config, "app", "clean").is_err());
    }

    #[test]
    fn stream_names_and_counts_are_checked() {
        let many: Vec<String> = (0..MAX_STREAMS).map(|n| format!("s{n}")).collect();

        assert!(ScheduledConfig::parse(&json!({ "sinks": many }), "app", "clean").is_err());
        assert!(ScheduledConfig::parse(&json!({ "sources": ["../x"] }), "app", "clean").is_err());
        assert!(
            ScheduledConfig::parse(&json!({ "sinks": ["_molesignal"] }), "app", "clean").is_err()
        );
        assert!(ScheduledConfig::parse(&json!({ "sources": "app" }), "app", "clean").is_err());
        assert!(ScheduledConfig::parse(&json!({}), " ", "clean").is_err());
    }

    #[test]
    fn retry_delay_and_dead_letters_are_read_and_bounded() {
        let config = ScheduledConfig::parse(
            &json!({
                "delay_secs": 0,
                "retry": { "run_retries": 0, "delivery_retries": 10 },
                "dead_letter_stream": " failed_events ",
                "dedupe_field": "trace_id",
            }),
            "app",
            "clean",
        )
        .unwrap();

        assert_eq!(config.delay_secs, 0);
        assert_eq!(
            config.retry,
            RetryPolicy {
                run_retries: 0,
                delivery_retries: 10
            }
        );
        assert_eq!(config.dead_letter_stream.as_deref(), Some("failed_events"));
        assert_eq!(config.dedupe_field.as_deref(), Some("trace_id"));

        for bad in [
            json!({ "delay_secs": -1 }),
            json!({ "delay_secs": MAX_DELAY_SECS + 1 }),
            json!({ "delay_secs": "60" }),
            json!({ "retry": { "run_retries": 11 } }),
            json!({ "retry": 3 }),
            json!({ "dead_letter_stream": "clean" }),
            json!({ "dead_letter_stream": "app" }),
            json!({ "dead_letter_stream": 7 }),
        ] {
            assert!(
                ScheduledConfig::parse(&bad, "app", "clean").is_err(),
                "{bad} should be refused"
            );
        }
    }
}
