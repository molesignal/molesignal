// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Rows to columns: the Arrow batch that a set of [`RawEvent`]s becomes in a dataset buffer and in
//! its WAL record.
//!
//! The columns follow the stream schema, `_timestamp` first. A field an event lacks is null; a
//! field the schema lacks is left out. Encrypted fields hold their ciphertext, so neither the WAL
//! nor the buffer ever sees their plain text.

use std::sync::Arc;

use anyhow::{Result, anyhow};
use arrow::{
    array::{
        Array, ArrayRef, AsArray, BooleanBuilder, Float64Builder, Int64Builder, RecordBatch,
        StringBuilder, TimestampMicrosecondBuilder,
    },
    datatypes::{DataType, Field, Schema as ArrowSchema, TimeUnit},
};
use domain::{
    intake::RawEvent,
    stream::{FieldType, StreamDefinition},
};
use postgres::cipher::{OrgFieldKey, encrypt_field};
use serde_json::Value;

use crate::infra::storage::arrow_schema::TS_COL;

/// The Arrow type a field is buffered as. Json is text; an encrypted field is Utf8 ciphertext.
pub fn buffered_type(field_type: FieldType, encrypted: bool) -> DataType {
    if encrypted {
        return DataType::Utf8;
    }

    match field_type {
        FieldType::Bool => DataType::Boolean,
        FieldType::Int64 => DataType::Int64,
        FieldType::Float64 => DataType::Float64,
        FieldType::Utf8 | FieldType::Json => DataType::Utf8,
        FieldType::Timestamp => timestamp_type(),
    }
}

/// The type of `_timestamp` and of timestamp fields: microseconds, UTC.
pub fn timestamp_type() -> DataType {
    DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into()))
}

/// The batch `events` become under `stream`'s schema. `key` encrypts the encrypted fields; a
/// stream with encrypted fields and no key is refused.
pub fn rows_to_columns(
    stream: &StreamDefinition,
    events: &[RawEvent],
    key: Option<&OrgFieldKey>,
) -> Result<RecordBatch> {
    let mut timestamps = TimestampMicrosecondBuilder::with_capacity(events.len());

    for event in events {
        timestamps.append_value(event.timestamp.0);
    }

    let mut fields = vec![Field::new(TS_COL, timestamp_type(), false)];
    let mut columns: Vec<ArrayRef> = vec![Arc::new(timestamps.finish().with_timezone("UTC"))];

    for field in &stream.schema.fields {
        let key = match (field.encrypted, key) {
            (false, _) => None,
            (true, Some(key)) => Some(key),
            (true, None) => {
                return Err(anyhow!(
                    "field `{}` is encrypted but no field key was given",
                    field.name
                ));
            }
        };

        let mut column = ValueColumn::new(field.data_type, key.is_some(), events.len());

        for (row, event) in events.iter().enumerate() {
            let value = event
                .fields
                .get(&field.name)
                .filter(|value| !value.is_null());

            match (value, key) {
                (None, _) => column.append_null(),
                (Some(value), Some(key)) => {
                    let ciphertext = encrypt_field(key, &plaintext(value))
                        .map_err(|error| anyhow!("encrypt field `{}`: {error}", field.name))?;

                    column.append_text(&ciphertext);
                }
                (Some(value), None) => column
                    .append(value)
                    .map_err(|error| anyhow!("row {row}, field `{}`: {error}", field.name))?,
            }
        }

        // Nullable whatever the schema says: a null value passes the type check of every field.
        fields.push(Field::new(
            &field.name,
            buffered_type(field.data_type, field.encrypted),
            true,
        ));
        columns.push(column.finish());
    }

    RecordBatch::try_new(Arc::new(ArrowSchema::new(fields)), columns)
        .map_err(|error| anyhow!("rows to columns: {error}"))
}

/// `batch`, a batch from the columnar intake path, with the values of `stream`'s encrypted fields
/// encrypted. The decoder leaves each such value as the text the row path encrypts (see
/// [`plaintext`]), so both paths store the same ciphertext for the same event.
pub fn encrypt_columns(
    stream: &StreamDefinition,
    batch: RecordBatch,
    key: &OrgFieldKey,
) -> Result<RecordBatch> {
    let schema = batch.schema();
    let mut columns = batch.columns().to_vec();

    for field in stream.schema.fields.iter().filter(|field| field.encrypted) {
        let Ok(index) = schema.index_of(&field.name) else {
            continue;
        };

        let texts = columns[index].as_string_opt::<i32>().ok_or_else(|| {
            anyhow!(
                "encrypted field `{}` holds {:?}, not text",
                field.name,
                columns[index].data_type()
            )
        })?;

        let mut sealed = StringBuilder::with_capacity(texts.len(), texts.len() * 64);

        for text in texts {
            match text {
                Some(text) => sealed.append_value(
                    encrypt_field(key, text)
                        .map_err(|error| anyhow!("encrypt field `{}`: {error}", field.name))?,
                ),
                None => sealed.append_null(),
            }
        }

        columns[index] = Arc::new(sealed.finish());
    }

    RecordBatch::try_new(schema, columns).map_err(|error| anyhow!("encrypted columns: {error}"))
}

/// One column being built from JSON values.
enum ValueColumn {
    Timestamp(TimestampMicrosecondBuilder),
    Bool(BooleanBuilder),
    Int64(Int64Builder),
    Float64(Float64Builder),
    Text(StringBuilder),
}

impl ValueColumn {
    fn new(field_type: FieldType, encrypted: bool, capacity: usize) -> Self {
        if encrypted {
            return Self::Text(StringBuilder::with_capacity(capacity, capacity * 32));
        }

        match field_type {
            FieldType::Bool => Self::Bool(BooleanBuilder::with_capacity(capacity)),
            FieldType::Int64 => Self::Int64(Int64Builder::with_capacity(capacity)),
            FieldType::Float64 => Self::Float64(Float64Builder::with_capacity(capacity)),
            FieldType::Utf8 | FieldType::Json => {
                Self::Text(StringBuilder::with_capacity(capacity, capacity * 32))
            }
            FieldType::Timestamp => {
                Self::Timestamp(TimestampMicrosecondBuilder::with_capacity(capacity))
            }
        }
    }

    fn append_null(&mut self) {
        match self {
            Self::Timestamp(builder) => builder.append_null(),
            Self::Bool(builder) => builder.append_null(),
            Self::Int64(builder) => builder.append_null(),
            Self::Float64(builder) => builder.append_null(),
            Self::Text(builder) => builder.append_null(),
        }
    }

    fn append_text(&mut self, text: &str) {
        if let Self::Text(builder) = self {
            builder.append_value(text);
        }
    }

    /// Appends a non-null value. Text columns take a string as it is and any other value as its
    /// JSON text; the others take only their own kind of value.
    fn append(&mut self, value: &Value) -> Result<()> {
        match self {
            Self::Bool(builder) => {
                builder.append_value(value.as_bool().ok_or_else(|| anyhow!("expect bool"))?);
            }
            Self::Int64(builder) => {
                builder.append_value(value.as_i64().ok_or_else(|| anyhow!("expect i64"))?);
            }
            Self::Float64(builder) => {
                builder.append_value(value.as_f64().ok_or_else(|| anyhow!("expect f64"))?);
            }
            Self::Timestamp(builder) => builder.append_value(
                value
                    .as_i64()
                    .ok_or_else(|| anyhow!("expect timestamp i64 micros"))?,
            ),
            Self::Text(builder) => match value {
                Value::String(text) => builder.append_value(text),
                other => builder.append_value(other.to_string()),
            },
        }

        Ok(())
    }

    fn finish(self) -> ArrayRef {
        match self {
            Self::Timestamp(mut builder) => Arc::new(builder.finish().with_timezone("UTC")),
            Self::Bool(mut builder) => Arc::new(builder.finish()),
            Self::Int64(mut builder) => Arc::new(builder.finish()),
            Self::Float64(mut builder) => Arc::new(builder.finish()),
            Self::Text(mut builder) => Arc::new(builder.finish()),
        }
    }
}

/// The plain text an encrypted field is encrypted from: a string as it is, any other value as its
/// JSON, the same text an unencrypted Utf8 column would hold, so `decrypt(col)` gives it back.
fn plaintext(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::{Array, AsArray},
        datatypes::{Float64Type, Int64Type},
    };
    use domain::stream::{FieldDef, Schema, StreamIndexType, StreamType};
    use kernel::{ids::Id, time::TimestampMicros};
    use serde_json::json;

    use super::*;

    fn field(name: &str, data_type: FieldType) -> FieldDef {
        FieldDef {
            name: name.into(),
            data_type,
            nullable: true,
            index_type: StreamIndexType::None,
            encrypted: false,
        }
    }

    fn stream(fields: Vec<FieldDef>) -> StreamDefinition {
        StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org-a"),
            name: "app".into(),
            stream_type: StreamType::LOGS,
            schema: Schema { fields },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn event(ts: i64, fields: Value) -> RawEvent {
        RawEvent {
            timestamp: TimestampMicros(ts),
            fields: serde_json::from_value(fields).unwrap(),
        }
    }

    #[test]
    fn columns_follow_the_schema_and_missing_fields_are_null() {
        let stream = stream(vec![
            field("message", FieldType::Utf8),
            field("latency", FieldType::Int64),
            field("ratio", FieldType::Float64),
            field("attrs", FieldType::Json),
        ]);

        let events = [
            event(
                1,
                json!({"message": "hi", "latency": 7, "ratio": 2, "attrs": {"a": 1}, "extra": 1}),
            ),
            event(2, json!({"attrs": "plain"})),
        ];

        let batch = rows_to_columns(&stream, &events, None).unwrap();
        let names: Vec<_> = batch
            .schema()
            .fields()
            .iter()
            .map(|field| field.name().clone())
            .collect();

        assert_eq!(names, [TS_COL, "message", "latency", "ratio", "attrs"]);
        assert_eq!(batch.column(1).as_string::<i32>().value(0), "hi");
        assert!(batch.column(1).is_null(1));
        assert_eq!(batch.column(2).as_primitive::<Int64Type>().value(0), 7);
        assert_eq!(batch.column(3).as_primitive::<Float64Type>().value(0), 2.0);

        // Json text: objects as their JSON, strings as they are.
        assert_eq!(batch.column(4).as_string::<i32>().value(0), r#"{"a":1}"#);
        assert_eq!(batch.column(4).as_string::<i32>().value(1), "plain");
    }

    #[test]
    fn a_value_of_the_wrong_kind_is_an_error() {
        let stream = stream(vec![field("latency", FieldType::Int64)]);

        assert!(rows_to_columns(&stream, &[event(1, json!({"latency": "slow"}))], None).is_err());
    }

    #[test]
    fn encrypted_fields_need_a_key() {
        let mut secret = field("card", FieldType::Utf8);
        secret.encrypted = true;

        assert!(
            rows_to_columns(
                &stream(vec![secret]),
                &[event(1, json!({"card": "4111"}))],
                None
            )
            .is_err()
        );
    }

    #[test]
    fn columnar_batches_are_encrypted_like_rows() {
        let mut email = field("email", FieldType::Utf8);
        email.encrypted = true;

        let mut code = field("code", FieldType::Int64);
        code.encrypted = true;

        let stream = stream(vec![email, code]);
        let key = OrgFieldKey {
            key_id: "key-1".into(),
            version: 1,
            raw_key: vec![4u8; 32],
        };

        // What the decoder leaves in encrypted columns: their plain text.
        let plain = RecordBatch::try_new(
            Arc::new(ArrowSchema::new(vec![
                Field::new("email", DataType::Utf8, true),
                Field::new("code", DataType::Utf8, true),
            ])),
            vec![
                Arc::new(arrow::array::StringArray::from(vec![Some("a@b.c"), None])),
                Arc::new(arrow::array::StringArray::from(vec![Some("7"), Some("8")])),
            ],
        )
        .unwrap();

        let sealed = encrypt_columns(&stream, plain, &key).unwrap();
        let keys = [("key-1".to_string(), vec![4u8; 32])].into_iter().collect();
        let open = |column: usize, row: usize| {
            postgres::cipher::decrypt_field(
                &keys,
                sealed.column(column).as_string::<i32>().value(row),
            )
        };

        assert_eq!(open(0, 0).as_deref(), Some("a@b.c"));
        assert!(sealed.column(0).is_null(1));
        assert_eq!(open(1, 1).as_deref(), Some("8"));
    }

    #[test]
    fn an_encrypted_field_holds_ciphertext_its_key_opens() {
        let mut email = field("email", FieldType::Utf8);
        email.encrypted = true;

        let key = OrgFieldKey {
            key_id: "key-1".into(),
            version: 1,
            raw_key: vec![4u8; 32],
        };

        let batch = rows_to_columns(
            &stream(vec![email]),
            &[event(1, json!({"email": "alice@example.com"}))],
            Some(&key),
        )
        .unwrap();

        let stored = batch.column(1).as_string::<i32>().value(0);
        let keys = [("key-1".to_string(), vec![4u8; 32])].into_iter().collect();

        assert!(stored.starts_with("kid:key-1:v1:"), "{stored}");
        assert_eq!(
            postgres::cipher::decrypt_field(&keys, stored).as_deref(),
            Some("alice@example.com")
        );
    }
}
