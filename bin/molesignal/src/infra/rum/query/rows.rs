// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The rows that come back from a list statement.

use serde_json::{Map, Value};

use super::statements::{ID_COLUMN, TS_COLUMN};
use crate::domain::{
    query::QueryResult,
    rum::event::{RumStream, field},
    stream::FieldType,
};

/// One row of a list: the columns asked for, when it happened, and what tells it from the other
/// rows of that moment.
#[derive(Debug)]
pub struct Row {
    pub timestamp_micros: i64,
    pub tie: String,
    pub fields: Map<String, Value>,
}

impl Row {
    /// The row as a page or a tool gets it: its columns, and when it happened.
    pub fn into_item(self) -> Map<String, Value> {
        let mut item = self.fields;

        item.insert("timestamp".into(), self.timestamp_micros.into());
        item
    }
}

/// The rows of a list statement. A JSON column comes back from the engine as text; the reader gets
/// the document.
pub fn rows(stream: RumStream, result: QueryResult) -> Vec<Row> {
    let QueryResult { columns, rows, .. } = result;

    let Some(time) = columns.iter().position(|column| column == TS_COLUMN) else {
        return Vec::new();
    };

    let tie = columns.iter().position(|column| column == ID_COLUMN);

    let documents = columns
        .iter()
        .map(|name| field(stream, name).is_some_and(|column| column.data_type == FieldType::Json))
        .collect::<Vec<_>>();

    rows.into_iter()
        .filter_map(|row| {
            let timestamp_micros = row.get(time).and_then(number)?;

            let tie = tie
                .and_then(|index| row.get(index))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();

            let mut fields = Map::new();

            for (index, (name, value)) in columns.iter().zip(row).enumerate() {
                if name == TS_COLUMN || name == ID_COLUMN {
                    continue;
                }

                let value = if documents[index] {
                    document(value)
                } else {
                    value
                };

                fields.insert(name.clone(), value);
            }

            Some(Row {
                timestamp_micros,
                tie,
                fields,
            })
        })
        .collect()
}

fn document(value: Value) -> Value {
    match &value {
        Value::String(text) => serde_json::from_str(text).unwrap_or(value),
        _ => value,
    }
}

fn number(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| value.as_f64().map(|value| value as i64))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn result(columns: &[&str], rows: Vec<Vec<Value>>) -> QueryResult {
        QueryResult {
            columns: columns.iter().map(|name| (*name).to_string()).collect(),
            rows,
            scanned_rows: 0,
            took_ms: 0,
            federation: None,
        }
    }

    #[test]
    fn rows_carry_their_time_and_tie_apart_from_their_columns() {
        let found = rows(
            RumStream::Errors,
            result(
                &["error.message", "__ts", "__id"],
                vec![vec![json!("boom"), json!(1500), json!("b:2")]],
            ),
        );

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].timestamp_micros, 1500);
        assert_eq!(found[0].tie, "b:2");
        assert_eq!(found[0].fields.len(), 1);
        assert_eq!(found[0].fields["error.message"], "boom");
    }

    #[test]
    fn a_json_column_comes_back_as_the_document() {
        let found = rows(
            RumStream::Views,
            result(
                &["context", "env", "__ts", "__id"],
                vec![vec![
                    json!(r#"{"plan":"pro"}"#),
                    json!(r#"{"not":"parsed"}"#),
                    json!(1),
                    json!("v-1"),
                ]],
            ),
        );

        assert_eq!(found[0].fields["context"], json!({"plan": "pro"}));
        assert_eq!(found[0].fields["env"], json!(r#"{"not":"parsed"}"#));
    }

    #[test]
    fn a_masked_json_column_stays_as_the_text_it_was_masked_to() {
        let found = rows(
            RumStream::Views,
            result(
                &["context", "__ts", "__id"],
                vec![vec![json!("***"), json!(1), json!("v-1")]],
            ),
        );

        assert_eq!(found[0].fields["context"], "***");
    }

    #[test]
    fn a_result_without_the_time_column_has_no_rows() {
        assert!(
            rows(
                RumStream::Views,
                result(&["env"], vec![vec![json!("prod")]])
            )
            .is_empty()
        );
    }

    #[test]
    fn a_row_is_its_columns_and_when_it_happened() {
        let mut fields = Map::new();

        fields.insert("error.message".into(), json!("boom"));

        let item = Row {
            timestamp_micros: 42,
            tie: "b:2".into(),
            fields,
        }
        .into_item();

        assert_eq!(item["error.message"], "boom");
        assert_eq!(item["timestamp"], 42);
        assert!(!item.contains_key("tie"));
    }
}
