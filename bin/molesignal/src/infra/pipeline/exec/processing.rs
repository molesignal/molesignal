// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Ordered processing with destination metadata separate from the event payload.
//!
//! Every event goes through the steps in order. A VRL step changes it, or drops it on purpose
//! (`abort`), or fails on it; a routing step picks the stream it goes to. A step reads the stream
//! the event was read from as `%source_stream`, which never becomes part of the event.
use serde_json::{Map, Value};

use crate::{
    domain::{
        function::FunctionOutcome,
        pipeline::processing::{ProcessingKind, parse_processing_steps},
    },
    infra::runtime::VrlRuntime,
    shared::{Error, Result},
};

/// The VRL metadata key that holds the stream an event was read from.
pub const SOURCE_STREAM_METADATA: &str = "source_stream";

/// An event as read, with the stream it was read from.
#[derive(Debug, Clone)]
pub struct SourceEvent {
    pub source: usize,
    pub event: Value,
}

/// An event that came through every step.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessedEvent {
    pub source: usize,
    /// The stream a routing step chose, or the pipeline's first destination.
    pub destination: String,
    pub event: Value,
}

/// An event a step failed on.
#[derive(Debug, Clone, PartialEq)]
pub struct FailedEvent {
    pub source: usize,
    /// The event as it was read, kept when it is asked for (to write it as a dead letter); the
    /// event as the failing step left it otherwise.
    pub event: Value,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct ProcessedRows {
    pub rows: Vec<ProcessedEvent>,
    pub failed: Vec<FailedEvent>,
    /// Events a step dropped on purpose.
    pub dropped: usize,
}

/// Runs the steps of `config` on every event. `sources` names the streams events were read from
/// (indexed by [`SourceEvent::source`]). With `keep_originals`, a failed event is reported as it
/// was read.
///
/// A step that does not compile fails the whole batch: that is the pipeline's configuration.
pub fn process_rows(
    runtime: &VrlRuntime,
    config: &Value,
    fallback_target: &str,
    sources: &[String],
    events: Vec<SourceEvent>,
    keep_originals: bool,
) -> Result<ProcessedRows> {
    let steps = parse_processing_steps(config)?;

    let programs = steps
        .iter()
        .map(|step| {
            if step.kind == ProcessingKind::Vrl {
                runtime.compile(&step.script).map(Some).map_err(|e| {
                    Error::invalid(format!("step `{}` compile: {e}", step.transform_name))
                })
            } else {
                Ok(None)
            }
        })
        .collect::<Result<Vec<_>>>()?;

    let mut processed = ProcessedRows::default();

    'events: for SourceEvent { source, mut event } in events {
        let original = keep_originals.then(|| event.clone());
        let source_name = sources.get(source).map(String::as_str).unwrap_or_default();
        let mut metadata = Map::new();
        metadata.insert(SOURCE_STREAM_METADATA.into(), source_name.into());
        let mut destination = fallback_target.to_owned();

        for (step, program) in steps.iter().zip(&programs) {
            let Some(program) = program else {
                destination = step.routing.destination(&event, &step.target);
                continue;
            };

            match runtime.run_with_metadata(program, &mut event, metadata.clone()) {
                Ok(FunctionOutcome::Kept) => {}
                Ok(FunctionOutcome::Dropped) => {
                    processed.dropped += 1;
                    continue 'events;
                }
                Err(error) => {
                    processed.failed.push(FailedEvent {
                        source,
                        event: original.unwrap_or(event),
                        reason: format!("step `{}` run: {error}", step.transform_name),
                    });

                    continue 'events;
                }
            }
        }

        processed.rows.push(ProcessedEvent {
            source,
            destination,
            event,
        });
    }

    Ok(processed)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn read(events: Vec<Value>) -> Vec<SourceEvent> {
        events
            .into_iter()
            .map(|event| SourceEvent { source: 0, event })
            .collect()
    }

    fn run(config: &Value, events: Vec<Value>) -> ProcessedRows {
        process_rows(
            &VrlRuntime::new(),
            config,
            "fallback",
            &["default".into()],
            read(events),
            false,
        )
        .unwrap()
    }

    #[test]
    fn builtin_routes_at_its_position_between_vrl_steps() {
        let config = json!({"steps":[
            {"script":".appname = \"orders\""},
            {"kind":"builtin","operation":"route","routing":{"kind":"field","field":"appname","prefix":"logs_","fallback":"other"}},
            {"script":".appname = \"changed\""}
        ]});

        let result = run(&config, vec![json!({})]);

        assert_eq!(
            result.rows,
            vec![ProcessedEvent {
                source: 0,
                destination: "logs_orders".into(),
                event: json!({"appname":"changed"}),
            }]
        );
        assert!(result.failed.is_empty());
    }

    #[test]
    fn fixed_route_and_invalid_field_fallback_are_supported() {
        for (routing, target) in [
            (json!({"kind":"fixed"}), "orders"),
            (
                json!({"kind":"field","field":"appname","fallback":"other"}),
                "other",
            ),
        ] {
            let config = json!({"steps":[{"kind":"builtin","operation":"route","target":"orders","routing":routing}]});

            let result = run(&config, vec![json!({"appname":42})]);

            assert_eq!(result.rows[0].destination, target);
        }
    }

    #[test]
    fn unknown_operations_fail_the_batch() {
        let config = json!({"steps":[{"kind":"builtin","operation":"unknown"}]});

        assert!(process_rows(&VrlRuntime::new(), &config, "t", &[], Vec::new(), false).is_err());
    }

    #[test]
    fn an_abort_drops_and_a_failure_is_reported_with_the_event_as_read() {
        let config = json!({"steps":[
            {"transform_name":"tag","script":".tagged = true"},
            {"transform_name":"filter","script":"if .level == \"debug\" { abort }"},
            {"transform_name":"count","script":".n = to_int!(.text)"},
        ]});
        let events = vec![
            json!({"level":"debug","text":"1"}),
            json!({"level":"info","text":"x"}),
            json!({"level":"info","text":"2"}),
        ];

        let result = process_rows(
            &VrlRuntime::new(),
            &config,
            "out",
            &["app".into()],
            read(events),
            true,
        )
        .unwrap();

        assert_eq!(result.dropped, 1);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].event["n"], 2);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].event, json!({"level":"info","text":"x"}));
        assert!(result.failed[0].reason.starts_with("step `count` run:"));
    }

    #[test]
    fn steps_read_the_stream_an_event_came_from() {
        let config = json!({"steps":[{"script":".from = %source_stream"}]});
        let events = vec![
            SourceEvent {
                source: 1,
                event: json!({}),
            },
            SourceEvent {
                source: 0,
                event: json!({}),
            },
        ];

        let result = process_rows(
            &VrlRuntime::new(),
            &config,
            "out",
            &["app".into(), "nginx".into()],
            events,
            false,
        )
        .unwrap();

        assert_eq!(result.rows[0].event, json!({"from":"nginx"}));
        assert_eq!(result.rows[1].event, json!({"from":"app"}));
    }
}
