// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The filters of one segment, collected batch by batch while its rows are written.

use std::collections::HashSet;

use arrow::array::{Array, Int64Array, RecordBatch, StringArray};
use domain::stream::{FieldType, StreamDefinition, StreamIndexType};

use super::{
    Bloom, FilterKind, Filters, MAX_VALUES, Probe,
    blocks::BlockGrams,
    grams::{SeenGrams, fold},
};

/// Filters of the `bloom` and `ngram` text fields of a segment, and of its `bloom` and `exact`
/// integer fields (their values written in decimal); an `ngram` field also gets one filter for
/// each block of rows (see [`super::blocks`]). A filter is sized for the number of distinct
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
    /// The trigrams of an `ngram` field already among `probes`: each distinct one is hashed once.
    seen: Option<SeenGrams>,
    /// An `ngram` field's grams block by block; `None` once the column turned out not to be text.
    blocks: Option<BlockGrams>,
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
                let kind = match (field.data_type, field.index_type) {
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
                    seen: (kind == FilterKind::Gram).then(SeenGrams::new),
                    blocks: (kind == FilterKind::Gram).then(BlockGrams::new),
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
        let rows = batch.num_rows();

        for field in &mut self.fields {
            if field.probes.is_none() && field.blocks.is_none() {
                continue;
            }

            // A batch without the column leaves the field empty in every row.
            let Ok(index) = schema.index_of(&field.name) else {
                if let Some(blocks) = field.blocks.as_mut() {
                    blocks.skip(rows);
                }

                continue;
            };

            let column = batch.column(index).as_any();

            let column = match field.integer {
                false => column.downcast_ref::<StringArray>().map(Column::Text),
                true => column.downcast_ref::<Int64Array>().map(Column::Integer),
            };

            match column {
                Some(Column::Text(values)) if field.kind == FilterKind::Gram => {
                    field.add_grams(values, &mut self.buffer);
                }
                Some(column) => field.add_values(&column, rows),
                None => {
                    field.probes = None;
                    field.seen = None;
                    field.blocks = None;
                }
            }
        }
    }

    /// The filters under the id of the segment, or `None` when no field got one.
    pub fn finish(self, segment: &str) -> Option<Filters> {
        let mut filters = Filters::default();

        for field in self.fields {
            if let Some(blocks) = field.blocks.and_then(BlockGrams::finish) {
                filters.insert_blocks(&field.name, segment, blocks);
            }

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

impl FieldEntries {
    /// Adds the whole values of `rows` rows: of a `bloom` text field, or of an integer field in
    /// decimal.
    fn add_values(&mut self, column: &Column, rows: usize) {
        let Some(probes) = self.probes.as_mut() else {
            return;
        };

        for row in 0..rows {
            let probe = match column {
                Column::Text(values) if !values.is_null(row) => {
                    Probe::of(values.value(row).as_bytes())
                }
                Column::Integer(values) if !values.is_null(row) => {
                    Probe::of(values.value(row).to_string().as_bytes())
                }
                _ => continue,
            };

            probes.insert(probe);

            if probes.len() > MAX_VALUES {
                self.probes = None;
                return;
            }
        }
    }

    /// Adds an `ngram` field's values: their trigrams to the segment's filter, each distinct one
    /// hashed once, and their folded text to the current block's grams.
    fn add_grams(&mut self, values: &StringArray, buffer: &mut String) {
        for row in 0..values.len() {
            if !values.is_null(row) {
                let folded = fold(values.value(row), buffer);

                if let Some(probes) = self.probes.as_mut() {
                    for window in folded.windows(3) {
                        let gram = [window[0], window[1], window[2]];

                        if self.seen.as_mut().is_none_or(|seen| seen.insert(gram)) {
                            probes.insert(Probe::of(&gram));
                        }
                    }
                }

                if let Some(blocks) = self.blocks.as_mut() {
                    blocks.add(folded);
                }

                if self
                    .probes
                    .as_ref()
                    .is_some_and(|probes| probes.len() > MAX_VALUES)
                {
                    self.probes = None;
                    self.seen = None;
                }
            }

            if let Some(blocks) = self.blocks.as_mut() {
                blocks.next_row();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::datatypes::{DataType, Field, Schema as ArrowSchema};
    use domain::stream::{FieldDef, Schema, StreamIndexType, StreamType};
    use kernel::{ids::Id, time::TimestampMicros};

    use super::{
        super::{LiteralGrams, grams::value_grams, literal_grams},
        *,
    };

    fn stream() -> StreamDefinition {
        let field = |name: &str, index_type: StreamIndexType| {
            let mut field = FieldDef {
                name: name.into(),
                data_type: FieldType::Utf8,
                nullable: true,
                index_type: StreamIndexType::None,
                encrypted: false,
            };

            field.index_type = index_type;
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

    /// 每个不同的三元组只算一次哈希，得到的过滤器与逐个三元组哈希的结果相同。
    #[test]
    fn an_ngram_filter_holds_the_probe_of_every_trigram_once() {
        let urls = [
            "/api/v1/orders?id=7",
            "/api/v1/orders?id=8",
            "/API/v1/Orders",
            "用户登录失败",
            "ab",
        ];
        let rows = urls.iter().map(|url| ("r-1", *url)).collect::<Vec<_>>();

        let mut filters = SegmentFilters::new(&stream()).unwrap();
        filters.add(&batch(&rows));
        filters.add(&batch(&rows));

        let mut probes = HashSet::new();

        for url in urls {
            value_grams(url, &mut String::new(), &mut |gram| {
                probes.insert(Probe::of(&gram));
            });
        }

        let mut expected = Bloom::for_values(probes.len()).unwrap();

        for probe in probes {
            expected.insert_probe(probe);
        }

        let filters = filters.finish("segment").unwrap();
        let built = &filters.fields[&(FilterKind::Gram, "url".to_string())]["segment"];

        assert_eq!(built, &expected);
    }

    /// 块按段里的行数切：没有值的行、整批缺这一列的行也占行；最后不满一块的也是一块。
    #[test]
    fn blocks_count_every_row_of_the_segment() {
        let mut filters = SegmentFilters::new(&stream()).unwrap();
        filters.add(&batch(&[("r-1", "/api/v1/orders"); 1_000]));

        let without_url = RecordBatch::try_new(
            Arc::new(ArrowSchema::new(vec![Field::new(
                "request_id",
                DataType::Utf8,
                true,
            )])),
            vec![Arc::new(StringArray::from(vec!["r-2"; 100]))],
        )
        .unwrap();

        filters.add(&without_url);

        let mut urls = vec![None; 1_000];
        urls[50] = Some("/needle/path");

        let with_nulls = RecordBatch::try_new(
            Arc::new(ArrowSchema::new(vec![Field::new(
                "url",
                DataType::Utf8,
                true,
            )])),
            vec![Arc::new(StringArray::from(urls))],
        )
        .unwrap();

        filters.add(&with_nulls);

        let filters = filters.finish("segment").unwrap();
        let blocks = filters.gram_blocks("url", "segment").expect("url blocks");

        let grams = |literal: &str| {
            let mut grams = LiteralGrams::default();
            grams.add(literal, false);
            grams
        };

        let needle = grams("needle");

        // 第 1 150 行在第二块里：1 000 + 100 + 50。
        assert_eq!(blocks.len(), 3);
        assert_eq!(
            blocks.may_hold(&needle, &[]),
            Some(vec![false, true, false])
        );
        assert!(blocks.may_hold(&grams("orders"), &[]).unwrap()[0]);
        assert_eq!(filters.gram_blocks("request_id", "segment"), None);
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
            field.index_type = index_type;
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
