// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The filters of one segment, collected batch by batch while its rows are written.

use std::collections::HashSet;

use arrow::array::{Array, Int64Array, RecordBatch, StringArray};

use super::{Bloom, FilterKind, Filters, MAX_VALUES, Probe, grams::value_grams};
use crate::domain::stream::{FieldType, StreamDefinition, StreamIndexType};

/// Filters of the `bloom` and `ngram` text fields of a segment, and of its `bloom` and `exact`
/// integer fields (their values written in decimal). A filter is sized for the number of distinct
/// entries, which is only known at the end: the entries' probes are kept until then.
pub struct SegmentFilters {
    fields: Vec<FieldEntries>,
    buffer: String,
}

struct FieldEntries {
    name: String,
    kind: FilterKind,
    /// An integer field, its values written in decimal.
    integer: bool,
    /// Distinct probes so far; `None` once there are more than a filter is built for, or the column
    /// turned out not to be of the field's type.
    probes: Option<HashSet<Probe>>,
}

/// The values of a field's column in one batch, as the bytes a filter holds.
enum Column<'a> {
    Text(&'a StringArray),
    Integer(&'a Int64Array),
}

impl SegmentFilters {
    /// The fields of `stream` that get filters, or `None` when it has none.
    pub fn new(stream: &StreamDefinition) -> Option<Self> {
        let fields = stream
            .schema
            .fields
            .iter()
            .filter(|field| !field.encrypted)
            .filter_map(|field| {
                // Exact text fields get theirs from the Tantivy dictionary when the partition seals.
                let kind = match (field.data_type, field.effective_index_type()) {
                    (FieldType::Utf8, StreamIndexType::Bloom) => FilterKind::Value,
                    (FieldType::Utf8, StreamIndexType::Ngram) => FilterKind::Gram,
                    (FieldType::Int64, StreamIndexType::Bloom | StreamIndexType::Exact) => {
                        FilterKind::Value
                    }
                    _ => return None,
                };

                Some(FieldEntries {
                    name: field.name.clone(),
                    kind,
                    integer: field.data_type == FieldType::Int64,
                    probes: Some(HashSet::new()),
                })
            })
            .collect::<Vec<_>>();

        (!fields.is_empty()).then(|| Self {
            fields,
            buffer: String::new(),
        })
    }

    /// Adds the values of the next batch of the segment.
    pub fn add(&mut self, batch: &RecordBatch) {
        let schema = batch.schema();

        for field in &mut self.fields {
            let Some(probes) = field.probes.as_mut() else {
                continue;
            };

            let Ok(index) = schema.index_of(&field.name) else {
                continue;
            };

            let column = batch.column(index).as_any();

            let column = match field.integer {
                false => column.downcast_ref::<StringArray>().map(Column::Text),
                true => column.downcast_ref::<Int64Array>().map(Column::Integer),
            };

            let Some(column) = column else {
                field.probes = None;
                continue;
            };

            for row in 0..batch.num_rows() {
                match (&column, field.kind) {
                    (Column::Text(values), _) if values.is_null(row) => continue,
                    (Column::Integer(values), _) if values.is_null(row) => continue,
                    (Column::Text(values), FilterKind::Value) => {
                        probes.insert(Probe::of(values.value(row).as_bytes()));
                    }
                    (Column::Text(values), FilterKind::Gram) => {
                        value_grams(values.value(row), &mut self.buffer, &mut |gram| {
                            probes.insert(Probe::of(&gram));
                        })
                    }
                    (Column::Integer(values), _) => {
                        probes.insert(Probe::of(values.value(row).to_string().as_bytes()));
                    }
                }

                if probes.len() > MAX_VALUES {
                    field.probes = None;
                    break;
                }
            }
        }
    }

    /// The filters under the id of the segment, or `None` when no field got one.
    pub fn finish(self, segment: &str) -> Option<Filters> {
        let mut filters = Filters::default();

        for field in self.fields {
            let Some(probes) = field.probes else {
                continue;
            };

            let Some(mut bloom) = Bloom::for_values(probes.len()) else {
                continue;
            };

            for probe in probes {
                bloom.insert_probe(probe);
            }

            filters.insert(field.kind, &field.name, segment, bloom);
        }

        (!filters.is_empty()).then_some(filters)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::datatypes::{DataType, Field, Schema as ArrowSchema};

    use super::{super::literal_grams, *};
    use crate::{
        domain::stream::{FieldDef, Schema, StreamType},
        shared::{ids::Id, time::TimestampMicros},
    };

    fn stream() -> StreamDefinition {
        let field = |name: &str, index_type: StreamIndexType| {
            let mut field = FieldDef {
                name: name.into(),
                data_type: FieldType::Utf8,
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
            name: "logs".into(),
            stream_type: StreamType::LOGS,
            schema: Schema {
                fields: vec![
                    field("request_id", StreamIndexType::Bloom),
                    field("url", StreamIndexType::Ngram),
                    field("message", StreamIndexType::FullText),
                ],
            },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn batch(rows: &[(&str, &str)]) -> RecordBatch {
        let schema = Arc::new(ArrowSchema::new(vec![
            Field::new("request_id", DataType::Utf8, true),
            Field::new("url", DataType::Utf8, true),
        ]));

        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(
                    rows.iter().map(|row| row.0).collect::<Vec<_>>(),
                )),
                Arc::new(StringArray::from(
                    rows.iter().map(|row| row.1).collect::<Vec<_>>(),
                )),
            ],
        )
        .unwrap()
    }

    #[test]
    fn values_and_trigrams_of_several_batches_land_in_one_filter_each() {
        let mut filters = SegmentFilters::new(&stream()).unwrap();
        filters.add(&batch(&[("r-1", "/api/v1/orders")]));
        filters.add(&batch(&[("r-2", "/api/v1/users")]));

        let filters = filters.finish("segment").unwrap();
        let grams = |literal: &str| literal_grams(literal, true).unwrap();

        assert_eq!(
            filters.may_contain("request_id", "segment", "r-2"),
            Some(true)
        );
        assert_eq!(
            filters.may_contain("request_id", "segment", "r-9"),
            Some(false)
        );
        assert_eq!(
            filters.may_contain_grams("url", "segment", &grams("V1/Users")),
            Some(true)
        );
        assert_eq!(
            filters.may_contain_grams("url", "segment", &grams("/api/v2/")),
            Some(false)
        );
        assert_eq!(filters.may_contain("message", "segment", "x"), None);
    }

    /// 整数的精确与 Bloom 字段按十进制记值；整数的跳过索引、文本的精确字段不在这里。
    #[test]
    fn integer_bloom_and_exact_fields_hold_their_values_in_decimal() {
        let mut stream = stream();

        for (name, index_type) in [
            ("user_id", StreamIndexType::Bloom),
            ("order_id", StreamIndexType::Exact),
            ("status", StreamIndexType::Skip),
        ] {
            let mut field = stream.schema.fields[0].clone();
            field.name = name.into();
            field.data_type = FieldType::Int64;
            field.configure_index(true, index_type);
            stream.schema.fields.push(field);
        }

        let schema = Arc::new(ArrowSchema::new(vec![
            Field::new("user_id", DataType::Int64, true),
            Field::new("order_id", DataType::Int64, true),
            Field::new("status", DataType::Int64, true),
        ]));

        let integers =
            |values: &[Option<i64>]| Arc::new(Int64Array::from(values.to_vec())) as Arc<dyn Array>;

        let mut filters = SegmentFilters::new(&stream).unwrap();
        filters.add(
            &RecordBatch::try_new(
                schema,
                vec![
                    integers(&[Some(42), None]),
                    integers(&[Some(-7), Some(9)]),
                    integers(&[Some(200), Some(500)]),
                ],
            )
            .unwrap(),
        );

        let filters = filters.finish("segment").unwrap();

        assert_eq!(filters.may_contain("user_id", "segment", "42"), Some(true));
        assert_eq!(filters.may_contain("user_id", "segment", "43"), Some(false));
        assert_eq!(filters.may_contain("order_id", "segment", "-7"), Some(true));
        assert_eq!(filters.may_contain("status", "segment", "200"), None);
    }

    #[test]
    fn a_stream_without_bloom_or_ngram_fields_gets_no_filters() {
        let mut stream = stream();
        stream.schema.fields.truncate(0);

        assert!(SegmentFilters::new(&stream).is_none());
    }
}
