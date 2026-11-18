// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A segment's minute counts: how many of its rows fall in each minute with each combination of
//! the values of its clustering fields. A segment written clustered (see [`Clustering`]) gets them
//! beside its file, so a count, a count by one of those fields or a count by minute, with equality
//! conditions on them, is answered without reading the file, also where the query window cuts the
//! file at whole minutes.
//!
//! The fields a stream is clustered by are the ones its queries name most and that take few
//! values, so the counts stay small: a few thousand entries for a file of millions of rows. A
//! segment whose combinations would pass [`MAX_ENTRIES`] gets none and is read as before.
//!
//! [`Clustering`]: domain::stream::Clustering

use std::ops::RangeInclusive;

use bytes::{BufMut, Bytes, BytesMut};
use kernel::{Error, Result};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

mod build;
mod cache;
mod partition;

pub use build::MinuteCountsBuilder;
pub use cache::{MinuteCountsCache, MinuteCountsKeys};
pub use partition::PartitionCounts;

/// Microseconds in a minute, the counts' unit of time.
pub const MINUTE: i64 = 60_000_000;

/// Format of the objects written now.
pub const FORMAT_VERSION: u32 = 1;

/// Most entries one segment's counts hold.
const MAX_ENTRIES: usize = 1 << 16;

/// Most fields counts can be kept by; a clustering has at most two.
const MAX_FIELDS: usize = 8;

/// Largest decompressed object read back: well above what [`MAX_ENTRIES`] entries take.
const MAX_DECODED_BYTES: usize = 64 * 1024 * 1024;

const MAGIC: &[u8; 4] = b"MSMC";

/// The counts of one segment, entry by entry in order of minute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinuteCounts {
    /// The fields the rows are counted by.
    fields: Vec<String>,
    /// Each field's values, sorted. An entry names a value by its place here, and a row with no
    /// value by the number of values.
    values: Vec<Vec<String>>,
    /// Each entry's minute: its rows' time in microseconds since the epoch divided by [`MINUTE`],
    /// rounded down.
    minutes: Vec<i64>,
    /// For each field, each entry's value.
    ids: Vec<Vec<u32>>,
    /// Each entry's rows.
    rows: Vec<u64>,
}

impl MinuteCounts {
    /// The fields the rows are counted by.
    pub fn fields(&self) -> &[String] {
        &self.fields
    }

    /// The place of `name` among the fields.
    pub fn field(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|field| field == name)
    }

    /// Equality conditions `(column, value)` as places among the fields, for [`Self::entries`];
    /// `None` when a column is not one of them.
    pub fn places<'a>(&self, filters: &'a [(String, String)]) -> Option<Vec<(usize, &'a str)>> {
        filters
            .iter()
            .map(|(column, value)| Some((self.field(column)?, value.as_str())))
            .collect()
    }

    /// The entries of `minutes` whose value of each field in `equal`, `(place of the field,
    /// value)`, is the one given.
    pub fn entries<'a>(
        &'a self,
        minutes: RangeInclusive<i64>,
        equal: &[(usize, &str)],
    ) -> impl Iterator<Item = usize> + 'a {
        let start = self
            .minutes
            .partition_point(|minute| minute < minutes.start());
        let end = self
            .minutes
            .partition_point(|minute| minute <= minutes.end());

        // A value the segment never had leaves no entry to visit.
        let wanted = equal
            .iter()
            .map(|(field, value)| {
                let id = self
                    .values
                    .get(*field)?
                    .binary_search_by(|known| known.as_str().cmp(value));

                Some((*field, u32::try_from(id.ok()?).ok()?))
            })
            .collect::<Option<Vec<_>>>();

        let end = match wanted {
            Some(_) => end.max(start),
            None => start,
        };

        let wanted = wanted.unwrap_or_default();

        (start..end).filter(move |&entry| {
            wanted
                .iter()
                .all(|(field, id)| self.ids[*field][entry] == *id)
        })
    }

    /// Rows of every entry: the segment's rows.
    pub fn total(&self) -> u64 {
        self.rows.iter().copied().fold(0, u64::saturating_add)
    }

    pub fn minute(&self, entry: usize) -> i64 {
        self.minutes[entry]
    }

    pub fn rows(&self, entry: usize) -> u64 {
        self.rows[entry]
    }

    /// The entry's value of the field at `field`; `None` for rows with no value.
    pub fn value(&self, field: usize, entry: usize) -> Option<&str> {
        let values = &self.values[field];

        values
            .get(self.ids[field][entry] as usize)
            .map(String::as_str)
    }

    /// Bytes the counts hold in memory.
    pub fn weight(&self) -> usize {
        let values = self
            .values
            .iter()
            .flatten()
            .map(|value| value.len() + 24)
            .sum::<usize>();

        self.minutes.len() * (16 + 4 * self.fields.len()) + values
    }

    /// The object: the magic, then the counts in CBOR, compressed with zstd.
    pub fn encode(&self) -> Result<Bytes> {
        pack(MAGIC, self)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let counts: Self = unpack(MAGIC, bytes)?;

        counts
            .consistent()
            .then_some(counts)
            .ok_or_else(|| Error::internal("minute counts object is invalid"))
    }

    /// Every entry has its minute, rows and a value of each field, values name known ones, and
    /// the entries are in order of minute.
    fn consistent(&self) -> bool {
        let entries = self.minutes.len();

        self.fields.len() <= MAX_FIELDS
            && entries <= MAX_ENTRIES
            && self.values.len() == self.fields.len()
            && self.ids.len() == self.fields.len()
            && self.rows.len() == entries
            && self.minutes.is_sorted()
            && self.values.iter().all(|values| values.is_sorted())
            && self.ids.iter().zip(&self.values).all(|(ids, values)| {
                ids.len() == entries && ids.iter().all(|id| *id as usize <= values.len())
            })
    }
}

/// `value` in CBOR, compressed with zstd, after `magic`.
fn pack(magic: &[u8; 4], value: &impl Serialize) -> Result<Bytes> {
    let mut cbor = Vec::new();

    ciborium::into_writer(value, &mut cbor)
        .map_err(|error| Error::internal(format!("encode minute counts: {error}")))?;

    let compressed = zstd::bulk::compress(&cbor, 3)
        .map_err(|error| Error::internal(format!("compress minute counts: {error}")))?;

    let mut out = BytesMut::with_capacity(magic.len() + compressed.len());
    out.put_slice(magic);
    out.put_slice(&compressed);
    Ok(out.freeze())
}

/// What [`pack`] packed after `magic`.
fn unpack<T: DeserializeOwned>(magic: &[u8; 4], bytes: &[u8]) -> Result<T> {
    let compressed = bytes
        .strip_prefix(magic.as_slice())
        .ok_or_else(|| Error::internal("minute counts object is invalid"))?;

    let cbor = zstd::bulk::decompress(compressed, MAX_DECODED_BYTES)
        .map_err(|error| Error::internal(format!("decompress minute counts: {error}")))?;

    ciborium::from_reader(cbor.as_slice())
        .map_err(|error| Error::internal(format!("decode minute counts: {error}")))
}

/// The minutes of a file's rows inside `[start, end)` when the window cuts the file, whose rows
/// are timed from `first` to `last`, only between minutes; `None` when it cuts one.
pub fn whole_minutes(start: i64, end: i64, first: i64, last: i64) -> Option<RangeInclusive<i64>> {
    let from = match start <= first {
        true => i64::MIN,
        false => (start % MINUTE == 0).then(|| start.div_euclid(MINUTE))?,
    };

    let to = match end > last {
        true => i64::MAX,
        false => (end % MINUTE == 0).then(|| end.div_euclid(MINUTE) - 1)?,
    };

    Some(from..=to)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts() -> MinuteCounts {
        MinuteCounts {
            fields: vec!["service".into(), "level".into()],
            values: vec![
                vec!["api".into(), "web".into()],
                vec!["ERROR".into(), "INFO".into()],
            ],
            minutes: vec![10, 10, 11, 12, 12],
            ids: vec![vec![0, 1, 0, 1, 2], vec![0, 1, 1, 0, 2]],
            rows: vec![3, 5, 7, 11, 13],
        }
    }

    fn rows(counts: &MinuteCounts, minutes: RangeInclusive<i64>, equal: &[(usize, &str)]) -> u64 {
        counts
            .entries(minutes, equal)
            .map(|entry| counts.rows(entry))
            .sum()
    }

    #[test]
    fn entries_are_chosen_by_minute_and_value() {
        let counts = counts();

        assert_eq!(rows(&counts, i64::MIN..=i64::MAX, &[]), 39);
        assert_eq!(rows(&counts, 11..=12, &[]), 31);
        assert_eq!(rows(&counts, 10..=12, &[(0, "api")]), 10);
        assert_eq!(rows(&counts, 10..=12, &[(0, "web"), (1, "ERROR")]), 11);
        assert_eq!(rows(&counts, 10..=12, &[(1, "WARN")]), 0);
        assert_eq!(rows(&counts, 13..=20, &[]), 0);

        // A window that ends where it starts holds no minute.
        let empty = whole_minutes(12 * MINUTE, 12 * MINUTE, 0, 20 * MINUTE).unwrap();
        assert_eq!(rows(&counts, empty, &[]), 0);
    }

    #[test]
    fn an_entry_without_a_value_has_none() {
        let counts = counts();

        assert_eq!(counts.value(0, 4), None);
        assert_eq!(counts.value(1, 2), Some("INFO"));
        assert_eq!(counts.field("level"), Some(1));
        assert_eq!(counts.field("host"), None);
    }

    #[test]
    fn counts_come_back_as_they_were_written() {
        let counts = counts();
        let bytes = counts.encode().unwrap();

        assert_eq!(MinuteCounts::decode(&bytes).unwrap(), counts);
        assert!(MinuteCounts::decode(&bytes[1..]).is_err());
    }

    #[test]
    fn inconsistent_counts_are_refused() {
        let mut counts = counts();
        counts.ids[1][0] = 3;

        assert!(MinuteCounts::decode(&counts.encode().unwrap()).is_err());

        let mut counts = self::counts();
        counts.minutes.swap(0, 2);

        assert!(MinuteCounts::decode(&counts.encode().unwrap()).is_err());
    }

    #[test]
    fn a_window_answers_by_minute_only_where_it_cuts_between_minutes() {
        let minute = MINUTE;

        // The file holds minutes 10 to 12.
        let (first, last) = (10 * minute + 5, 12 * minute + 7);

        assert_eq!(
            whole_minutes(0, i64::MAX, first, last),
            Some(i64::MIN..=i64::MAX)
        );
        assert_eq!(
            whole_minutes(11 * minute, 12 * minute, first, last),
            Some(11..=11)
        );
        assert_eq!(whole_minutes(11 * minute + 1, i64::MAX, first, last), None);
        assert_eq!(whole_minutes(0, 12 * minute + 7, first, last), None);
        assert_eq!(
            whole_minutes(10 * minute + 5, 12 * minute + 8, first, last),
            Some(i64::MIN..=i64::MAX)
        );
    }
}
