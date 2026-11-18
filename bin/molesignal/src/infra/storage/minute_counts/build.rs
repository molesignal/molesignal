// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Gathering a segment's minute counts from its rows as they are written. The rows come in the
//! storage order, by time slice and then by the clustering fields, so neighbouring rows mostly
//! share their minute and values: a run of them is tallied once.

use std::collections::HashMap;

use arrow::{
    array::{Array, ArrayRef, AsArray, RecordBatch, StringArray, TimestampMicrosecondArray},
    compute::cast,
    datatypes::DataType,
};
use domain::stream::{Clustering, StreamDefinition};

use super::{MAX_ENTRIES, MINUTE, MinuteCounts};

/// The time column every row has.
const TIMESTAMP_COLUMN: &str = "_timestamp";

/// A minute and a value of each field.
type Key = (i64, Vec<Option<String>>);

pub struct MinuteCountsBuilder {
    fields: Vec<String>,
    counts: HashMap<Key, u64>,
    /// The rows could not all be counted, or took too many combinations: the segment gets no
    /// counts.
    given_up: bool,
}

impl MinuteCountsBuilder {
    /// Counts by the fields of `clustering`. `None` when there are none, or when one of them is
    /// encrypted: its values are ciphertext, different in every row.
    pub fn new(stream: &StreamDefinition, clustering: Option<&Clustering>) -> Option<Self> {
        let clustering = clustering.filter(|clustering| !clustering.is_empty())?;

        let encrypted = stream
            .schema
            .fields
            .iter()
            .any(|field| field.encrypted && clustering.fields.contains(&field.name));

        (!encrypted).then(|| Self {
            fields: clustering.fields.clone(),
            counts: HashMap::new(),
            given_up: false,
        })
    }

    /// Counts the rows of `batch`. A field the batch lacks has no value in its rows.
    pub fn add(&mut self, batch: &RecordBatch) {
        if self.given_up || batch.num_rows() == 0 {
            return;
        }

        let times = batch
            .column_by_name(TIMESTAMP_COLUMN)
            .and_then(|times| times.as_any().downcast_ref::<TimestampMicrosecondArray>());

        let columns = self
            .fields
            .iter()
            .map(|field| match batch.column_by_name(field) {
                Some(column) => cast(column, &DataType::Utf8).ok().map(Some),
                None => Some(None),
            })
            .collect::<Option<Vec<Option<ArrayRef>>>>();

        let (Some(times), Some(columns)) = (times, columns) else {
            self.give_up();
            return;
        };

        let texts = columns
            .iter()
            .map(|column| column.as_ref().map(|column| column.as_string::<i32>()))
            .collect::<Vec<_>>();

        self.count(times, &texts);
    }

    /// The counts, unless they were given up.
    pub fn finish(self) -> Option<MinuteCounts> {
        if self.given_up || self.counts.is_empty() {
            return None;
        }

        let mut values = vec![Vec::new(); self.fields.len()];

        for (_, key_values) in self.counts.keys() {
            for (field, value) in key_values.iter().enumerate() {
                values[field].extend(value.clone());
            }
        }

        for field_values in &mut values {
            field_values.sort_unstable();
            field_values.dedup();
        }

        let id = |field: usize, value: &Option<String>| match value {
            Some(value) => values[field].binary_search(value).unwrap_or_default() as u32,
            None => values[field].len() as u32,
        };

        let mut entries = self
            .counts
            .iter()
            .map(|((minute, key_values), rows)| {
                let ids = key_values
                    .iter()
                    .enumerate()
                    .map(|(field, value)| id(field, value))
                    .collect::<Vec<_>>();

                (*minute, ids, *rows)
            })
            .collect::<Vec<_>>();

        entries.sort_unstable();

        let mut ids = vec![Vec::with_capacity(entries.len()); self.fields.len()];

        for (_, entry_ids, _) in &entries {
            for (field, id) in entry_ids.iter().enumerate() {
                ids[field].push(*id);
            }
        }

        Some(MinuteCounts {
            minutes: entries.iter().map(|(minute, _, _)| *minute).collect(),
            rows: entries.iter().map(|(_, _, rows)| *rows).collect(),
            fields: self.fields,
            values,
            ids,
        })
    }

    fn count(&mut self, times: &TimestampMicrosecondArray, texts: &[Option<&StringArray>]) {
        let mut run_minute = 0;
        let mut run_values: Vec<Option<&str>> = Vec::with_capacity(texts.len());
        let mut run_rows = 0_u64;

        for row in 0..times.len() {
            let minute = times.value(row).div_euclid(MINUTE);

            let same = run_rows > 0
                && minute == run_minute
                && texts
                    .iter()
                    .zip(&run_values)
                    .all(|(text, run_value)| text_value(*text, row) == *run_value);

            if same {
                run_rows += 1;
                continue;
            }

            if run_rows > 0 {
                self.tally(run_minute, &run_values, run_rows);
            }

            run_minute = minute;
            run_values.clear();
            run_values.extend(texts.iter().map(|text| text_value(*text, row)));
            run_rows = 1;
        }

        if run_rows > 0 {
            self.tally(run_minute, &run_values, run_rows);
        }
    }

    fn tally(&mut self, minute: i64, values: &[Option<&str>], rows: u64) {
        if self.given_up {
            return;
        }

        let key = (
            minute,
            values
                .iter()
                .map(|value| value.map(str::to_owned))
                .collect(),
        );
        *self.counts.entry(key).or_default() += rows;

        if self.counts.len() > MAX_ENTRIES {
            self.give_up();
        }
    }

    fn give_up(&mut self) {
        self.given_up = true;
        self.counts = HashMap::new();
    }
}

/// The value of `row` in a field's text, `None` where the field has none.
fn text_value(text: Option<&StringArray>, row: usize) -> Option<&str> {
    text.filter(|text| text.is_valid(row))
        .map(|text| text.value(row))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{
        array::{BooleanArray, Int64Array},
        datatypes::{Field, Schema as ArrowSchema, TimeUnit},
    };
    use domain::stream::{FieldDef, FieldType, Schema, StreamIndexType, StreamType};
    use kernel::{ids::Id, time::TimestampMicros};

    use super::*;

    fn stream(encrypted: bool) -> StreamDefinition {
        let field = |name: &str, data_type| FieldDef {
            name: name.into(),
            data_type,
            nullable: true,
            index_type: StreamIndexType::None,
            encrypted: encrypted && name == "service",
        };

        StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org"),
            name: "logs".into(),
            stream_type: StreamType::LOGS,
            schema: Schema {
                fields: vec![
                    field("service", FieldType::Utf8),
                    field("status", FieldType::Int64),
                ],
            },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn clustering(fields: &[&str]) -> Clustering {
        Clustering::new(fields.iter().map(|field| (*field).into()).collect(), MINUTE)
    }

    /// Rows of `(time, service, status)`.
    fn batch(rows: &[(i64, Option<&str>, Option<i64>)]) -> RecordBatch {
        let schema = ArrowSchema::new(vec![
            Field::new(
                TIMESTAMP_COLUMN,
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("service", DataType::Utf8, true),
            Field::new("status", DataType::Int64, true),
            Field::new("ok", DataType::Boolean, true),
        ]);

        let columns: Vec<ArrayRef> = vec![
            Arc::new(
                TimestampMicrosecondArray::from(
                    rows.iter().map(|(time, _, _)| *time).collect::<Vec<_>>(),
                )
                .with_timezone("UTC"),
            ),
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|(_, service, _)| *service)
                    .collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(
                rows.iter()
                    .map(|(_, _, status)| *status)
                    .collect::<Vec<_>>(),
            )),
            Arc::new(BooleanArray::from(vec![true; rows.len()])),
        ];

        RecordBatch::try_new(Arc::new(schema), columns).unwrap()
    }

    /// `(minute, values, rows)` of every entry.
    fn entries(counts: &MinuteCounts) -> Vec<(i64, Vec<Option<String>>, u64)> {
        counts
            .entries(i64::MIN..=i64::MAX, &[])
            .map(|entry| {
                let values = (0..counts.fields().len())
                    .map(|field| counts.value(field, entry).map(str::to_owned))
                    .collect();

                (counts.minute(entry), values, counts.rows(entry))
            })
            .collect()
    }

    fn values(values: &[Option<&str>]) -> Vec<Option<String>> {
        values
            .iter()
            .map(|value| value.map(str::to_owned))
            .collect()
    }

    #[test]
    fn rows_are_counted_by_minute_and_values_across_batches() {
        let mut builder =
            MinuteCountsBuilder::new(&stream(false), Some(&clustering(&["service", "status"])))
                .unwrap();

        builder.add(&batch(&[
            (MINUTE + 1, Some("api"), Some(200)),
            (MINUTE + 2, Some("api"), Some(200)),
            (MINUTE + 3, Some("web"), None),
            (2 * MINUTE, Some("api"), Some(200)),
        ]));

        builder.add(&batch(&[
            (MINUTE + 9, Some("api"), Some(200)),
            (-1, None, Some(500)),
        ]));

        let counts = builder.finish().unwrap();

        assert_eq!(
            entries(&counts),
            vec![
                (-1, values(&[None, Some("500")]), 1),
                (1, values(&[Some("api"), Some("200")]), 3),
                (1, values(&[Some("web"), None]), 1),
                (2, values(&[Some("api"), Some("200")]), 1),
            ]
        );
    }

    #[test]
    fn a_field_the_rows_lack_has_no_value_and_any_type_is_counted_as_text() {
        let mut builder =
            MinuteCountsBuilder::new(&stream(false), Some(&clustering(&["ok", "host"]))).unwrap();

        builder.add(&batch(&[(0, Some("api"), None), (5, Some("web"), None)]));

        assert_eq!(
            entries(&builder.finish().unwrap()),
            vec![(0, values(&[Some("true"), None]), 2)]
        );
    }

    #[test]
    fn no_counts_without_clustering_or_with_an_encrypted_field() {
        let stream = stream(true);

        assert!(MinuteCountsBuilder::new(&stream, None).is_none());
        assert!(MinuteCountsBuilder::new(&stream, Some(&Clustering::default())).is_none());
        assert!(MinuteCountsBuilder::new(&stream, Some(&clustering(&["service"]))).is_none());
        assert!(MinuteCountsBuilder::new(&stream, Some(&clustering(&["status"]))).is_some());
    }

    #[test]
    fn too_many_combinations_leave_no_counts() {
        let mut builder =
            MinuteCountsBuilder::new(&stream(false), Some(&clustering(&["status"]))).unwrap();

        let rows = (0..=MAX_ENTRIES as i64)
            .map(|row| (0, None, Some(row)))
            .collect::<Vec<_>>();

        builder.add(&batch(&rows));

        assert!(builder.finish().is_none());
    }

    #[test]
    fn a_segment_without_rows_has_no_counts() {
        let builder =
            MinuteCountsBuilder::new(&stream(false), Some(&clustering(&["status"]))).unwrap();

        assert!(builder.finish().is_none());
    }
}
