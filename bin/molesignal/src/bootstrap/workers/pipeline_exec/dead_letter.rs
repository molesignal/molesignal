// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Events a run could not process, write or deliver, kept in the pipeline's dead letter stream
//! together with what happened to them. That stream is a logs stream whatever signal the pipeline
//! handles: dead letters are read in Logs.
//!
//! A dead letter is the event plus these fields:
//! - `dead_letter_pipeline`: the pipeline's name;
//! - `dead_letter_stage`: `transform` (a step failed on it), `write` (a stream rejected it) or
//!   `deliver` (a connector did not take it);
//! - `dead_letter_reason`: what went wrong;
//! - `dead_letter_source`: the stream it was read from;
//! - `dead_letter_destination`: the stream or connector that did not take it, for `write` and
//!   `deliver`.
//!
//! A step's failure keeps the event as it was read; a rejection or a delivery keeps it as it was
//! sent.

use serde_json::{Map, Value};

use super::{PipelineOutput, PipelineSpec, streams::to_raw_event, summaries};
use crate::{
    domain::{intake::IntakeBatch, stream::StreamType},
    shared::{ids::Id, time::TimestampMicros},
};

pub(super) struct DeadLetter {
    stage: &'static str,
    source: String,
    destination: Option<String>,
    reason: String,
    event: Value,
}

impl DeadLetter {
    pub(super) fn transform(source: String, event: Value, reason: &str) -> Self {
        Self {
            stage: "transform",
            source,
            destination: None,
            reason: reason.to_owned(),
            event,
        }
    }

    pub(super) fn rejected(source: String, event: Value, stream: &str, reason: &str) -> Self {
        Self {
            stage: "write",
            source,
            destination: Some(stream.to_owned()),
            reason: reason.to_owned(),
            event,
        }
    }

    pub(super) fn undelivered(source: String, event: Value, connector: &str, reason: &str) -> Self {
        Self {
            stage: "deliver",
            source,
            destination: Some(connector.to_owned()),
            reason: reason.to_owned(),
            event,
        }
    }

    fn into_event(self, pipeline: &str) -> Value {
        let mut fields = match self.event {
            Value::Object(fields) => fields,
            other => Map::from_iter([("value".to_owned(), other)]),
        };

        fields.insert("dead_letter_pipeline".into(), pipeline.into());
        fields.insert("dead_letter_stage".into(), self.stage.into());
        fields.insert("dead_letter_reason".into(), self.reason.into());
        fields.insert("dead_letter_source".into(), self.source.into());

        if let Some(destination) = self.destination {
            fields.insert("dead_letter_destination".into(), destination.into());
        }

        Value::Object(fields)
    }
}

/// Writes `letters` to the pipeline's dead letter stream. False when there were some for a stream
/// and they did not reach it.
pub(super) async fn write(
    output: &PipelineOutput,
    pipeline: &PipelineSpec<'_>,
    letters: Vec<DeadLetter>,
    errors: &mut Vec<String>,
) -> bool {
    let Some(stream) = &pipeline.config.dead_letter_stream else {
        return true;
    };

    if letters.is_empty() {
        return true;
    }

    let now = TimestampMicros::now();

    let events = letters
        .into_iter()
        .map(|letter| to_raw_event(&letter.into_event(pipeline.name), now))
        .collect();

    let batch = IntakeBatch {
        batch_id: Id::new(),
        org_id: pipeline.org_id.clone(),
        stream: stream.clone(),
        stream_type: StreamType::LOGS,
        events,
        received_at: now,
    };

    match output.intake.intake_pipeline_output(batch).await {
        Ok(result) => {
            errors.extend(summaries(
                result.errors.iter().map(|error| error.reason.as_str()),
                |count, reason| {
                    format!("dead letter stream `{stream}` rejected {count} event(s): {reason}")
                },
            ));

            true
        }
        Err(e) => {
            errors.push(format!(
                "dead letters could not be written to `{stream}`: {e}"
            ));

            false
        }
    }
}
