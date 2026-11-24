// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A JSON request body straight into the Arrow columns of a log stream's buffer, by the rules the
//! row path applies event by event (`schema.rs`, write-time masking and the buffer's columns):
//!
//! - a schema field's value must fit its type, and a field that is not nullable must be there; a
//!   row that breaks either is rejected with the same message, the first field in schema order
//!   deciding which;
//! - a field the schema lacks takes the type of its first value in a kept row (`Json` when that
//!   value is null); a later value of another type rejects its row;
//! - an Int64 field, known or new, becomes Float64 at the first number it cannot hold (a fraction,
//!   or an integer above `i64::MAX`), its earlier values converted;
//! - `_timestamp` is taken out of the row (now, when it is not an integer), and `_event_id` is the
//!   batch id and the row's position in the body, whatever the row says;
//! - string values are masked when the organization masks at write time;
//! - an encrypted field's column holds the text the sink encrypts: a string as it is, any other
//!   value as its JSON;
//! - a row's usage bytes are those of its compact JSON without `_timestamp`, before masking, as
//!   `event_bytes` counts them, and every row of the body is counted for its service.
//!
//! For the fields a real-time alert rule reads, each kept row also comes out as a small map, which
//! the rule is matched against as on the row path.
//!
//! The body is parsed once by simd-json into a tape, whose strings point into the body: no value is
//! copied until it is appended to its column.

mod data;

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt::Write as _,
    sync::Arc,
};

use arrow::{
    array::{ArrayRef, RecordBatch, TimestampMicrosecondBuilder},
    datatypes::{Field, Schema as ArrowSchema},
};
use data::{Data, guess, mismatch, timestamp_type, widens};
use domain::{
    intake::{EVENT_ID_FIELD, IntakeError, RawEvent, usage::source::UNNAMED_SERVICE},
    masking::Masker,
    stream::{FieldType, Schema},
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde_json::Map;
use simd_json::{
    ValueType,
    prelude::{TypedValue, ValueAsScalar, ValueIntoString},
    tape::Value,
};

use super::json_text::{int_len, string_len, to_json, value_len};
use crate::app::intake::usage::{SERVICE_FIELDS, ServiceTally};

const TIMESTAMP_FIELD: &str = "_timestamp";

/// What else decoding does besides building columns.
#[derive(Default)]
pub(crate) struct Options<'a> {
    /// The organization's write-time masking; `None` or an empty masker masks nothing.
    pub masker: Option<&'a Masker>,
    /// Fields real-time alert rules read; `Some` makes every kept row also a [`RawEvent`] of them.
    pub watched: Option<&'a HashSet<String>>,
}

/// What a body decodes to.
pub(crate) struct Decoded {
    /// `_timestamp`, then the schema's fields in order, then the new fields by name.
    pub columns: RecordBatch,
    /// Fields the schema lacks, with the type their first kept value gave them: Float64 for one
    /// first seen as Int64 that a later kept value widened.
    pub new_fields: BTreeMap<String, FieldType>,
    /// Int64 fields of the schema that a kept value widened to Float64; their column is Float64.
    pub widened: BTreeSet<String>,
    /// Rejected rows, by their position in the body.
    pub errors: Vec<IntakeError>,
    /// Usage bytes of the kept rows.
    pub collected_bytes: u64,
    /// Every row of the body, by the service it names.
    pub services: ServiceTally,
    /// The kept rows with only the watched fields, in column order; empty without watched fields.
    pub watched: Vec<RawEvent>,
}

/// Decodes `body`, a JSON array of objects or a single object, against `schema`. The body is
/// changed in place by the parser.
pub(crate) fn decode(
    body: &mut [u8],
    schema: &Schema,
    batch_id: &Id,
    now: TimestampMicros,
    options: Options<'_>,
) -> Result<Decoded> {
    let tape = simd_json::to_tape(body)
        .map_err(|error| Error::invalid(format!("intake body: {error}")))?;
    let root = tape.as_value();
    let masker = options.masker.filter(|masker| !masker.is_empty());
    let mut decoder = Decoder::new(schema, batch_id, now, masker, options.watched);

    match root.value_type() {
        ValueType::Array => {
            let array = root.as_array().expect("an array value");

            decoder.timestamps = TimestampMicrosecondBuilder::with_capacity(array.len());

            for (index, item) in array.iter().enumerate() {
                decoder.row(index, &item)?;
            }
        }
        ValueType::Object => decoder.row(0, &root)?,
        _ => return Err(Error::invalid("intake body must be array or object")),
    }

    decoder.finish()
}

/// A column being built.
struct Column {
    name: String,
    /// `None` for a new field no kept row has given a value yet.
    field_type: Option<FieldType>,
    nullable: bool,
    /// The field is in the stream's schema.
    known: bool,
    /// Built once the type is known.
    data: Option<Data>,
}

struct Decoder<'a> {
    columns: Vec<Column>,
    by_name: HashMap<String, usize>,
    /// The column of each key position in the previous row: rows usually list their keys alike,
    /// so a key is checked against it before it is looked up.
    predicted: Vec<usize>,
    event_id: usize,
    timestamps: TimestampMicrosecondBuilder,
    kept: usize,
    errors: Vec<IntakeError>,
    widened: BTreeSet<String>,
    collected_bytes: u64,
    services: ServiceTally,
    batch_id: &'a Id,
    now: TimestampMicros,
    masker: Option<&'a Masker>,
    watched_fields: Option<&'a HashSet<String>>,
    watched: Vec<RawEvent>,
    /// The row's `_event_id`, and the compact JSON of a text column's value.
    text: String,
    json: String,
}

/// One row's values, by column, before it is known whether the row is kept.
struct Staged<'t, 'i> {
    values: Vec<Option<(Value<'t, 'i>, usize)>>,
    touched: Vec<usize>,
}

impl<'a> Decoder<'a> {
    fn new(
        schema: &Schema,
        batch_id: &'a Id,
        now: TimestampMicros,
        masker: Option<&'a Masker>,
        watched_fields: Option<&'a HashSet<String>>,
    ) -> Self {
        let mut columns: Vec<Column> = schema
            .fields
            .iter()
            .map(|field| Column {
                name: field.name.clone(),
                field_type: Some(field.data_type),
                nullable: field.nullable,
                known: true,
                data: Some(Data::new(field.data_type, field.encrypted)),
            })
            .collect();

        let event_id = match columns
            .iter()
            .position(|column| column.name == EVENT_ID_FIELD)
        {
            Some(position) => position,
            None => {
                columns.push(Column {
                    name: EVENT_ID_FIELD.to_owned(),
                    field_type: Some(FieldType::Utf8),
                    nullable: true,
                    known: false,
                    data: Some(Data::new(FieldType::Utf8, false)),
                });

                columns.len() - 1
            }
        };

        let by_name = columns
            .iter()
            .enumerate()
            .map(|(index, column)| (column.name.clone(), index))
            .collect();

        Self {
            columns,
            by_name,
            predicted: Vec::new(),
            event_id,
            timestamps: TimestampMicrosecondBuilder::new(),
            kept: 0,
            errors: Vec::new(),
            widened: BTreeSet::new(),
            collected_bytes: 0,
            services: ServiceTally::default(),
            batch_id,
            now,
            masker,
            watched_fields,
            watched: Vec::new(),
            text: String::new(),
            json: String::new(),
        }
    }

    /// The column of `key`, at `position` among the row's keys; a new column for a new field.
    fn column_of(&mut self, position: usize, key: &str) -> usize {
        if let Some(&column) = self.predicted.get(position)
            && self.columns[column].name == key
        {
            return column;
        }

        let column = match self.by_name.get(key) {
            Some(&column) => column,
            None => {
                self.columns.push(Column {
                    name: key.to_owned(),
                    field_type: None,
                    nullable: true,
                    known: false,
                    data: None,
                });
                self.by_name.insert(key.to_owned(), self.columns.len() - 1);
                self.columns.len() - 1
            }
        };

        if position < self.predicted.len() {
            self.predicted[position] = column;
        } else {
            self.predicted.push(column);
        }

        column
    }

    fn row(&mut self, index: usize, item: &Value<'_, '_>) -> Result<()> {
        let Some(object) = item.as_object() else {
            return Err(Error::invalid(format!(
                "event #{index} must be a JSON object"
            )));
        };

        let mut staged = Staged {
            values: vec![None; self.columns.len() + object.len()],
            touched: Vec::with_capacity(object.len()),
        };

        let mut timestamp = None;
        let mut client_event_id = None;
        let mut service = [None; SERVICE_FIELDS.len()];

        for (position, (key, value)) in object.iter().enumerate() {
            if key == TIMESTAMP_FIELD {
                timestamp = Some(value.as_i64());
                continue;
            }

            // `into_string` keeps the body's lifetime, which `as_str` would tie to `value`.
            if let Some(slot) = SERVICE_FIELDS.iter().position(|field| *field == key) {
                service[slot] = value.into_string();
            }

            let entry = string_len(key) + 1 + value_len(&value);

            if key == EVENT_ID_FIELD {
                client_event_id = Some(entry);
                continue;
            }

            let column = self.column_of(position, key);

            if column >= staged.values.len() {
                staged.values.resize(column + 1, None);
            }

            if staged.values[column].is_none() {
                staged.touched.push(column);
            }

            staged.values[column] = Some((value, entry));
        }

        let named = service.iter().flatten().next().copied();
        self.services.add(named.unwrap_or(UNNAMED_SERVICE), 1);

        if let Some(reason) = self.rejection(&staged) {
            self.errors.push(IntakeError { index, reason });
            return Ok(());
        }

        // `{"timestamp":<ts>,"fields":{...}}`: 24 bytes around the time and the field map.
        let timestamp = timestamp.flatten().unwrap_or(self.now.0);
        let (count, length) = staged
            .touched
            .iter()
            .filter_map(|&column| staged.values[column].as_ref().map(|(_, entry)| *entry))
            .chain(client_event_id)
            .fold((0_usize, 0_usize), |(count, length), entry| {
                (count + 1, length + entry)
            });

        let map = 2 + length + count.saturating_sub(1);
        self.collected_bytes += (24 + int_len(timestamp) + map) as u64;

        self.watch(timestamp, &staged);
        self.keep(index, timestamp, &staged);
        Ok(())
    }

    /// Why the row is rejected, if it is.
    fn rejection(&self, staged: &Staged<'_, '_>) -> Option<String> {
        for (column, info) in self.columns.iter().enumerate() {
            if !info.known || column == self.event_id {
                continue;
            }

            match (
                staged.values.get(column).and_then(Option::as_ref),
                info.field_type,
            ) {
                (Some((value, _)), Some(field_type)) => {
                    if let Some(reason) = mismatch(&info.name, field_type, value) {
                        return Some(reason);
                    }
                }
                (None, _) if !info.nullable => {
                    return Some(format!("missing non-null field `{}`", info.name));
                }
                _ => {}
            }
        }

        staged.touched.iter().find_map(|&column| {
            let info = &self.columns[column];
            let (value, _) = staged.values[column].as_ref()?;

            match (info.known, info.field_type) {
                (false, Some(field_type)) => mismatch(&info.name, field_type, value),
                _ => None,
            }
        })
    }

    /// The row's watched fields as an event, masked as the columns are.
    fn watch(&mut self, timestamp: i64, staged: &Staged<'_, '_>) {
        let Some(fields) = self.watched_fields else {
            return;
        };

        let mut watched = Map::new();

        for &column in &staged.touched {
            let name = &self.columns[column].name;

            if fields.contains(name)
                && let Some((value, _)) = &staged.values[column]
            {
                watched.insert(name.clone(), to_json(value, self.masker));
            }
        }

        self.watched.push(RawEvent {
            timestamp: TimestampMicros(timestamp),
            fields: watched,
        });
    }

    fn keep(&mut self, index: usize, timestamp: i64, staged: &Staged<'_, '_>) {
        for &column in &staged.touched {
            let info = &mut self.columns[column];
            let Some((value, _)) = &staged.values[column] else {
                continue;
            };

            match info.field_type {
                None => {
                    let field_type = guess(value);
                    let mut data = Data::new(field_type, false);

                    data.append_nulls(self.kept);
                    info.field_type = Some(field_type);
                    info.data = Some(data);
                }
                // A number an Int64 column cannot hold makes it Float64, rows so far included.
                Some(field_type) if widens(field_type, value) => {
                    info.field_type = Some(FieldType::Float64);

                    if let Some(data) = info.data.as_mut() {
                        data.widen();
                    }

                    if info.known {
                        self.widened.insert(info.name.clone());
                    }
                }
                Some(_) => {}
            }
        }

        self.timestamps.append_value(timestamp);

        let Self {
            columns,
            event_id,
            batch_id,
            masker,
            text,
            json,
            ..
        } = self;

        text.clear();
        let _ = write!(text, "{}:{index}", batch_id.as_str());

        for (column, info) in columns.iter_mut().enumerate() {
            let Some(data) = info.data.as_mut() else {
                continue;
            };

            if column == *event_id {
                data.append_text(text);
                continue;
            }

            match staged.values.get(column).and_then(Option::as_ref) {
                Some((value, _)) => data.append(value, *masker, json),
                None => data.append_nulls(1),
            }
        }

        self.kept += 1;
    }

    fn finish(mut self) -> Result<Decoded> {
        let mut fields = vec![Field::new(TIMESTAMP_FIELD, timestamp_type(), false)];
        let mut arrays: Vec<ArrayRef> =
            vec![Arc::new(self.timestamps.finish().with_timezone("UTC"))];
        let mut new_fields = BTreeMap::new();

        let mut order: Vec<usize> = (0..self.columns.len())
            .filter(|&column| self.columns[column].known)
            .collect();

        let mut fresh: Vec<usize> = (0..self.columns.len())
            .filter(|&column| {
                let info = &self.columns[column];
                !info.known && info.data.is_some() && self.kept > 0
            })
            .collect();

        fresh.sort_by(|left, right| self.columns[*left].name.cmp(&self.columns[*right].name));
        order.extend(fresh);

        for column in order {
            let info = &mut self.columns[column];

            let (Some(field_type), Some(data)) = (info.field_type, info.data.take()) else {
                continue;
            };

            if !info.known {
                new_fields.insert(info.name.clone(), field_type);
            }

            // Nullable whatever the schema says: a null fits every field, as on the row path.
            fields.push(Field::new(&info.name, data.data_type(), true));
            arrays.push(data.finish());
        }

        let columns = RecordBatch::try_new(Arc::new(ArrowSchema::new(fields)), arrays)
            .map_err(|error| Error::internal(format!("intake columns: {error}")))?;

        Ok(Decoded {
            columns,
            new_fields,
            widened: self.widened,
            errors: self.errors,
            collected_bytes: self.collected_bytes,
            services: self.services,
            watched: self.watched,
        })
    }
}

#[cfg(test)]
mod tests;
