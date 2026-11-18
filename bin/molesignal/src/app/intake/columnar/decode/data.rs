// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A decoded column's builder, and the type rules a value meets on its way in.

use std::sync::Arc;

use arrow::{
    array::{
        ArrayRef, BooleanBuilder, Float64Builder, Int64Builder, StringBuilder,
        TimestampMicrosecondBuilder,
    },
    datatypes::{DataType, TimeUnit},
};
use domain::{masking::Masker, stream::FieldType};
use simd_json::{
    ValueType,
    prelude::{TypedValue, ValueAsScalar},
    tape::Value,
};

use super::super::json_text::write_value;

/// One column's values as they are appended.
pub(super) enum Data {
    Bool(BooleanBuilder),
    Int64(Int64Builder),
    Float64(Float64Builder),
    Timestamp(TimestampMicrosecondBuilder),
    /// Utf8 and Json fields, and every encrypted field: a string as it is, any other value as its
    /// compact JSON. An encrypted field's text is what the sink encrypts.
    Text(StringBuilder),
}

impl Data {
    pub(super) fn new(field_type: FieldType, encrypted: bool) -> Self {
        if encrypted {
            return Self::Text(StringBuilder::new());
        }

        match field_type {
            FieldType::Bool => Self::Bool(BooleanBuilder::new()),
            FieldType::Int64 => Self::Int64(Int64Builder::new()),
            FieldType::Float64 => Self::Float64(Float64Builder::new()),
            FieldType::Timestamp => Self::Timestamp(TimestampMicrosecondBuilder::new()),
            FieldType::Utf8 | FieldType::Json => Self::Text(StringBuilder::new()),
        }
    }

    pub(super) fn data_type(&self) -> DataType {
        match self {
            Self::Bool(_) => DataType::Boolean,
            Self::Int64(_) => DataType::Int64,
            Self::Float64(_) => DataType::Float64,
            Self::Timestamp(_) => timestamp_type(),
            Self::Text(_) => DataType::Utf8,
        }
    }

    pub(super) fn append_nulls(&mut self, rows: usize) {
        match self {
            Self::Bool(builder) => builder.append_nulls(rows),
            Self::Int64(builder) => builder.append_nulls(rows),
            Self::Float64(builder) => builder.append_nulls(rows),
            Self::Timestamp(builder) => builder.append_nulls(rows),
            Self::Text(builder) => builder.append_nulls(rows),
        }
    }

    pub(super) fn append_text(&mut self, text: &str) {
        if let Self::Text(builder) = self {
            builder.append_value(text);
        }
    }

    /// Appends a value that fits the column (see [`mismatch`]), its strings masked by `masker`.
    pub(super) fn append(
        &mut self,
        value: &Value<'_, '_>,
        masker: Option<&Masker>,
        json: &mut String,
    ) {
        if value.value_type() == ValueType::Null {
            self.append_nulls(1);
            return;
        }

        match self {
            Self::Bool(builder) => builder.append_option(value.as_bool()),
            Self::Int64(builder) => builder.append_option(value.as_i64()),
            Self::Float64(builder) => builder.append_option(number(value)),
            Self::Timestamp(builder) => builder.append_option(value.as_i64()),
            Self::Text(builder) => match (value.as_str(), masker) {
                (Some(text), Some(masker)) => builder.append_value(masker.mask_str(text)),
                (Some(text), None) => builder.append_value(text),
                (None, _) => {
                    json.clear();
                    write_value(json, value, masker);
                    builder.append_value(json.as_str());
                }
            },
        }
    }

    pub(super) fn finish(self) -> ArrayRef {
        match self {
            Self::Bool(mut builder) => Arc::new(builder.finish()),
            Self::Int64(mut builder) => Arc::new(builder.finish()),
            Self::Float64(mut builder) => Arc::new(builder.finish()),
            Self::Timestamp(mut builder) => Arc::new(builder.finish().with_timezone("UTC")),
            Self::Text(mut builder) => Arc::new(builder.finish()),
        }
    }
}

pub(super) fn timestamp_type() -> DataType {
    DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into()))
}

/// Why `value` does not fit a field `name` of `field_type`, in the row path's words; `None` when
/// it fits. Null fits every type.
pub(super) fn mismatch(name: &str, field_type: FieldType, value: &Value<'_, '_>) -> Option<String> {
    let kind = value.value_type();

    let fits = match (field_type, kind) {
        (_, ValueType::Null) | (FieldType::Json, _) => true,
        (FieldType::Bool, ValueType::Bool) => true,
        (FieldType::Int64, ValueType::I64 | ValueType::U64) => value.as_i64().is_some(),
        (FieldType::Float64, ValueType::I64 | ValueType::U64 | ValueType::F64) => true,
        (FieldType::Utf8, ValueType::String) => true,
        (FieldType::Timestamp, ValueType::I64 | ValueType::U64) => value.as_i64().is_some(),
        _ => false,
    };

    (!fits).then(|| {
        format!(
            "type mismatch on field `{name}`: expected {field_type:?}, got {}",
            type_name(kind)
        )
    })
}

/// The type a new field takes from its first value. simd-json reads a non-negative integer as U64;
/// like `serde_json`, an integer that fits an i64 is an Int64 whatever its sign.
pub(super) fn guess(value: &Value<'_, '_>) -> FieldType {
    match value.value_type() {
        ValueType::Bool => FieldType::Bool,
        ValueType::I64 | ValueType::U64 if value.as_i64().is_some() => FieldType::Int64,
        ValueType::U64 | ValueType::F64 => FieldType::Float64,
        ValueType::String => FieldType::Utf8,
        _ => FieldType::Json,
    }
}

fn number(value: &Value<'_, '_>) -> Option<f64> {
    match value.value_type() {
        ValueType::I64 => value.as_i64().map(|number| number as f64),
        ValueType::U64 => value.as_u64().map(|number| number as f64),
        ValueType::F64 => value.as_f64(),
        _ => None,
    }
}

fn type_name(kind: ValueType) -> &'static str {
    match kind {
        ValueType::Null => "Null",
        ValueType::Bool => "Bool",
        ValueType::String => "String",
        ValueType::Array => "Array",
        ValueType::Object => "Object",
        _ => "Number",
    }
}
