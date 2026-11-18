// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Clustering: the order of the rows inside each file of a stream.
//!
//! Rows are kept in time slices [`Clustering::interval`] wide, newest slice first. Inside a slice
//! they are ordered by the values of one or two fields, then by time, newest first. In time order a
//! value of a field like `service` turns up on every page of a file; clustered, each slice holds it
//! on a few pages, and the Parquet page index of the field lets an equality on it skip the rest.
//! Reading the newest rows first still reads only the newest slice.
//!
//! Each file records the clustering it was written with (see
//! [`crate::domain::storage::ColumnStats::clustering`]). Files written before, or without one, keep
//! their rows in time order, and compaction merges only files clustered alike.

use kernel::{Error, Result};
use serde::{Deserialize, Serialize};

/// At most this many clustering fields.
const MAX_CLUSTERING_FIELDS: usize = 2;

/// The width of a time slice when none is given: one minute, in microseconds.
pub const DEFAULT_CLUSTERING_INTERVAL: i64 = 60_000_000;

/// The narrowest time slice: one second.
const MIN_CLUSTERING_INTERVAL: i64 = 1_000_000;

/// An hour in microseconds: a slice divides it, so slices never cross an hour partition.
const HOUR: i64 = 3_600_000_000;

/// The fields a stream clusters the rows of each file by, and the width of its time slices. No
/// fields means no clustering: rows in time order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clustering {
    #[serde(default)]
    pub fields: Vec<String>,
    /// Width of a time slice, in microseconds.
    #[serde(default = "default_interval")]
    pub interval: i64,
}

fn default_interval() -> i64 {
    DEFAULT_CLUSTERING_INTERVAL
}

impl Default for Clustering {
    fn default() -> Self {
        Self {
            fields: Vec::new(),
            interval: DEFAULT_CLUSTERING_INTERVAL,
        }
    }
}

impl Clustering {
    pub fn new(fields: Vec<String>, interval: i64) -> Self {
        Self { fields, interval }
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn validate(&self) -> Result<()> {
        if self.fields.len() > MAX_CLUSTERING_FIELDS {
            return Err(Error::invalid(format!(
                "a stream clusters by at most {MAX_CLUSTERING_FIELDS} fields"
            )));
        }

        for (index, field) in self.fields.iter().enumerate() {
            if field.trim().is_empty() {
                return Err(Error::invalid("clustering field cannot be empty"));
            }

            if field == "_timestamp" {
                return Err(Error::invalid(
                    "clustering field cannot be `_timestamp`: rows are ordered by time already",
                ));
            }

            if self.fields[..index].contains(field) {
                return Err(Error::invalid(format!(
                    "clustering field `{field}` is listed twice"
                )));
            }
        }

        if !(MIN_CLUSTERING_INTERVAL..=HOUR).contains(&self.interval) || HOUR % self.interval != 0 {
            return Err(Error::invalid(
                "clustering.interval must be between one second and one hour and divide an hour \
                 evenly",
            ));
        }

        Ok(())
    }

    /// The time slice of a row at `timestamp` microseconds. The division truncates, as the sort
    /// expression of a compaction merge does, so the two always agree.
    pub fn slice_of(&self, timestamp: i64) -> i64 {
        timestamp / self.interval
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clustering(fields: &[&str], interval: i64) -> Clustering {
        Clustering::new(
            fields.iter().map(|field| field.to_string()).collect(),
            interval,
        )
    }

    #[test]
    fn one_or_two_distinct_fields_and_a_slice_that_divides_an_hour_are_valid() {
        for interval in [1_000_000, 60_000_000, 300_000_000, 600_000_000, HOUR] {
            clustering(&["service"], interval).validate().unwrap();
            clustering(&["service", "level"], interval)
                .validate()
                .unwrap();
        }
    }

    #[test]
    fn too_many_repeated_empty_or_time_fields_are_refused() {
        for fields in [
            &["service", "level", "host"][..],
            &["service", "service"][..],
            &[" "][..],
            &["_timestamp"][..],
        ] {
            assert!(
                clustering(fields, DEFAULT_CLUSTERING_INTERVAL)
                    .validate()
                    .is_err(),
                "{fields:?}"
            );
        }
    }

    #[test]
    fn a_slice_must_divide_an_hour_within_one_second_and_one_hour() {
        for interval in [0, -60_000_000, 999_999, 7_000_000, 2 * HOUR] {
            assert!(
                clustering(&["service"], interval).validate().is_err(),
                "{interval}"
            );
        }
    }

    #[test]
    fn rows_fall_in_the_slice_their_time_truncates_to() {
        let clustering = clustering(&["service"], DEFAULT_CLUSTERING_INTERVAL);

        assert_eq!(clustering.slice_of(0), 0);
        assert_eq!(clustering.slice_of(59_999_999), 0);
        assert_eq!(clustering.slice_of(60_000_000), 1);
        assert_eq!(clustering.slice_of(1_793_836_812_345_678), 29_897_280);
    }

    #[test]
    fn the_interval_defaults_to_a_minute_and_reads_camel_case() {
        let read: Clustering =
            serde_json::from_value(serde_json::json!({ "fields": ["service"] })).unwrap();

        assert_eq!(read.interval, DEFAULT_CLUSTERING_INTERVAL);
        assert!(Clustering::default().is_empty());
        assert_eq!(
            serde_json::to_value(&read).unwrap(),
            serde_json::json!({ "fields": ["service"], "interval": DEFAULT_CLUSTERING_INTERVAL })
        );
    }
}
