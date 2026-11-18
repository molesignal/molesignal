// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Numeric range fields of an archive. A stream's `range` field keeps its numbers in the archive:
//! `int64` and `timestamp` values as integers, `float64` values as floats. A count with `>=`, `>`,
//! `<` or `<=` on such a field is then answered from the archive. The numbers are kept as a column
//! (a fast field), not as terms: an archive is read from object storage a range of bytes at a time,
//! and a count reads the column in one read and scans it, where terms would take a read for each
//! number in the range, thousands for a field like a duration.

use std::{collections::HashMap, ops::Bound};

use arrow::array::{Array, Float64Array, Int64Array, RecordBatch, TimestampMicrosecondArray};
use domain::stream::{FieldType, StreamDefinition, StreamIndexType};
use query_language::{Number, NumberRange};
use tantivy::{
    TantivyDocument, Term,
    query::{Query, RangeQuery},
    schema::{FAST, Field, FieldType as TantivyFieldType, Schema, SchemaBuilder},
};

/// How a range field's numbers are indexed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Integer,
    Decimal,
}

/// The `range` fields of an archive being built, by stream field name.
#[derive(Default)]
pub(super) struct NumberFields(HashMap<String, (Field, Kind)>);

impl NumberFields {
    /// Adds the unencrypted `range` fields of `stream` to `schema`, but none named like the
    /// archive's own fields in `reserved`.
    pub(super) fn add(
        schema: &mut SchemaBuilder,
        stream: &StreamDefinition,
        reserved: &[&str],
    ) -> Self {
        let mut fields = HashMap::new();

        for field in stream.schema.fields.iter().filter(|field| {
            field.index_type == StreamIndexType::Range
                && !field.encrypted
                && !reserved.contains(&field.name.as_str())
        }) {
            let indexed = match field.data_type {
                FieldType::Int64 | FieldType::Timestamp => {
                    (schema.add_i64_field(&field.name, FAST), Kind::Integer)
                }
                FieldType::Float64 => (schema.add_f64_field(&field.name, FAST), Kind::Decimal),
                _ => continue,
            };

            fields.insert(field.name.clone(), indexed);
        }

        Self(fields)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Adds a row's `numbers` of the range fields to its document.
    pub(super) fn add_to(&self, doc: &mut TantivyDocument, numbers: &HashMap<&str, Number>) {
        for (name, number) in numbers {
            match (self.0.get(*name), number) {
                (Some((field, Kind::Integer)), Number::Integer(value)) => {
                    doc.add_i64(*field, *value);
                }
                (Some((field, Kind::Decimal)), Number::Integer(value)) => {
                    doc.add_f64(*field, *value as f64);
                }
                (Some((field, Kind::Decimal)), Number::Decimal(value)) => {
                    doc.add_f64(*field, *value);
                }
                _ => {}
            }
        }
    }
}

/// The `range` columns of a batch, read a row at a time.
pub struct NumberColumns<'a>(Vec<(&'a str, Column<'a>)>);

enum Column<'a> {
    Integers(&'a Int64Array),
    Times(&'a TimestampMicrosecondArray),
    Decimals(&'a Float64Array),
}

impl<'a> NumberColumns<'a> {
    /// The columns of `batch` that hold unencrypted `range` fields of `stream`.
    pub fn of(stream: &'a StreamDefinition, batch: &'a RecordBatch) -> Self {
        let schema = batch.schema();

        let columns = stream
            .schema
            .fields
            .iter()
            .filter(|field| field.index_type == StreamIndexType::Range && !field.encrypted)
            .filter_map(|field| {
                let values = batch.column(schema.index_of(&field.name).ok()?).as_any();

                let column = if let Some(values) = values.downcast_ref::<Int64Array>() {
                    Column::Integers(values)
                } else if let Some(values) = values.downcast_ref::<TimestampMicrosecondArray>() {
                    Column::Times(values)
                } else {
                    Column::Decimals(values.downcast_ref::<Float64Array>()?)
                };

                Some((field.name.as_str(), column))
            })
            .collect();

        Self(columns)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The numbers of `row` into `numbers`, which is cleared first; an empty value is left out.
    pub fn read(&self, row: usize, numbers: &mut HashMap<&'a str, Number>) {
        numbers.clear();

        for (name, column) in &self.0 {
            let number = match column {
                Column::Integers(values) => {
                    (!values.is_null(row)).then(|| Number::Integer(values.value(row)))
                }
                Column::Times(values) => {
                    (!values.is_null(row)).then(|| Number::Integer(values.value(row)))
                }
                Column::Decimals(values) => (!values.is_null(row))
                    .then(|| Number::Decimal(values.value(row)))
                    .filter(|_| values.value(row).is_finite()),
            };

            if let Some(number) = number {
                numbers.insert(*name, number);
            }
        }
    }
}

/// The documents whose `field` is in `range`. `None` when the archive does not keep `field`'s
/// numbers as a column, or a bound has no integer counterpart in an integer field.
pub(super) fn range_query(
    schema: &Schema,
    field: &str,
    range: &NumberRange,
) -> Option<Box<dyn Query>> {
    let field = schema.get_field(field).ok()?;
    let entry = schema.get_field_entry(field);

    if !entry.is_fast() {
        return None;
    }

    let (lower, upper) = match entry.field_type() {
        TantivyFieldType::I64(_) => (
            lower_integer(range.lower)?.map(|value| Term::from_field_i64(field, value)),
            upper_integer(range.upper)?.map(|value| Term::from_field_i64(field, value)),
        ),
        TantivyFieldType::F64(_) => (
            range
                .lower
                .map(|number| Term::from_field_f64(field, decimal(number))),
            range
                .upper
                .map(|number| Term::from_field_f64(field, decimal(number))),
        ),
        _ => return None,
    };

    if matches!((&lower, &upper), (Bound::Unbounded, Bound::Unbounded)) {
        return None;
    }

    Some(Box::new(RangeQuery::new(lower, upper)))
}

fn decimal(number: Number) -> f64 {
    match number {
        Number::Integer(value) => value as f64,
        Number::Decimal(value) => value,
    }
}

/// The lower end of an integer range: `> 2.5` and `>= 2.5` are `>= 3`.
fn lower_integer(bound: Bound<Number>) -> Option<Bound<i64>> {
    Some(match bound {
        Bound::Unbounded => Bound::Unbounded,
        Bound::Included(Number::Integer(value)) => Bound::Included(value),
        Bound::Excluded(Number::Integer(value)) => Bound::Excluded(value),
        Bound::Included(Number::Decimal(value)) => Bound::Included(whole(value.ceil())?),
        Bound::Excluded(Number::Decimal(value)) => {
            Bound::Included(whole(value.floor())?.checked_add(1)?)
        }
    })
}

/// The upper end of an integer range: `< 2.5` and `<= 2.5` are `<= 2`.
fn upper_integer(bound: Bound<Number>) -> Option<Bound<i64>> {
    Some(match bound {
        Bound::Unbounded => Bound::Unbounded,
        Bound::Included(Number::Integer(value)) => Bound::Included(value),
        Bound::Excluded(Number::Integer(value)) => Bound::Excluded(value),
        Bound::Included(Number::Decimal(value)) => Bound::Included(whole(value.floor())?),
        Bound::Excluded(Number::Decimal(value)) => {
            Bound::Included(whole(value.ceil())?.checked_sub(1)?)
        }
    })
}

/// A whole `f64` as an `i64`, when it fits.
fn whole(value: f64) -> Option<i64> {
    (value >= i64::MIN as f64 && value < i64::MAX as f64).then_some(value as i64)
}

#[cfg(test)]
mod tests {
    use std::{
        fmt,
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
    };

    use domain::stream::{FieldDef, Schema as StreamSchema, StreamIndexType};
    use futures::stream::BoxStream;
    use kernel::{ids::Id, time::TimestampMicros};
    use object_store::{
        CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
        ObjectStoreExt, PutMultipartOptions, PutOptions, PutPayload, PutResult, memory::InMemory,
        path::Path as ObjectPath,
    };

    use super::{
        super::{IndexHandle, TantivyArchiveBuilder, TantivyArchiveOpener},
        *,
    };

    /// Counts the reads of the wrapped store.
    #[derive(Debug, Default)]
    struct Counting {
        inner: InMemory,
        reads: AtomicU64,
    }

    impl Counting {
        fn reads(&self) -> u64 {
            self.reads.load(Ordering::SeqCst)
        }
    }

    impl fmt::Display for Counting {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("Counting")
        }
    }

    #[async_trait::async_trait]
    impl ObjectStore for Counting {
        async fn put_opts(
            &self,
            location: &ObjectPath,
            payload: PutPayload,
            options: PutOptions,
        ) -> object_store::Result<PutResult> {
            self.inner.put_opts(location, payload, options).await
        }

        async fn put_multipart_opts(
            &self,
            location: &ObjectPath,
            options: PutMultipartOptions,
        ) -> object_store::Result<Box<dyn MultipartUpload>> {
            self.inner.put_multipart_opts(location, options).await
        }

        async fn get_opts(
            &self,
            location: &ObjectPath,
            options: GetOptions,
        ) -> object_store::Result<GetResult> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.inner.get_opts(location, options).await
        }

        fn delete_stream(
            &self,
            locations: BoxStream<'static, object_store::Result<ObjectPath>>,
        ) -> BoxStream<'static, object_store::Result<ObjectPath>> {
            self.inner.delete_stream(locations)
        }

        fn list(
            &self,
            prefix: Option<&ObjectPath>,
        ) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
            self.inner.list(prefix)
        }

        async fn list_with_delimiter(
            &self,
            prefix: Option<&ObjectPath>,
        ) -> object_store::Result<ListResult> {
            self.inner.list_with_delimiter(prefix).await
        }

        async fn copy_opts(
            &self,
            from: &ObjectPath,
            to: &ObjectPath,
            options: CopyOptions,
        ) -> object_store::Result<()> {
            self.inner.copy_opts(from, to, options).await
        }
    }

    fn stream() -> StreamDefinition {
        let field = |name: &str, data_type, index_type| {
            let mut field = FieldDef {
                name: name.into(),
                data_type,
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
            name: "app".into(),
            stream_type: domain::stream::StreamType::LOGS,
            schema: StreamSchema {
                fields: vec![
                    field("level", FieldType::Utf8, StreamIndexType::Exact),
                    field("duration", FieldType::Int64, StreamIndexType::Range),
                    field("ratio", FieldType::Float64, StreamIndexType::Range),
                    field("at", FieldType::Timestamp, StreamIndexType::Range),
                    field("size", FieldType::Int64, StreamIndexType::Skip),
                ],
            },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    /// An archive of five rows: `(level, duration, ratio, at)`, `at` empty in the last.
    async fn archive() -> IndexHandle {
        let mut builder = TantivyArchiveBuilder::try_new(&stream()).unwrap().unwrap();

        for (level, duration, ratio, at) in [
            ("error", 50, 0.25, Some(10)),
            ("error", 1500, 0.75, Some(20)),
            ("info", 120, 0.5, Some(30)),
            ("info", 3000, 1.0, Some(40)),
            ("error", 100, 0.0, None),
        ] {
            let values = HashMap::from([("level", level)]);
            let mut numbers = HashMap::from([
                ("duration", Number::Integer(duration)),
                ("ratio", Number::Decimal(ratio)),
                ("size", Number::Integer(7)),
            ]);

            if let Some(at) = at {
                numbers.insert("at", Number::Integer(at));
            }

            builder
                .add_doc_with_numbers(None, &values, &numbers)
                .unwrap();
        }

        let bytes = builder.commit_and_archive().unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let path = ObjectPath::from("numbers.ttv");
        store.put(&path, bytes.clone().into()).await.unwrap();

        TantivyArchiveOpener::open_from_object_store(store, path, bytes.len() as u64)
            .await
            .unwrap()
    }

    fn range(field: &str, lower: Bound<Number>, upper: Bound<Number>) -> (String, NumberRange) {
        (field.to_string(), NumberRange { lower, upper })
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn counts_the_rows_whose_numbers_are_in_range() {
        let handle = archive().await;
        let count = |terms: &[(String, String)], ranges: &[(String, NumberRange)]| {
            handle.count_matching(terms, &[], ranges).unwrap()
        };
        let at_least = |value| Bound::Included(Number::Integer(value));

        assert_eq!(
            count(&[], &[range("duration", at_least(1000), Bound::Unbounded)]),
            Some(2)
        );
        assert_eq!(
            count(
                &[("level".to_string(), "error".to_string())],
                &[range("duration", at_least(100), Bound::Unbounded)]
            ),
            Some(2)
        );
        assert_eq!(
            count(
                &[],
                &[range(
                    "duration",
                    Bound::Excluded(Number::Decimal(99.5)),
                    Bound::Excluded(Number::Integer(1500))
                )]
            ),
            Some(2)
        );
        assert_eq!(
            count(
                &[],
                &[range(
                    "ratio",
                    Bound::Unbounded,
                    Bound::Excluded(Number::Decimal(0.5))
                )]
            ),
            Some(2)
        );
        assert_eq!(
            count(&[], &[range("ratio", at_least(1), Bound::Unbounded)]),
            Some(1)
        );
        assert_eq!(
            count(
                &[],
                &[range(
                    "at",
                    Bound::Excluded(Number::Integer(10)),
                    Bound::Unbounded
                )]
            ),
            Some(3)
        );
    }

    /// A count over a range that holds thousands of distinct numbers reads the field's column at
    /// once, not each number of the range: an archive is read from object storage a range of
    /// bytes at a time.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_wide_range_is_counted_in_a_few_reads() {
        let mut builder = TantivyArchiveBuilder::try_new(&stream()).unwrap().unwrap();
        let values = HashMap::from([("level", "info")]);

        // Each of 0..5000 ten times, in no order.
        for row in 0..50_000_i64 {
            let numbers = HashMap::from([("duration", Number::Integer(row * 7_919 % 5_000))]);

            builder
                .add_doc_with_numbers(None, &values, &numbers)
                .unwrap();
        }

        let bytes = builder.commit_and_archive().unwrap();
        let store = Arc::new(Counting::default());
        let path = ObjectPath::from("wide.ttv");
        store.put(&path, bytes.clone().into()).await.unwrap();

        let handle =
            TantivyArchiveOpener::open_from_object_store(store.clone(), path, bytes.len() as u64)
                .await
                .unwrap();

        let opened = store.reads();

        let count = handle
            .count_matching(
                &[],
                &[],
                &[range(
                    "duration",
                    Bound::Included(Number::Integer(1_000)),
                    Bound::Unbounded,
                )],
            )
            .unwrap();

        let reads = store.reads() - opened;

        // The searcher's own reads and the column's; terms would take one or more for each of
        // the 4,000 numbers in the range.
        assert_eq!(count, Some(40_000));
        assert!(reads <= 16, "{reads} reads");
    }

    /// A field whose numbers the archive does not keep answers nothing: its file is scanned.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_field_whose_numbers_are_not_kept_answers_nothing() {
        let handle = archive().await;

        for field in ["size", "level", "absent"] {
            assert_eq!(
                handle
                    .count_matching(
                        &[],
                        &[],
                        &[range(
                            field,
                            Bound::Included(Number::Integer(1)),
                            Bound::Unbounded
                        )]
                    )
                    .unwrap(),
                None,
                "{field}"
            );
        }

        assert_eq!(handle.count_exact(&[], &[]).unwrap(), Some(5));
    }

    #[test]
    fn decimal_bounds_of_an_integer_field_round_inward() {
        let decimal = |value| Number::Decimal(value);

        assert_eq!(
            lower_integer(Bound::Included(decimal(2.5))),
            Some(Bound::Included(3))
        );
        assert_eq!(
            lower_integer(Bound::Excluded(decimal(2.5))),
            Some(Bound::Included(3))
        );
        assert_eq!(
            lower_integer(Bound::Excluded(decimal(3.0))),
            Some(Bound::Included(4))
        );
        assert_eq!(
            upper_integer(Bound::Included(decimal(2.5))),
            Some(Bound::Included(2))
        );
        assert_eq!(
            upper_integer(Bound::Excluded(decimal(2.5))),
            Some(Bound::Included(2))
        );
        assert_eq!(
            upper_integer(Bound::Excluded(decimal(3.0))),
            Some(Bound::Included(2))
        );
        assert_eq!(lower_integer(Bound::Included(decimal(1e30))), None);
    }
}
