// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A segment's column statistics, gathered batch by batch while its rows are written: the zone
//! maps of [`super::metadata`], every value of a `skip` field that has few of them, the address
//! range of an `ip` field, and how many rows leave each indexed field empty.

use std::cmp::Ordering;

use arrow::array::{
    Array, BooleanArray, Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray,
};
use serde_json::{Map, Value};

use super::metadata::{SORT_FIELDS, merge_zone_maps, shorten_bounds, zone_maps};
use crate::{
    domain::{
        storage::ColumnStats,
        stream::{
            FieldPartitions, FieldType, StreamDefinition, StreamIndexType, address_key,
            address_number,
        },
    },
    shared::time::TimestampMicros,
};

/// Most distinct values a segment's value set holds.
const MAX_SET_VALUES: usize = 32;

/// Longest string a value set holds, in bytes: longer ones are rarely compared whole.
const MAX_SET_VALUE_BYTES: usize = 64;

pub(crate) struct ColumnStatsBuilder {
    minimums: Map<String, Value>,
    maximums: Map<String, Value>,
    /// `skip` fields and their values so far; `None` once a field has too many, or too long ones.
    sets: Vec<(String, Option<Vec<Value>>)>,
    /// `ip` fields and the lowest and highest address so far.
    addresses: Vec<(String, Option<(u128, u128)>)>,
    /// Indexed fields and the rows so far that leave them empty.
    nulls: Vec<(String, u64)>,
}

impl ColumnStatsBuilder {
    pub(crate) fn new(stream: &StreamDefinition) -> Self {
        let fields =
            |index_type: StreamIndexType| {
                stream.schema.fields.iter().filter(move |field| {
                    !field.encrypted && field.effective_index_type() == index_type
                })
            };

        Self {
            minimums: Map::new(),
            maximums: Map::new(),
            sets: fields(StreamIndexType::Skip)
                .filter(|field| {
                    matches!(
                        field.data_type,
                        FieldType::Utf8 | FieldType::Int64 | FieldType::Bool | FieldType::Timestamp
                    )
                })
                .map(|field| (field.name.clone(), Some(Vec::new())))
                .collect(),
            addresses: fields(StreamIndexType::Ip)
                .filter(|field| field.data_type == FieldType::Utf8)
                .map(|field| (field.name.clone(), None))
                .collect(),
            nulls: stream
                .schema
                .fields
                .iter()
                .filter(|field| {
                    !field.encrypted && field.effective_index_type() != StreamIndexType::None
                })
                .map(|field| (field.name.clone(), 0))
                .collect(),
        }
    }

    pub(crate) fn add(&mut self, stream: &StreamDefinition, batch: &RecordBatch) {
        merge_zone_maps(
            (&mut self.minimums, &mut self.maximums),
            zone_maps(stream, batch),
        );

        let schema = batch.schema();

        // A batch without the column leaves the field empty in every row.
        for (name, nulls) in &mut self.nulls {
            *nulls += match schema.index_of(name) {
                Ok(index) => batch.column(index).null_count(),
                Err(_) => batch.num_rows(),
            } as u64;
        }

        for (name, set) in &mut self.sets {
            if set.is_none() {
                continue;
            }

            if let Ok(index) = schema.index_of(name) {
                add_values(set, batch.column(index).as_ref());
            }
        }

        for (name, range) in &mut self.addresses {
            let Ok(index) = schema.index_of(name) else {
                continue;
            };

            let Some(values) = batch.column(index).as_any().downcast_ref::<StringArray>() else {
                continue;
            };

            for address in values.iter().flatten().filter_map(address_number) {
                *range = Some(match *range {
                    Some((low, high)) => (low.min(address), high.max(address)),
                    None => (address, address),
                });
            }
        }
    }

    pub(crate) fn finish(mut self) -> ColumnStats {
        shorten_bounds(&mut self.minimums, &mut self.maximums);

        let value_sets = self
            .sets
            .into_iter()
            .filter_map(|(name, set)| {
                let mut values = set.filter(|values| !values.is_empty())?;
                values.sort_by(order);
                Some((name, Value::Array(values)))
            })
            .collect();

        let address_ranges = self
            .addresses
            .into_iter()
            .filter_map(|(name, range)| {
                let (low, high) = range?;
                Some((name, Value::from(vec![address_key(low), address_key(high)])))
            })
            .collect();

        let null_counts = self
            .nulls
            .into_iter()
            .map(|(name, nulls)| (name, Value::from(nulls)))
            .collect();

        ColumnStats {
            min_values: self.minimums,
            max_values: self.maximums,
            value_sets,
            address_ranges,
            null_counts,
            indexed_at_micros: Some(TimestampMicros::now().0),
            partitions: FieldPartitions::default(),
        }
    }
}

/// The columns a segment's statistics are gathered from: its indexed fields, and the fields the
/// trace scanner sorts by.
pub(crate) fn stats_columns(stream: &StreamDefinition) -> Vec<String> {
    stream
        .schema
        .fields
        .iter()
        .filter(|field| {
            field.effective_index_type() != StreamIndexType::None
                || SORT_FIELDS.contains(&field.name.as_str())
        })
        .map(|field| field.name.clone())
        .collect()
}

/// Adds the distinct values of `array` to `set`, giving up on the field once it has too many.
fn add_values(set: &mut Option<Vec<Value>>, array: &dyn Array) {
    let values: Box<dyn Iterator<Item = Option<Value>>> =
        if let Some(values) = array.as_any().downcast_ref::<StringArray>() {
            Box::new(
                values
                    .iter()
                    .flatten()
                    .map(|value| (value.len() <= MAX_SET_VALUE_BYTES).then(|| Value::from(value))),
            )
        } else if let Some(values) = array.as_any().downcast_ref::<Int64Array>() {
            Box::new(
                values
                    .iter()
                    .flatten()
                    .map(|value| Some(Value::from(value))),
            )
        } else if let Some(values) = array.as_any().downcast_ref::<TimestampMicrosecondArray>() {
            Box::new(
                values
                    .iter()
                    .flatten()
                    .map(|value| Some(Value::from(value))),
            )
        } else if let Some(values) = array.as_any().downcast_ref::<BooleanArray>() {
            Box::new(
                values
                    .iter()
                    .flatten()
                    .map(|value| Some(Value::from(value))),
            )
        } else {
            *set = None;
            return;
        };

    for value in values {
        let Some(value) = value else {
            *set = None;
            return;
        };

        let Some(known) = set.as_mut() else {
            return;
        };

        if known.contains(&value) {
            continue;
        }

        if known.len() == MAX_SET_VALUES {
            *set = None;
            return;
        }

        known.push(value);
    }
}

/// The order of two values of one field: numbers by value, strings and booleans as themselves.
fn order(left: &Value, right: &Value) -> Ordering {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => left
            .as_i64()
            .zip(right.as_i64())
            .map_or(Ordering::Equal, |(left, right)| left.cmp(&right)),
        (Value::String(left), Value::String(right)) => left.cmp(right),
        (Value::Bool(left), Value::Bool(right)) => left.cmp(right),
        _ => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::datatypes::{DataType, Field, Schema as ArrowSchema};
    use serde_json::json;

    use super::*;
    use crate::{
        domain::stream::{FieldDef, Schema, StreamType},
        shared::{ids::Id, time::TimestampMicros},
    };

    fn stream() -> StreamDefinition {
        let field = |name: &str, data_type: FieldType, index_type: StreamIndexType| {
            let mut field = FieldDef {
                name: name.into(),
                data_type,
                nullable: true,
                index_type: None,
                indexed: false,
                encrypted: false,
                exact: false,
            };

            field.configure_index(true, index_type);
            field
        };

        StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org"),
            name: "access".into(),
            stream_type: StreamType::LOGS,
            schema: Schema {
                fields: vec![
                    field("level", FieldType::Utf8, StreamIndexType::Skip),
                    field("status", FieldType::Int64, StreamIndexType::Skip),
                    field("client_ip", FieldType::Utf8, StreamIndexType::Ip),
                    field("message", FieldType::Utf8, StreamIndexType::FullText),
                ],
            },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn batch(rows: &[(&str, i64, &str)]) -> RecordBatch {
        let schema = Arc::new(ArrowSchema::new(vec![
            Field::new("level", DataType::Utf8, true),
            Field::new("status", DataType::Int64, true),
            Field::new("client_ip", DataType::Utf8, true),
            Field::new("message", DataType::Utf8, true),
        ]));

        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(
                    rows.iter().map(|row| row.0).collect::<Vec<_>>(),
                )),
                Arc::new(Int64Array::from(
                    rows.iter().map(|row| row.1).collect::<Vec<_>>(),
                )),
                Arc::new(StringArray::from(
                    rows.iter().map(|row| row.2).collect::<Vec<_>>(),
                )),
                Arc::new(StringArray::from(vec!["a long log line"; rows.len()])),
            ],
        )
        .unwrap()
    }

    #[test]
    fn sets_ranges_and_bounds_cover_every_batch() {
        let stream = stream();
        let mut builder = ColumnStatsBuilder::new(&stream);
        builder.add(
            &stream,
            &batch(&[("warn", 500, "10.0.0.9"), ("info", 200, "-")]),
        );
        builder.add(&stream, &batch(&[("info", 404, "10.0.0.1")]));

        let stats = builder.finish();

        assert_eq!(stats.value_sets["level"], json!(["info", "warn"]));
        assert_eq!(stats.value_sets["status"], json!([200, 404, 500]));
        assert_eq!(
            stats.address_ranges["client_ip"],
            json!([
                address_key(address_number("10.0.0.1").unwrap()),
                address_key(address_number("10.0.0.9").unwrap())
            ])
        );
        assert_eq!(stats.min_values["status"], json!(200));
        assert!(
            !stats.min_values.contains_key("message"),
            "full text has no bounds"
        );
        assert!(
            !stats.min_values.contains_key("client_ip"),
            "ip fields keep a range instead"
        );
    }

    #[test]
    fn a_field_with_many_values_keeps_no_set() {
        let stream = stream();
        let mut builder = ColumnStatsBuilder::new(&stream);
        let levels = (0..=MAX_SET_VALUES)
            .map(|value| format!("level-{value}"))
            .collect::<Vec<_>>();
        let rows = levels
            .iter()
            .map(|level| (level.as_str(), 200, "-"))
            .collect::<Vec<_>>();

        builder.add(&stream, &batch(&rows));

        let stats = builder.finish();

        assert!(!stats.value_sets.contains_key("level"));
        assert_eq!(stats.value_sets["status"], json!([200]));
        assert!(!stats.address_ranges.contains_key("client_ip"));
    }

    /// 每个建了索引的字段都数空值；一批里没有这一列，这批的每一行都算空。
    #[test]
    fn indexed_fields_count_their_empty_rows() {
        let stream = stream();
        let mut builder = ColumnStatsBuilder::new(&stream);

        let level = Arc::new(ArrowSchema::new(vec![Field::new(
            "level",
            DataType::Utf8,
            true,
        )]));

        builder.add(
            &stream,
            &RecordBatch::try_new(
                level,
                vec![Arc::new(StringArray::from(vec![Some("info"), None, None]))],
            )
            .unwrap(),
        );
        builder.add(&stream, &batch(&[("warn", 500, "10.0.0.9")]));

        let stats = builder.finish();

        assert_eq!(stats.null_counts["level"], json!(2));
        assert_eq!(stats.null_counts["status"], json!(3));
        assert_eq!(stats.null_counts["client_ip"], json!(3));
        assert_eq!(stats.null_counts["message"], json!(3));
    }
}
