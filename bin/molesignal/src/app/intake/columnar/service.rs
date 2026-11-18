// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! [`IntakeService::intake_json`]: the body of a native write, by the columnar path when its
//! stream takes it and by the row path otherwise.

use domain::{
    intake::{IntakeBatch, IntakeResult, RawEvent},
    rum::event::RumStream,
    stream::{
        StreamDefinition, StreamType, is_reserved_system_stream, refuse_platform_stream,
        validate_stream_name,
    },
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde_json::Map;
use simd_json::{
    ValueType,
    prelude::{TypedValue, ValueAsScalar},
};

use super::{
    ColumnarBatch, JsonIntake,
    decode::{Options, decode},
    json_text::to_json,
};
use crate::app::intake::{
    IntakeService, apply_index_rules, schema::extend_schema, usage::ServiceTally,
};

impl IntakeService {
    /// Writes the events of a native request body: a JSON array of objects, or one object.
    pub async fn intake_json(
        &self,
        org_id: Id,
        stream: String,
        stream_type: StreamType,
        body: Vec<u8>,
    ) -> Result<JsonIntake> {
        if let Some(definition) = self.columnar_stream(&org_id, &stream, stream_type).await? {
            return self.intake_columns(definition, body).await;
        }

        let events = tokio::task::spawn_blocking(move || {
            let mut body = body;
            events_from_json(&mut body)
        })
        .await
        .map_err(|error| Error::internal(format!("intake body join: {error}")))??;

        let services = ServiceTally::of_events(&events);

        let batch = IntakeBatch {
            batch_id: Id::new(),
            org_id,
            stream,
            stream_type,
            events,
            received_at: TimestampMicros::now(),
        };

        // Spans meet their stream's realtime pipeline in `intake_spans`, before they are handed on.
        let result = if stream_type == StreamType::TRACES {
            self.intake_spans(batch).await?
        } else {
            self.intake(batch).await?
        };

        Ok(JsonIntake { result, services })
    }

    /// The stream, when its writes may take the columnar path: a log stream that exists, has no
    /// real-time pipeline and is not one of the RUM streams that merge or track their rows, on a
    /// node that writes it itself. Anything else is left to the row path, which also answers the
    /// requests it refuses.
    async fn columnar_stream(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
    ) -> Result<Option<StreamDefinition>> {
        let refused = is_reserved_system_stream(stream)
            || refuse_platform_stream(stream).is_err()
            || validate_stream_name(stream).is_err()
            || stream == RumStream::Sessions.name()
            || stream == RumStream::Errors.name();

        let draining = self
            .drain
            .as_ref()
            .is_some_and(|drain| !drain.accepts_writes());

        if stream_type != StreamType::LOGS
            || self.columnar.is_none()
            || self.forwarder.is_some()
            || refused
            || draining
        {
            return Ok(None);
        }

        if let Some(engine) = &self.realtime
            && engine
                .repository
                .for_source(org_id, stream_type, stream)
                .await?
                .is_some()
        {
            return Ok(None);
        }

        match self.streams.get(org_id, stream, stream_type).await {
            Ok(definition) => Ok(Some(definition)),
            Err(Error::NotFound(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    #[tracing::instrument(
        name = "intake.batch",
        skip_all,
        fields(
            otel.kind = "internal",
            molesignal.intake.origin = "External",
            molesignal.intake.signal = ?definition.stream_type,
            molesignal.intake.path = "columnar"
        )
    )]
    async fn intake_columns(
        &self,
        definition: StreamDefinition,
        body: Vec<u8>,
    ) -> Result<JsonIntake> {
        let schema = definition.schema.clone();
        let batch_id = Id::new();

        let masker = match &self.masking {
            Some(masking) => Some(masking.intake_masker(&definition.org_id).await?),
            None => None,
        };

        let watched = self.realtime_alerts.as_ref().and_then(|alerts| {
            alerts.watched_fields(&definition.org_id, &definition.name, definition.stream_type)
        });

        let decoded = tokio::task::spawn_blocking(move || {
            let mut body = body;
            let options = Options {
                masker: masker.as_deref(),
                watched: watched.as_ref(),
            };

            decode(
                &mut body,
                &schema,
                &batch_id,
                TimestampMicros::now(),
                options,
            )
        })
        .await
        .map_err(|error| Error::internal(format!("intake body join: {error}")))??;

        // Real-time alert rules see the records about to be written, and count them once they are.
        let hits = match &self.realtime_alerts {
            Some(alerts) if !decoded.watched.is_empty() => alerts.observe(
                &definition.org_id,
                &definition.name,
                definition.stream_type,
                &decoded.watched,
            ),
            _ => None,
        };

        // Schema evolution: new fields get their indexes by the stream's rules, as on the row path.
        if !decoded.new_fields.is_empty() {
            let known = definition.schema.fields.len();
            let mut extended = extend_schema(
                &definition.schema,
                decoded.new_fields.clone(),
                definition.stream_type,
            );

            match self.streams.get_settings(&definition.id).await {
                Ok(settings) => apply_index_rules(&mut extended, known, &settings),
                Err(error) => tracing::warn!(
                    stream = %definition.name,
                    %error,
                    "stream settings unreadable; new fields keep the default indexes"
                ),
            }

            self.streams.update_schema(&definition.id, extended).await?;
        }

        let mut result = match (&self.columnar, decoded.columns.num_rows()) {
            (Some(sink), rows) if rows > 0 => {
                let dataset_type = self.sink.primary_dataset_type(definition.stream_type)?;

                sink.write_columns(
                    dataset_type,
                    ColumnarBatch {
                        org_id: definition.org_id.clone(),
                        stream: definition.name.clone(),
                        stream_type: definition.stream_type,
                        collected_bytes: Some(decoded.collected_bytes),
                        columns: decoded.columns,
                    },
                )
                .await?
            }
            _ => IntakeResult {
                accepted: 0,
                rejected: 0,
                errors: Vec::new(),
            },
        };

        if result.accepted > 0
            && let (Some(hits), Some(alerts)) = (hits, &self.realtime_alerts)
        {
            alerts.record(hits, &result.errors, TimestampMicros::now());
        }

        result.rejected += decoded.errors.len();
        result.errors.extend(decoded.errors);

        Ok(JsonIntake {
            result,
            services: decoded.services,
        })
    }
}

/// The events of a request body for the row path: a JSON array of objects, or one object, parsed
/// with simd-json as the columnar path parses it. Each event's `_timestamp` is taken out of its
/// fields; an event without an integer one is stamped now. Of a key that repeats, the last value
/// is kept, as `serde_json` keeps it.
fn events_from_json(body: &mut [u8]) -> Result<Vec<RawEvent>> {
    let tape = simd_json::to_tape(body)
        .map_err(|error| Error::invalid(format!("intake body: {error}")))?;
    let root = tape.as_value();

    let items: Vec<_> = match root.value_type() {
        ValueType::Array => root
            .as_array()
            .map(|array| array.iter().collect())
            .unwrap_or_default(),
        ValueType::Object => vec![root],
        other => {
            return Err(Error::invalid(format!(
                "intake body must be array or object, got {other:?}"
            )));
        }
    };

    let mut events = Vec::with_capacity(items.len());

    for (index, item) in items.iter().enumerate() {
        let Some(object) = item.as_object() else {
            return Err(Error::invalid(format!(
                "event #{index} must be a JSON object"
            )));
        };

        let mut timestamp = None;
        let mut fields = Map::with_capacity(object.len());

        for (key, value) in object.iter() {
            if key == "_timestamp" {
                timestamp = Some(value.as_i64());
            } else {
                fields.insert(key.to_owned(), to_json(&value, None));
            }
        }

        events.push(RawEvent {
            timestamp: timestamp
                .flatten()
                .map(TimestampMicros)
                .unwrap_or_else(TimestampMicros::now),
            fields,
        });
    }

    Ok(events)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn events(body: &serde_json::Value) -> Result<Vec<RawEvent>> {
        events_from_json(&mut serde_json::to_vec(body).unwrap())
    }

    /// The row path's events are those `serde_json` would give: values as sent, `_timestamp`
    /// taken out of the fields.
    #[test]
    fn row_path_events_match_serde_json() {
        let body = json!([
            {"_timestamp": 7, "message": "中文\n", "n": 1.5, "big": u64::MAX, "attrs": {"a": [1, null]}},
            {"_timestamp": "late", "ok": true}
        ]);

        let events = events(&body).unwrap();

        assert_eq!(events[0].timestamp, TimestampMicros(7));
        assert_eq!(
            serde_json::Value::Object(events[0].fields.clone()),
            json!({"message": "中文\n", "n": 1.5, "big": u64::MAX, "attrs": {"a": [1, null]}})
        );
        assert_eq!(
            serde_json::Value::Object(events[1].fields.clone()),
            json!({"ok": true})
        );
        assert!(events[1].timestamp.0 > 7);
    }

    #[test]
    fn a_body_of_one_object_is_one_event_and_others_are_refused() {
        assert_eq!(events(&json!({"a": 1})).unwrap().len(), 1);
        assert!(matches!(
            events(&json!([{"a": 1}, 2])),
            Err(Error::InvalidArgument(_))
        ));
        assert!(matches!(events(&json!(3)), Err(Error::InvalidArgument(_))));
    }
}
