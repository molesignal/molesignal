// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Schema-on-write validation and evolution policy.

use std::collections::{BTreeMap, HashSet};

use crate::{
    domain::{
        intake::{EVENT_ID_FIELD, RawEvent},
        stream::{FieldDef, FieldType, Schema, StreamIndexType, StreamSettings, StreamType},
    },
    shared::trace::summary::TRACE_SUMMARY_MARKER_FIELD,
};

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
        // reading the files.
        StreamType::LOGS => {
            let exact = matches!(
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
            let exact = field_name == crate::domain::metrics::METRIC_NAME_FIELD;

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
            field.configure_index(index_type != StreamIndexType::None, index_type);
        }
    }
}

/// Extend a schema with fields first observed in `events`.
///
/// The inferred fields are nullable because earlier rows in the stream do not contain them.
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
            if known.contains(name.as_str()) || new_fields.contains_key(name) {
                continue;
            }

            new_fields.insert(name.clone(), guess_field_type(value));
        }
    }

    if new_fields.is_empty() {
        return None;
    }

    let mut next = schema.clone();

    for (name, data_type) in new_fields {
        let index_type = auto_index_type(stream_type, &name);

        next.fields.push(FieldDef {
            name,
            data_type,
            nullable: true,
            index_type: Some(index_type),
            indexed: index_type != StreamIndexType::None,
            encrypted: false,
            exact: index_type == StreamIndexType::Exact,
        });
    }

    Some(next)
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
        use crate::domain::stream::IndexPattern;

        let known = Schema {
            fields: vec![FieldDef {
                name: "user_id".into(),
                data_type: FieldType::Utf8,
                nullable: true,
                index_type: Some(StreamIndexType::None),
                indexed: false,
                encrypted: false,
                exact: false,
            }],
        };

        let events = vec![RawEvent {
            timestamp: crate::shared::time::TimestampMicros(1),
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
                .effective_index_type()
        };

        assert_eq!(index_of("user_id"), StreamIndexType::None);
        assert_eq!(index_of("order_id"), StreamIndexType::Exact);
        assert_eq!(index_of("client_ip"), StreamIndexType::Ip);
        assert_eq!(index_of("note"), StreamIndexType::None);
    }
}
