// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! File-level Bloom filters: a query skips a file whose filter says a value, or a substring, is
//! certainly absent from it.
//!
//! One encoding serves two places. Beside a sealed partition's manifest, one object holds the
//! filters of every segment of the partition: exact fields get theirs from the segment's Tantivy
//! dictionary, `bloom` and `ngram` fields copy theirs from the segment's own filter artifact. A
//! segment not yet sealed has that artifact alone, holding only its own filters.
//!
//! A `bloom` field's filter holds its whole values; an `ngram` field's holds the byte trigrams of
//! its values, lower-cased, so that a `LIKE` / `ILIKE` literal whose trigrams are not all present
//! cannot occur in the file (see [`grams`]).

use std::collections::HashMap;

use bytes::{BufMut, Bytes, BytesMut};
use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result};

mod build;
mod grams;

pub use build::SegmentFilters;
pub use grams::{Gram, literal_grams};

const MAGIC: &[u8; 4] = b"MSPB";
/// Bits per value: with [`HASHES`] probes about one value in a hundred is let through.
const BITS_PER_VALUE: usize = 10;
const HASHES: u32 = 7;
/// Largest filter of one segment's field. A field with more values gets none: its filter would
/// let most values through.
const MAX_BITS: usize = 8 * 1024 * 1024;
/// Most distinct values a filter is built for.
pub(super) const MAX_VALUES: usize = MAX_BITS / BITS_PER_VALUE;

/// What a filter holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterKind {
    /// Whole values of the field.
    #[default]
    Value,
    /// Byte trigrams of the lower-cased values.
    Gram,
}

impl FilterKind {
    fn is_value(&self) -> bool {
        *self == Self::Value
    }
}

/// A Bloom filter over byte strings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bloom {
    words: Vec<u64>,
    hashes: u32,
}

impl Bloom {
    /// A filter for `values` values, or `None` when it would be larger than [`MAX_BITS`].
    pub(crate) fn for_values(values: usize) -> Option<Self> {
        let bits = values.max(1).checked_mul(BITS_PER_VALUE)?;

        (bits <= MAX_BITS).then(|| Self {
            words: vec![0; bits.div_ceil(64)],
            hashes: HASHES,
        })
    }

    pub(crate) fn insert(&mut self, value: &[u8]) {
        self.insert_probe(Probe::of(value));
    }

    fn insert_probe(&mut self, probe: Probe) {
        for bit in probe.bits(self.words.len(), self.hashes) {
            self.words[bit / 64] |= 1 << (bit % 64);
        }
    }

    /// `false` only when `value` is certainly absent.
    pub fn may_contain(&self, value: &[u8]) -> bool {
        Probe::of(value)
            .bits(self.words.len(), self.hashes)
            .all(|bit| self.words[bit / 64] & (1 << (bit % 64)) != 0)
    }
}

/// The two hashes a value's bits are derived from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Probe {
    first: u64,
    step: u64,
}

impl Probe {
    pub(super) fn of(value: &[u8]) -> Self {
        let hash = blake3::hash(value);
        let bytes = hash.as_bytes();

        Self {
            first: u64::from_le_bytes(bytes[..8].try_into().expect("eight bytes")),
            step: u64::from_le_bytes(bytes[8..16].try_into().expect("eight bytes")) | 1,
        }
    }

    /// The bits the value stands for in a filter of `words` words read with `hashes` probes.
    fn bits(self, words: usize, hashes: u32) -> impl Iterator<Item = usize> {
        let bits = (words * 64) as u64;

        (0..u64::from(hashes)).map(move |probe| {
            (self.first.wrapping_add(probe.wrapping_mul(self.step)) % bits) as usize
        })
    }
}

/// Filters by kind and field, then by segment id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filters {
    fields: HashMap<(FilterKind, String), HashMap<String, Bloom>>,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    field: String,
    segment: String,
    hashes: u32,
    words: usize,
    #[serde(default, skip_serializing_if = "FilterKind::is_value")]
    kind: FilterKind,
}

impl Filters {
    /// Whether `segment` may hold `value` in `field`. `None` when there is no filter for them:
    /// the segment is then read as before.
    pub fn may_contain(&self, field: &str, segment: &str, value: &str) -> Option<bool> {
        let bloom = self
            .fields
            .get(&(FilterKind::Value, field.to_owned()))?
            .get(segment)?;

        Some(bloom.may_contain(value.as_bytes()))
    }

    /// Whether `segment` may hold every trigram of `grams` in `field`, see [`literal_grams`].
    pub fn may_contain_grams(&self, field: &str, segment: &str, grams: &[Gram]) -> Option<bool> {
        let bloom = self
            .fields
            .get(&(FilterKind::Gram, field.to_owned()))?
            .get(segment)?;

        Some(grams.iter().all(|gram| bloom.may_contain(gram)))
    }

    pub fn is_empty(&self) -> bool {
        self.fields.values().all(HashMap::is_empty)
    }

    /// Bytes the filters hold in memory.
    pub fn weight(&self) -> usize {
        self.fields
            .values()
            .flat_map(HashMap::values)
            .map(|bloom| bloom.words.len() * 8)
            .sum()
    }

    pub(crate) fn insert(&mut self, kind: FilterKind, field: &str, segment: &str, bloom: Bloom) {
        self.fields
            .entry((kind, field.to_owned()))
            .or_default()
            .insert(segment.to_owned(), bloom);
    }

    /// Every filter of `segment`, of any kind and field.
    pub(crate) fn of_segment(&self, segment: &str) -> Vec<(FilterKind, String, Bloom)> {
        self.fields
            .iter()
            .filter_map(|((kind, field), segments)| {
                segments
                    .get(segment)
                    .map(|bloom| (*kind, field.clone(), bloom.clone()))
            })
            .collect()
    }

    /// The object: magic, header length, a JSON header of the entries, then their words.
    pub fn encode(&self) -> Result<Bytes> {
        let mut entries = Vec::new();
        let mut words = Vec::new();

        for ((kind, field), segments) in &self.fields {
            for (segment, bloom) in segments {
                entries.push(Entry {
                    field: field.clone(),
                    segment: segment.clone(),
                    hashes: bloom.hashes,
                    words: bloom.words.len(),
                    kind: *kind,
                });

                words.extend_from_slice(&bloom.words);
            }
        }

        let header = serde_json::to_vec(&entries)
            .map_err(|error| Error::internal(format!("encode filters: {error}")))?;

        let mut out = BytesMut::with_capacity(8 + header.len() + words.len() * 8);
        out.put_slice(MAGIC);
        out.put_u32_le(u32::try_from(header.len()).map_err(|_| Error::internal("filter header"))?);
        out.put_slice(&header);

        for word in words {
            out.put_u64_le(word);
        }

        Ok(out.freeze())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let invalid = || Error::internal("filter object is invalid");

        if bytes.len() < 8 || &bytes[..4] != MAGIC {
            return Err(invalid());
        }

        let header_len =
            u32::from_le_bytes(bytes[4..8].try_into().map_err(|_| invalid())?) as usize;
        let header = bytes.get(8..8 + header_len).ok_or_else(invalid)?;

        let entries: Vec<Entry> = serde_json::from_slice(header)
            .map_err(|error| Error::internal(format!("decode filters: {error}")))?;

        let mut offset = 8 + header_len;
        let mut filters = Self::default();

        for entry in entries {
            let end = entry
                .words
                .checked_mul(8)
                .and_then(|len| offset.checked_add(len))
                .ok_or_else(invalid)?;

            let words = bytes
                .get(offset..end)
                .ok_or_else(invalid)?
                .chunks_exact(8)
                .map(|word| u64::from_le_bytes(word.try_into().expect("eight bytes")))
                .collect::<Vec<_>>();

            if words.is_empty() || entry.hashes == 0 {
                return Err(invalid());
            }

            filters.insert(
                entry.kind,
                &entry.field,
                &entry.segment,
                Bloom {
                    words,
                    hashes: entry.hashes,
                },
            );

            offset = end;
        }

        Ok(filters)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filter_keeps_every_value_and_lets_few_others_through() {
        let mut bloom = Bloom::for_values(1_000).unwrap();

        for value in 0..1_000 {
            bloom.insert(format!("trace-{value}").as_bytes());
        }

        assert!((0..1_000).all(|value| bloom.may_contain(format!("trace-{value}").as_bytes())));

        let through = (1_000..11_000)
            .filter(|value| bloom.may_contain(format!("trace-{value}").as_bytes()))
            .count();

        assert!(
            through < 300,
            "{through} of 10000 absent values let through"
        );
    }

    #[test]
    fn a_field_with_too_many_values_gets_no_filter() {
        assert!(Bloom::for_values(MAX_VALUES).is_some());
        assert!(Bloom::for_values(MAX_VALUES + 1).is_none());
    }

    #[test]
    fn the_object_reads_back_as_written() {
        let mut filters = Filters::default();
        let mut bloom = Bloom::for_values(10).unwrap();
        bloom.insert(b"abc");
        filters.insert(FilterKind::Value, "trace_id", "segment-a", bloom.clone());
        filters.insert(FilterKind::Gram, "url", "segment-a", bloom);

        let decoded = Filters::decode(&filters.encode().unwrap()).unwrap();

        assert_eq!(decoded, filters);
        assert_eq!(
            decoded.may_contain("trace_id", "segment-a", "abc"),
            Some(true)
        );
        assert_eq!(decoded.may_contain("trace_id", "segment-b", "abc"), None);
        assert_eq!(decoded.may_contain("url", "segment-a", "abc"), None);
        assert_eq!(
            decoded.may_contain_grams("url", "segment-a", &[*b"abc"]),
            Some(true)
        );
        assert_eq!(decoded.of_segment("segment-a").len(), 2);
        assert!(Filters::decode(b"nope").is_err());
    }

    /// Objects written before filters had kinds hold whole-value filters only.
    #[test]
    fn an_entry_without_a_kind_holds_whole_values() {
        let entry: Entry =
            serde_json::from_str(r#"{"field":"trace_id","segment":"s","hashes":7,"words":1}"#)
                .unwrap();

        assert_eq!(entry.kind, FilterKind::Value);
    }
}
