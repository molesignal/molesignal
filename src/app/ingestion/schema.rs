// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Schema-on-write validation and evolution policy.

use std::collections::{BTreeMap, HashSet};

use crate::{
    domain::{
        ingestion::{EVENT_ID_FIELD, RawEvent},
        stream::{FieldDef, FieldType, Schema, StreamType},
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

/// Return the automatic `(indexed, exact)` policy for newly discovered fields.
fn auto_index_policy(stream_type: StreamType, field_name: &str) -> (bool, bool) {
    match stream_type {
        StreamType::Traces => {
            let exact = matches!(field_name, "trace_id" | "span_id" | "service.name")
                || field_name == TRACE_SUMMARY_MARKER_FIELD;
            (exact, exact)
        }
        // Cursor identities and common RUM dimensions are exact indexed so that Bloom/Tantivy can
        // eliminate unrelated hourly objects before Parquet planning.
        StreamType::Logs => {
            let exact = matches!(
                field_name,
                EVENT_ID_FIELD
                    | "session_id"
                    | "fingerprint"
                    | "country"
                    | "browser"
                    | "application"
                    | "environment"
            );
            // `message` uses a tokenized TEXT index. Exact dimensions use a STRING index and
            // Bloom filter; the two modes intentionally remain mutually exclusive.
            (exact || field_name == "message", exact)
        }
        StreamType::Metrics => {
            let exact = field_name == crate::domain::metrics::METRIC_NAME_FIELD;
            (exact, exact)
        }
        StreamType::Profiles | StreamType::Extend => (false, false),
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
        let (indexed, exact) = auto_index_policy(stream_type, &name);
        next.fields.push(FieldDef {
            name,
            data_type,
            nullable: true,
            indexed,
            encrypted: false,
            exact,
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
            auto_index_policy(StreamType::Logs, "message"),
            (true, false)
        );
        assert_eq!(
            auto_index_policy(StreamType::Logs, EVENT_ID_FIELD),
            (true, true)
        );
    }
}
