// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! JSON values read as the type of the column they go to.

use serde_json::{Number, Value};

use crate::domain::stream::FieldType;

/// Largest serialized size a JSON column keeps. A larger value is replaced by a marker that says
/// how large it was, so a runaway `context` cannot fill a row.
pub const MAX_JSON_BYTES: usize = 64 * 1024;

/// The whole number a value holds, if it holds one. A float with nothing after the point counts:
/// a client that computes `1.2e9` in floating point sends `1200000000.0`.
pub fn integral(value: &Value) -> Option<i64> {
    let Value::Number(number) = value else {
        return None;
    };

    number.as_i64().or_else(|| {
        let float = number.as_f64()?;

        (float.fract() == 0.0 && float.abs() < 9.0e15).then_some(float as i64)
    })
}

/// A value for a column of type `data_type`, or `None` when the value cannot be one. `max_len` is
/// the longest text kept.
pub fn column_value(data_type: FieldType, max_len: usize, value: &Value) -> Option<Value> {
    match (data_type, value) {
        (_, Value::Null) => None,
        (FieldType::Utf8, Value::String(text)) => {
            Some(Value::String(truncate(text.clone(), max_len)))
        }
        (FieldType::Utf8, other @ (Value::Number(_) | Value::Bool(_))) => {
            Some(Value::String(truncate(other.to_string(), max_len)))
        }
        (FieldType::Int64, value) => integral(value).map(Value::from),
        (FieldType::Float64, Value::Number(number)) => float(number),
        (FieldType::Bool, Value::Bool(flag)) => Some(Value::Bool(*flag)),
        (FieldType::Json, value) => Some(cap_json(value.clone())),
        _ => None,
    }
}

fn float(number: &Number) -> Option<Value> {
    number
        .as_f64()
        .filter(|float| float.is_finite())
        .and_then(Number::from_f64)
        .map(Value::Number)
}

/// Text cut to at most `max_len` bytes at a character boundary.
pub fn truncate(mut text: String, max_len: usize) -> String {
    if text.len() <= max_len {
        return text;
    }

    let mut end = max_len;

    while !text.is_char_boundary(end) {
        end -= 1;
    }

    text.truncate(end);
    text
}

fn cap_json(value: Value) -> Value {
    // Scalars and small values are by far the common case; measure only what could be large.
    let bytes = match &value {
        Value::Object(_) | Value::Array(_) => {
            serde_json::to_vec(&value).map_or(0, |bytes| bytes.len())
        }
        _ => 0,
    };

    if bytes > MAX_JSON_BYTES {
        return serde_json::json!({ "_truncated": true, "bytes": bytes });
    }

    value
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn whole_floats_are_integers_and_fractions_are_not() {
        assert_eq!(integral(&json!(1_200_000_000_i64)), Some(1_200_000_000));
        assert_eq!(integral(&json!(1_200_000_000.0)), Some(1_200_000_000));
        assert_eq!(integral(&json!(1.5)), None);
        assert_eq!(integral(&json!("7")), None);
        assert_eq!(integral(&json!(1e30)), None);
    }

    #[test]
    fn text_is_cut_at_a_character_boundary() {
        assert_eq!(truncate("héllo".into(), 2), "h");
        assert_eq!(truncate("héllo".into(), 3), "hé");
        assert_eq!(truncate("short".into(), 100), "short");
    }

    #[test]
    fn a_value_takes_the_type_of_its_column() {
        let text = |value: Value| column_value(FieldType::Utf8, 4, &value);

        assert_eq!(text(json!("abcdef")), Some(json!("abcd")));
        assert_eq!(text(json!(42)), Some(json!("42")));
        assert_eq!(text(json!(true)), Some(json!("true")));
        assert_eq!(text(json!({"a": 1})), None);
        assert_eq!(text(Value::Null), None);

        assert_eq!(
            column_value(FieldType::Int64, 0, &json!(3.0)),
            Some(json!(3))
        );
        assert_eq!(column_value(FieldType::Int64, 0, &json!(3.5)), None);
        assert_eq!(column_value(FieldType::Int64, 0, &json!(true)), None);

        assert_eq!(
            column_value(FieldType::Float64, 0, &json!(0)),
            Some(json!(0.0))
        );
        assert_eq!(
            column_value(FieldType::Float64, 0, &json!(0.12)),
            Some(json!(0.12))
        );
        assert_eq!(column_value(FieldType::Float64, 0, &json!("0.1")), None);

        assert_eq!(
            column_value(FieldType::Bool, 0, &json!(false)),
            Some(json!(false))
        );
        assert_eq!(column_value(FieldType::Bool, 0, &json!(0)), None);
    }

    #[test]
    fn json_columns_keep_small_values_and_mark_huge_ones() {
        let small = json!({"plan": "pro", "flags": [1, 2, 3]});

        assert_eq!(column_value(FieldType::Json, 0, &small), Some(small));

        let huge = json!({ "blob": "x".repeat(MAX_JSON_BYTES) });
        let marked = column_value(FieldType::Json, 0, &huge).unwrap();

        assert_eq!(marked["_truncated"], true);
        assert!(marked["bytes"].as_u64().unwrap() > MAX_JSON_BYTES as u64);
    }
}
