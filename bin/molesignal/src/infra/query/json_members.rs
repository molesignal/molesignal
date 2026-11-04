// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `json_text(document TEXT, key TEXT) -> TEXT` and `json_keys(document TEXT) -> LIST<TEXT>`.
//!
//! A JSON column is stored as text. These two read one level of it: the value of a key as text,
//! and the keys there are, so that a statement can filter, group and measure by what such a column
//! holds. `json_text` is NULL for a key the document does not have, for a JSON `null`, and for text
//! that is not a JSON object, as a masked value is. A nested value comes back as its JSON.

use std::sync::Arc;

use arrow::{
    array::{Array, ArrayRef, ListBuilder, StringArray, StringBuilder},
    compute::cast,
};
use datafusion::{
    arrow::datatypes::{DataType, Field},
    common::ScalarValue,
    error::{DataFusionError, Result},
    logical_expr::{
        ColumnarValue, ScalarFunctionArgs, ScalarUDF, ScalarUDFImpl, Signature, Volatility,
    },
};
use serde_json::{Map, Value};

pub fn json_text_udf() -> ScalarUDF {
    ScalarUDF::from(JsonText {
        signature: Signature::uniform(2, vec![DataType::Utf8], Volatility::Immutable),
    })
}

pub fn json_keys_udf() -> ScalarUDF {
    ScalarUDF::from(JsonKeys {
        signature: Signature::uniform(1, vec![DataType::Utf8], Volatility::Immutable),
    })
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct JsonText {
    signature: Signature,
}

impl ScalarUDFImpl for JsonText {
    fn name(&self) -> &str {
        "json_text"
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Utf8)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        let [document, key] = args.args.as_slice() else {
            return Err(DataFusionError::Execution(
                "json_text takes a document and a key".into(),
            ));
        };

        // The key is nearly always a constant.
        if let ColumnarValue::Scalar(key) = key {
            let Some(key) = scalar_text(key) else {
                return Ok(ColumnarValue::Scalar(ScalarValue::Utf8(None)));
            };

            return Ok(match document {
                ColumnarValue::Scalar(document) => ColumnarValue::Scalar(ScalarValue::Utf8(
                    scalar_text(document).and_then(|document| member(&document, &key)),
                )),
                ColumnarValue::Array(documents) => {
                    let documents = text_array(documents)?;
                    let documents = strings(&documents);

                    ColumnarValue::Array(Arc::new(
                        documents
                            .iter()
                            .map(|document| document.and_then(|text| member(text, &key)))
                            .collect::<StringArray>(),
                    ))
                }
            });
        }

        let rows = args.number_rows;
        let documents = text_array(&document.to_array(rows)?)?;
        let keys = text_array(&key.to_array(rows)?)?;

        let values = strings(&documents)
            .iter()
            .zip(strings(&keys).iter())
            .map(|(document, key)| member((*document)?, (*key)?))
            .collect::<StringArray>();

        Ok(ColumnarValue::Array(Arc::new(values)))
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct JsonKeys {
    signature: Signature,
}

impl ScalarUDFImpl for JsonKeys {
    fn name(&self) -> &str {
        "json_keys"
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::List(Arc::new(Field::new(
            "item",
            DataType::Utf8,
            true,
        ))))
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        let [document] = args.args.as_slice() else {
            return Err(DataFusionError::Execution(
                "json_keys takes a document".into(),
            ));
        };

        let documents = text_array(&document.to_array(args.number_rows)?)?;
        let mut keys = ListBuilder::new(StringBuilder::new());

        for document in strings(&documents) {
            if let Some(object) = document.and_then(object_of) {
                for key in object.keys() {
                    keys.values().append_value(key);
                }
            }

            keys.append(true);
        }

        Ok(ColumnarValue::Array(Arc::new(keys.finish())))
    }
}

/// The members of a document that is a JSON object; `None` for any other text.
pub fn object_of(document: &str) -> Option<Map<String, Value>> {
    match serde_json::from_str(document) {
        Ok(Value::Object(object)) => Some(object),
        _ => None,
    }
}

/// What `json_text` gives for a member's value: text as it is, any other value as its JSON,
/// nothing for a JSON `null`. The key-value index records the same text.
pub fn member_text(value: Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text),
        other => Some(other.to_string()),
    }
}

fn member(document: &str, key: &str) -> Option<String> {
    member_text(object_of(document)?.remove(key)?)
}

fn scalar_text(value: &ScalarValue) -> Option<String> {
    match value {
        ScalarValue::Utf8(text) | ScalarValue::LargeUtf8(text) | ScalarValue::Utf8View(text) => {
            text.clone()
        }
        _ => None,
    }
}

fn text_array(values: &ArrayRef) -> Result<ArrayRef> {
    cast(values, &DataType::Utf8).map_err(|error| DataFusionError::ArrowError(error.into(), None))
}

fn strings(values: &ArrayRef) -> Vec<Option<&str>> {
    values
        .as_any()
        .downcast_ref::<StringArray>()
        .map(|values| values.iter().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use arrow::{
        datatypes::Schema, record_batch::RecordBatch, util::display::array_value_to_string,
    };
    use datafusion::{datasource::MemTable, prelude::SessionContext};

    use super::*;

    async fn answer(documents: Vec<Option<&str>>, sql: &str) -> Vec<String> {
        let schema = Arc::new(Schema::new(vec![Field::new("doc", DataType::Utf8, true)]));

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(StringArray::from(documents)) as ArrayRef],
        )
        .unwrap();

        let ctx = SessionContext::new();

        ctx.register_udf(json_text_udf());
        ctx.register_udf(json_keys_udf());
        ctx.register_table(
            "t",
            Arc::new(MemTable::try_new(schema, vec![vec![batch]]).unwrap()),
        )
        .unwrap();

        ctx.sql(sql)
            .await
            .unwrap()
            .collect()
            .await
            .unwrap()
            .iter()
            .flat_map(|batch| {
                (0..batch.num_rows())
                    .map(|row| array_value_to_string(batch.column(0), row).unwrap())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    #[tokio::test]
    async fn a_key_reads_as_text_whatever_it_holds() {
        let found = answer(
            vec![
                Some(r#"{"flag":"on"}"#),
                Some(r#"{"flag":3}"#),
                Some(r#"{"flag":true}"#),
                Some(r#"{"flag":{"a":1}}"#),
                Some(r#"{"flag":null}"#),
                Some(r#"{"other":"x"}"#),
                Some("not json, as a masked value is"),
                Some(r#"["flag"]"#),
                None,
            ],
            "SELECT COALESCE(json_text(doc, 'flag'), '-') FROM t",
        )
        .await;

        assert_eq!(
            found,
            ["on", "3", "true", r#"{"a":1}"#, "-", "-", "-", "-", "-"]
        );
    }

    #[tokio::test]
    async fn a_statement_can_group_by_what_a_document_holds() {
        let found = answer(
            vec![
                Some(r#"{"flag":"on"}"#),
                Some(r#"{"flag":"on"}"#),
                Some(r#"{"flag":"off"}"#),
                None,
            ],
            "SELECT COALESCE(json_text(doc, 'flag'), '-') || ':' || CAST(COUNT(*) AS VARCHAR) AS a \
             FROM t GROUP BY json_text(doc, 'flag') ORDER BY a",
        )
        .await;

        assert_eq!(found, ["-:1", "off:1", "on:2"]);
    }

    #[tokio::test]
    async fn the_keys_of_documents_are_rows_of_their_own() {
        let found = answer(
            vec![
                Some(r#"{"a":1,"b":2}"#),
                Some(r#"{"b":3}"#),
                Some("nope"),
                None,
            ],
            "SELECT k FROM (SELECT unnest(json_keys(doc)) AS k FROM t) GROUP BY k ORDER BY k",
        )
        .await;

        assert_eq!(found, ["a", "b"]);
    }
}
