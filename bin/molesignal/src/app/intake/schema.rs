// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Schema-on-write validation and evolution policy.
//!
//! A number column takes every number: an Int64 field that meets a value outside the i64 range or
//! with a fraction becomes Float64 (see [`widen_numeric_fields`]), so no value is dropped or
//! truncated. Files and buffers written while it was Int64 are read as Float64.

use std::collections::{BTreeMap, HashSet};

use domain::{
    intake::{EVENT_ID_FIELD, IntakeError, RawEvent},
    stream::{FieldDef, FieldType, Schema, StreamIndexType, StreamSettings, StreamType},
};
use signals::trace::summary::TRACE_SUMMARY_MARKER_FIELD;

/// Validate a raw event against the fields already present in the stream schema.
pub fn check_event_types(schema: &Schema, event: &RawEvent) -> std::result::Result<(), String> {
    for field in &schema.fields {
        let Some(value) = event.fields.get(&field.name) else {
            if field.nullable {
                continue;
            }

            return Err(format!("missing non-null field `{}`", field.name));
        };

        if !value_fits_type(value, field.data_type) {
            return Err(format!(
                "type mismatch on field `{}`: expected {:?}, got {}",
                field.name,
                field.data_type,
                json_type_name(value)
            ));
        }
    }

    Ok(())
}

/// A batch's events after the type check, and the schema they are written under.
pub(crate) struct Admission {
    /// The events that fit, in batch order.
    pub events: Vec<RawEvent>,
    /// The batch position of each kept event.
    pub positions: Vec<usize>,
    /// The events refused, by batch position.
    pub errors: Vec<IntakeError>,
    /// The evolved schema when the batch widened a number field or brought new fields.
    pub schema: Option<Schema>,
}

/// Checks `events` against `schema` and evolves it: Int64 fields that receive other numbers are
/// widened to Float64 first, then the events that still do not fit are refused, and the fields
/// first seen in the kept events are added. A kept event whose value contradicts the type a new
/// field took from an earlier event of the batch (a string after a number) is refused too.
pub(crate) fn admit(schema: &Schema, events: Vec<RawEvent>, stream_type: StreamType) -> Admission {
    let widened = widen_numeric_fields(schema, &events);
    let checked = widened.as_ref().unwrap_or(schema);

    let mut errors = Vec::new();
    let mut kept = Vec::with_capacity(events.len());
    let mut positions = Vec::with_capacity(events.len());

    for (index, event) in events.into_iter().enumerate() {
        match check_event_types(checked, &event) {
            Ok(()) => {
                kept.push(event);
                positions.push(index);
            }
            Err(reason) => errors.push(IntakeError { index, reason }),
        }
    }

    let extended = infer_schema_extension(checked, &kept, stream_type);

    // The new fields typed by the first value of the batch: later values of another kind are
    // refused here rather than failing the whole batch when it becomes columns.
    if let Some(extended) = &extended {
        let mut fitting = Vec::with_capacity(kept.len());
        let mut fitting_positions = Vec::with_capacity(kept.len());

        for (event, position) in kept.into_iter().zip(positions) {
            match check_event_types(extended, &event) {
                Ok(()) => {
                    fitting.push(event);
                    fitting_positions.push(position);
                }
                Err(reason) => errors.push(IntakeError {
                    index: position,
                    reason,
                }),
            }
        }

        kept = fitting;
        positions = fitting_positions;
        errors.sort_by_key(|error| error.index);
    }

    Admission {
        events: kept,
        positions,
        errors,
        schema: extended.or(widened),
    }
}

/// `schema` with its Int64 fields widened to Float64 where an event gives them a number that is
/// not an i64: a fraction, or an integer above `i64::MAX`. `None` when no field widens.
fn widen_numeric_fields(schema: &Schema, events: &[RawEvent]) -> Option<Schema> {
    let mut widened: Option<Schema> = None;

    for (position, field) in schema.fields.iter().enumerate() {
        if field.data_type != FieldType::Int64 {
            continue;
        }

        let widens = events.iter().any(|event| {
            event
                .fields
                .get(&field.name)
                .is_some_and(|value| widens_int(FieldType::Int64, value))
        });

        if widens {
            widened.get_or_insert_with(|| schema.clone()).fields[position].data_type =
                FieldType::Float64;
        }
    }

    widened
}

/// Whether `value` turns a field of `field_type` into Float64: an Int64 field and a number that is
/// not an i64.
fn widens_int(field_type: FieldType, value: &serde_json::Value) -> bool {
    field_type == FieldType::Int64
        && matches!(value, serde_json::Value::Number(number) if !number.is_i64())
}

/// The OpenTelemetry dimensions a log is filtered by: the host and the container it came from, the
/// event it reports, how severe it is and what part of the system it is about. Node Agent's host
/// events are asked for by each of them, as OTLP logs of other sources are.
pub(crate) const LOG_DIMENSIONS: [&str; 5] = [
    "host.id",
    "container.id",
    "event.name",
    "event.domain",
    "severity_text",
];

/// Return the automatic index policy for newly discovered fields.
fn auto_index_type(stream_type: StreamType, field_name: &str) -> StreamIndexType {
    match stream_type {
        // The identities a trace is walked by, and the service it is asked for: a span is found
        // by its own id, its children by the id of their parent.
        StreamType::TRACES => {
            let exact = matches!(
                field_name,
                "trace_id" | "span_id" | "parent_span_id" | "service.name"
            ) || field_name == TRACE_SUMMARY_MARKER_FIELD;

            if exact {
                StreamIndexType::Exact
            } else {
                StreamIndexType::None
            }
        }
        // Cursor identities and common RUM dimensions are exact indexed so that Bloom/Tantivy can
        // eliminate unrelated hourly objects before Parquet planning. `level` is too: the
        // explorer histogram counts errors by it, and an exact index answers that count without
        // reading the files. So are the OpenTelemetry dimensions of `LOG_DIMENSIONS`.
        StreamType::LOGS => {
            let exact = LOG_DIMENSIONS.contains(&field_name)
                || matches!(
                    field_name,
                    EVENT_ID_FIELD
                        | "level"
                        | "session_id"
                        | "fingerprint"
                        | "country"
                        | "browser"
                        | "application"
                        | "environment"
                );

            // `message` uses a tokenized TEXT index. Exact dimensions use a STRING index and
            // Bloom filter; the two modes intentionally remain mutually exclusive.
            if exact {
                StreamIndexType::Exact
            } else if field_name == "message" {
                StreamIndexType::FullText
            } else {
                StreamIndexType::None
            }
        }
        StreamType::METRICS => {
            let exact = field_name == domain::metrics::METRIC_NAME_FIELD;

            if exact {
                StreamIndexType::Exact
            } else {
                StreamIndexType::None
            }
        }
        StreamType::PROFILES | StreamType::EXTEND => StreamIndexType::None,
        _ => StreamIndexType::None,
    }
}

/// 新出现的字段（`schema` 里排在前 `known` 个之后的）按数据流的规则定索引，见
/// [`StreamSettings::new_field_index`]；没有规则的保留默认的索引。
pub fn apply_index_rules(schema: &mut Schema, known: usize, settings: &StreamSettings) {
    for field in schema.fields.iter_mut().skip(known) {
        if let Some(index_type) = settings.new_field_index(&field.name, field.data_type) {
            field.index_type = index_type;
        }
    }
}

/// Extend a schema with fields first observed in `events`.
///
/// The inferred fields are nullable because earlier rows in the stream do not contain them. A field
/// takes the type of its first value, except that a field first seen as Int64 is Float64 when a
/// later value is a number that is not an i64.
pub fn infer_schema_extension(
    schema: &Schema,
    events: &[RawEvent],
    stream_type: StreamType,
) -> Option<Schema> {
    let known: HashSet<&str> = schema
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();

    let mut new_fields: BTreeMap<String, FieldType> = BTreeMap::new();

    for event in events {
        for (name, value) in &event.fields {
            if known.contains(name.as_str()) {
                continue;
            }

            match new_fields.get_mut(name) {
                Some(field_type) if widens_int(*field_type, value) => {
                    *field_type = FieldType::Float64;
                }
                Some(_) => {}
                None => {
                    new_fields.insert(name.clone(), guess_field_type(value));
                }
            }
        }
    }

    if new_fields.is_empty() {
        return None;
    }

    Some(extend_schema(schema, new_fields, stream_type))
}

/// `schema` with `new_fields` appended in name order: nullable, each with the automatic index of
/// its name. The columnar path, which types new fields as it decodes, extends schemas with it too.
pub(crate) fn extend_schema(
    schema: &Schema,
    new_fields: BTreeMap<String, FieldType>,
    stream_type: StreamType,
) -> Schema {
    let mut next = schema.clone();

    for (name, data_type) in new_fields {
        let index_type = auto_index_type(stream_type, &name);

        next.fields.push(FieldDef {
            name,
            data_type,
            nullable: true,
            index_type,
            encrypted: false,
        });
    }

    next
}

fn value_fits_type(value: &serde_json::Value, data_type: FieldType) -> bool {
    match (data_type, value) {
        (FieldType::Bool, serde_json::Value::Bool(_)) => true,
        (FieldType::Int64, serde_json::Value::Number(number)) => number.is_i64(),
        (FieldType::Float64, serde_json::Value::Number(_)) => true,
        (FieldType::Utf8, serde_json::Value::String(_)) => true,
        (FieldType::Timestamp, serde_json::Value::Number(number)) => {
            number.is_i64() || number.is_u64()
        }
        (FieldType::Json, _) | (_, serde_json::Value::Null) => true,
        _ => false,
    }
}

fn guess_field_type(value: &serde_json::Value) -> FieldType {
    match value {
        serde_json::Value::Bool(_) => FieldType::Bool,
        serde_json::Value::Number(number) if number.is_i64() => FieldType::Int64,
        serde_json::Value::Number(_) => FieldType::Float64,
        serde_json::Value::String(_) => FieldType::Utf8,
        _ => FieldType::Json,
    }
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "Null",
        serde_json::Value::Bool(_) => "Bool",
        serde_json::Value::Number(_) => "Number",
        serde_json::Value::String(_) => "String",
        serde_json::Value::Array(_) => "Array",
        serde_json::Value::Object(_) => "Object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_message_uses_tokenized_index() {
        assert_eq!(
            auto_index_type(StreamType::LOGS, "message"),
            StreamIndexType::FullText
        );
        assert_eq!(
            auto_index_type(StreamType::LOGS, EVENT_ID_FIELD),
            StreamIndexType::Exact
        );
        assert_eq!(
            auto_index_type(StreamType::LOGS, "level"),
            StreamIndexType::Exact
        );
        assert_eq!(
            auto_index_type(StreamType::LOGS, "body_level"),
            StreamIndexType::None
        );
    }

    #[test]
    fn a_log_is_indexed_by_its_opentelemetry_dimensions() {
        for field in LOG_DIMENSIONS {
            assert_eq!(
                auto_index_type(StreamType::LOGS, field),
                StreamIndexType::Exact,
                "{field}"
            );
        }

        assert_eq!(
            auto_index_type(StreamType::LOGS, "body"),
            StreamIndexType::None
        );
        assert_eq!(
            auto_index_type(StreamType::METRICS, "host.id"),
            StreamIndexType::None
        );
    }

    #[test]
    fn a_trace_is_indexed_by_its_identities_and_its_service() {
        for field in ["trace_id", "span_id", "parent_span_id", "service.name"] {
            assert_eq!(
                auto_index_type(StreamType::TRACES, field),
                StreamIndexType::Exact,
                "{field}"
            );
        }

        assert_eq!(
            auto_index_type(StreamType::TRACES, "name"),
            StreamIndexType::None
        );
    }

    /// 新字段按数据流的规则定索引；已有的字段与没有规则的新字段保留原样。
    #[test]
    fn new_fields_take_the_stream_rules() {
        use domain::stream::IndexPattern;

        let known = Schema {
            fields: vec![FieldDef {
                name: "user_id".into(),
                data_type: FieldType::Utf8,
                nullable: true,
                index_type: StreamIndexType::None,
                encrypted: false,
            }],
        };

        let events = vec![RawEvent {
            timestamp: kernel::time::TimestampMicros(1),
            fields: serde_json::json!({
                "user_id": "u", "order_id": "o", "client_ip": "10.0.0.1", "note": "n"
            })
            .as_object()
            .unwrap()
            .clone(),
        }];

        let mut schema = infer_schema_extension(&known, &events, StreamType::LOGS).unwrap();

        let settings = StreamSettings {
            index_patterns: vec![
                IndexPattern {
                    pattern: "*_id".into(),
                    index_type: StreamIndexType::Exact,
                },
                IndexPattern {
                    pattern: "*_ip".into(),
                    index_type: StreamIndexType::Ip,
                },
            ],
            ..StreamSettings::default()
        };

        apply_index_rules(&mut schema, known.fields.len(), &settings);

        let index_of = |name: &str| {
            schema
                .fields
                .iter()
                .find(|field| field.name == name)
                .unwrap()
                .index_type
        };

        assert_eq!(index_of("user_id"), StreamIndexType::None);
        assert_eq!(index_of("order_id"), StreamIndexType::Exact);
        assert_eq!(index_of("client_ip"), StreamIndexType::Ip);
        assert_eq!(index_of("note"), StreamIndexType::None);
    }

    fn number_field(name: &str, data_type: FieldType) -> FieldDef {
        FieldDef {
            name: name.into(),
            data_type,
            nullable: true,
            index_type: StreamIndexType::None,
            encrypted: false,
        }
    }

    fn event(fields: serde_json::Value) -> RawEvent {
        RawEvent {
            timestamp: kernel::time::TimestampMicros(1),
            fields: fields.as_object().unwrap().clone(),
        }
    }

    fn type_of(schema: &Schema, name: &str) -> FieldType {
        schema
            .fields
            .iter()
            .find(|field| field.name == name)
            .unwrap()
            .data_type
    }

    /// Integer first, float after: the Int64 field becomes Float64 and the float is kept.
    #[test]
    fn a_float_after_integers_widens_the_field() {
        let schema = Schema {
            fields: vec![number_field("value", FieldType::Int64)],
        };

        let admission = admit(
            &schema,
            vec![event(serde_json::json!({"value": 1.5}))],
            StreamType::METRICS,
        );

        assert!(admission.errors.is_empty());
        assert_eq!(admission.positions, [0]);

        let evolved = admission.schema.expect("widened schema");
        assert_eq!(type_of(&evolved, "value"), FieldType::Float64);
        assert_eq!(evolved.fields[0].index_type, schema.fields[0].index_type);
    }

    /// Float first, integer after: the integer fits the Float64 field and the schema stays.
    #[test]
    fn an_integer_after_floats_fits_the_float_field() {
        let schema = Schema {
            fields: vec![number_field("value", FieldType::Float64)],
        };

        let admission = admit(
            &schema,
            vec![event(serde_json::json!({"value": 2}))],
            StreamType::METRICS,
        );

        assert!(admission.errors.is_empty());
        assert_eq!(admission.events.len(), 1);
        assert!(admission.schema.is_none());
    }

    /// Integers and floats in one batch: a new field and a known Int64 field both become Float64,
    /// whichever comes first, and every event is kept.
    #[test]
    fn integers_and_floats_in_one_batch_make_a_float_field() {
        let schema = Schema {
            fields: vec![number_field("known", FieldType::Int64)],
        };

        let events = vec![
            event(serde_json::json!({"known": 1, "fresh": 3})),
            event(serde_json::json!({"known": 2.5, "fresh": 0.25})),
            event(serde_json::json!({"known": 4, "fresh": 5})),
        ];

        let admission = admit(&schema, events, StreamType::METRICS);

        assert!(admission.errors.is_empty());
        assert_eq!(admission.positions, [0, 1, 2]);

        let evolved = admission.schema.expect("evolved schema");
        assert_eq!(type_of(&evolved, "known"), FieldType::Float64);
        assert_eq!(type_of(&evolved, "fresh"), FieldType::Float64);

        let empty = Schema { fields: vec![] };
        let created = infer_schema_extension(
            &empty,
            &[
                event(serde_json::json!({"value": 3})),
                event(serde_json::json!({"value": 0.42})),
            ],
            StreamType::METRICS,
        )
        .unwrap();

        assert_eq!(type_of(&created, "value"), FieldType::Float64);
    }

    /// An integer above `i64::MAX` cannot be an Int64 either: the field widens.
    #[test]
    fn an_integer_above_the_i64_range_widens_the_field() {
        let schema = Schema {
            fields: vec![number_field("count", FieldType::Int64)],
        };

        let admission = admit(
            &schema,
            vec![event(serde_json::json!({"count": u64::MAX}))],
            StreamType::METRICS,
        );

        assert!(admission.errors.is_empty());
        assert_eq!(
            type_of(&admission.schema.unwrap(), "count"),
            FieldType::Float64
        );
    }

    /// What cannot be widened is refused and reported by position, not written or dropped
    /// silently: a string in a number field, and a string after a number in a new field.
    #[test]
    fn values_that_cannot_widen_are_refused_by_position() {
        let schema = Schema {
            fields: vec![number_field("value", FieldType::Int64)],
        };

        let events = vec![
            event(serde_json::json!({"value": "high"})),
            event(serde_json::json!({"value": 1, "code": 200})),
            event(serde_json::json!({"value": 2, "code": "OK"})),
            event(serde_json::json!({"value": 3})),
        ];

        let admission = admit(&schema, events, StreamType::LOGS);

        let refused: Vec<usize> = admission.errors.iter().map(|error| error.index).collect();
        assert_eq!(refused, [0, 2]);
        assert!(admission.errors[1].reason.contains("`code`"));
        assert_eq!(admission.positions, [1, 3]);

        let evolved = admission.schema.expect("extended schema");
        assert_eq!(type_of(&evolved, "value"), FieldType::Int64);
        assert_eq!(type_of(&evolved, "code"), FieldType::Int64);
    }
}
