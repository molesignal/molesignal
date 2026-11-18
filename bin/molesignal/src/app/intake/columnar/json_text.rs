// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Values of a parsed request body as `serde_json` would write them, compact: their text for Json
//! columns, and their length for usage metering, which counts an event as its compact JSON (see
//! [`domain::intake::usage::event_bytes`]). Text written for a column has its strings masked when
//! the organization masks at write time; lengths are always those of the body as sent.
//!
//! Strings are escaped as `serde_json` escapes them: `"` and `\` with a backslash, the control
//! characters below 0x20 as `\b`, `\f`, `\n`, `\r`, `\t` or `\u00xx`, everything else as it is.
//! Duplicate keys inside a nested object are kept as sent; `serde_json` would keep the last.

use std::fmt::Write as _;

use domain::masking::Masker;
use serde_json::{Map, Number, Value as Json};
use simd_json::{
    ValueType,
    prelude::{TypedValue, ValueAsScalar},
    tape::Value,
};

/// Bytes `serde_json` adds to escape each byte of a string, beyond the byte itself.
static ESCAPE_EXTRA: [u8; 256] = {
    let mut table = [0_u8; 256];
    let mut byte = 0;

    while byte < 0x20 {
        table[byte] = match byte {
            0x08 | 0x09 | 0x0a | 0x0c | 0x0d => 1,
            _ => 5,
        };
        byte += 1;
    }

    table[b'"' as usize] = 1;
    table[b'\\' as usize] = 1;
    table
};

/// The length of `text` as a JSON string, quotes included.
pub(super) fn string_len(text: &str) -> usize {
    let extra: usize = text
        .bytes()
        .map(|byte| usize::from(ESCAPE_EXTRA[usize::from(byte)]))
        .sum();

    text.len() + extra + 2
}

/// The length of `value` as compact JSON.
pub(super) fn value_len(value: &Value<'_, '_>) -> usize {
    match value.value_type() {
        ValueType::Null => 4,
        ValueType::Bool => {
            if value.as_bool() == Some(true) {
                4
            } else {
                5
            }
        }
        ValueType::I64 => int_len(value.as_i64().unwrap_or_default()),
        ValueType::U64 => uint_len(value.as_u64().unwrap_or_default()),
        ValueType::F64 => float_len(value.as_f64().unwrap_or_default()),
        ValueType::String => string_len(value.as_str().unwrap_or_default()),
        ValueType::Array => {
            let Some(array) = value.as_array() else {
                return 2;
            };

            let items: usize = array.iter().map(|item| value_len(&item)).sum();
            2 + items + array.len().saturating_sub(1)
        }
        ValueType::Object => {
            let Some(object) = value.as_object() else {
                return 2;
            };

            let entries: usize = object
                .iter()
                .map(|(key, value)| string_len(key) + 1 + value_len(&value))
                .sum();

            2 + entries + object.len().saturating_sub(1)
        }
        _ => 4,
    }
}

/// Appends `value` to `out` as compact JSON, its string values (not its keys) masked by `masker`.
pub(super) fn write_value(out: &mut String, value: &Value<'_, '_>, masker: Option<&Masker>) {
    match value.value_type() {
        ValueType::Bool => out.push_str(if value.as_bool() == Some(true) {
            "true"
        } else {
            "false"
        }),
        ValueType::I64 => {
            let _ = write!(out, "{}", value.as_i64().unwrap_or_default());
        }
        ValueType::U64 => {
            let _ = write!(out, "{}", value.as_u64().unwrap_or_default());
        }
        ValueType::F64 => write_float(out, value.as_f64().unwrap_or_default()),
        ValueType::String => {
            let text = value.as_str().unwrap_or_default();

            match masker {
                Some(masker) => write_string(out, &masker.mask_str(text)),
                None => write_string(out, text),
            }
        }
        ValueType::Array => {
            out.push('[');

            if let Some(array) = value.as_array() {
                for (index, item) in array.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }

                    write_value(out, &item, masker);
                }
            }

            out.push(']');
        }
        ValueType::Object => {
            out.push('{');

            if let Some(object) = value.as_object() {
                for (index, (key, item)) in object.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }

                    write_string(out, key);
                    out.push(':');
                    write_value(out, &item, masker);
                }
            }

            out.push('}');
        }
        _ => out.push_str("null"),
    }
}

/// `value` as a `serde_json` value, its strings masked by `masker`: what a real-time alert rule
/// is matched against. Of a key that repeats, the last value is kept, as `serde_json` keeps it.
pub(super) fn to_json(value: &Value<'_, '_>, masker: Option<&Masker>) -> Json {
    match value.value_type() {
        ValueType::Bool => Json::Bool(value.as_bool() == Some(true)),
        ValueType::I64 => Json::from(value.as_i64().unwrap_or_default()),
        ValueType::U64 => Json::from(value.as_u64().unwrap_or_default()),
        ValueType::F64 => value
            .as_f64()
            .and_then(Number::from_f64)
            .map_or(Json::Null, Json::Number),
        ValueType::String => {
            let text = value.as_str().unwrap_or_default();

            Json::String(match masker {
                Some(masker) => masker.mask_str(text).into_owned(),
                None => text.to_owned(),
            })
        }
        ValueType::Array => Json::Array(
            value
                .as_array()
                .map(|array| array.iter().map(|item| to_json(&item, masker)).collect())
                .unwrap_or_default(),
        ),
        ValueType::Object => {
            let mut map = Map::new();

            if let Some(object) = value.as_object() {
                for (key, item) in object.iter() {
                    map.insert(key.to_owned(), to_json(&item, masker));
                }
            }

            Json::Object(map)
        }
        _ => Json::Null,
    }
}

fn write_string(out: &mut String, text: &str) {
    out.push('"');

    let mut start = 0;

    for (index, byte) in text.bytes().enumerate() {
        if ESCAPE_EXTRA[usize::from(byte)] == 0 {
            continue;
        }

        out.push_str(&text[start..index]);

        match byte {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            0x08 => out.push_str("\\b"),
            0x09 => out.push_str("\\t"),
            0x0a => out.push_str("\\n"),
            0x0c => out.push_str("\\f"),
            0x0d => out.push_str("\\r"),
            other => {
                let _ = write!(out, "\\u{other:04x}");
            }
        }

        start = index + 1;
    }

    out.push_str(&text[start..]);
    out.push('"');
}

/// A float as `serde_json` writes it, by its own formatting, and `null` for a value JSON cannot
/// hold.
fn write_float(out: &mut String, value: f64) {
    match Number::from_f64(value) {
        Some(number) => {
            let _ = write!(out, "{number}");
        }
        None => out.push_str("null"),
    }
}

fn float_len(value: f64) -> usize {
    /// Counts what is written to it.
    struct Counter(usize);

    impl std::fmt::Write for Counter {
        fn write_str(&mut self, text: &str) -> std::fmt::Result {
            self.0 += text.len();
            Ok(())
        }
    }

    let Some(number) = Number::from_f64(value) else {
        return 4;
    };

    let mut counter = Counter(0);
    let _ = write!(counter, "{number}");
    counter.0
}

fn uint_len(value: u64) -> usize {
    value
        .checked_ilog10()
        .map_or(1, |digits| digits as usize + 1)
}

pub(super) fn int_len(value: i64) -> usize {
    uint_len(value.unsigned_abs()) + usize::from(value < 0)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Every case is checked against `serde_json` itself: the text and its length.
    #[test]
    fn values_are_written_as_serde_json_writes_them() {
        let cases = [
            json!(null),
            json!(true),
            json!(false),
            json!(0),
            json!(-17),
            json!(i64::MIN),
            json!(u64::MAX),
            json!(1.5),
            json!(1.0),
            json!(-0.000_001_25),
            json!(1e21),
            json!("plain"),
            json!("中文 \"quoted\" back\\slash \n\t\r\u{8}\u{c}\u{1}\u{1f}"),
            json!([]),
            json!({}),
            json!([1, "two", {"three": [3.25, null, false]}]),
            json!({"a": {"b": "c\n"}, "list": [1, 2]}),
        ];

        for case in cases {
            let expected = serde_json::to_string(&case).unwrap();
            let mut bytes = expected.clone().into_bytes();
            let tape = simd_json::to_tape(&mut bytes).unwrap();
            let value = tape.as_value();

            let mut written = String::new();
            write_value(&mut written, &value, None);

            assert_eq!(written, expected);
            assert_eq!(value_len(&value), expected.len(), "{expected}");
            assert_eq!(to_json(&value, None), case);
        }
    }

    #[test]
    fn masking_reaches_nested_strings_but_not_keys() {
        let masker = Masker::compile([("secret-\\d+".to_owned(), "***".to_owned())]);
        let mut bytes = br#"{"secret-1":["secret-22",{"k":"secret-3"}],"n":1}"#.to_vec();
        let tape = simd_json::to_tape(&mut bytes).unwrap();

        let mut written = String::new();
        write_value(&mut written, &tape.as_value(), Some(&masker));

        assert_eq!(written, r#"{"secret-1":["***",{"k":"***"}],"n":1}"#);
        assert_eq!(
            to_json(&tape.as_value(), Some(&masker)),
            json!({"secret-1": ["***", {"k": "***"}], "n": 1})
        );
    }
}
