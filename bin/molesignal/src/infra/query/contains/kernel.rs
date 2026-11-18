// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Which rows of a text column hold one of a set of needles, found with one pass over the
//! column's values instead of one search per row.
//!
//! A string column keeps its values back to back in one buffer. The search runs over the whole
//! buffer with SIMD (`memchr`'s memmem for one needle, aho-corasick's Teddy for several); a hit is
//! mapped to its row through the offsets, a hit that runs into the next value is discarded, and the
//! search resumes at the next row once a row has matched. A log line is a few hundred bytes, so
//! searching row by row spends much of its time starting the search over.
//!
//! Case-insensitive searches fold ASCII letters, as Arrow's `ILIKE` does for an ASCII pattern: the
//! buffer is copied folded to lower case (a vectorized loop), and the lower-case needles are looked
//! for in the copy, at the same offsets.

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, Input, MatchKind};
use arrow::array::{
    Array, BooleanArray, BooleanBufferBuilder, GenericStringArray, OffsetSizeTrait,
};
use memchr::memmem::Finder;

/// The needles of one search.
pub(super) struct Search {
    finder: Option<Finder<'static>>,
    automaton: Option<AhoCorasick>,
    fold: bool,
}

impl Search {
    /// A search for any of `needles`, which must not be empty; `fold` ignores ASCII case.
    pub(super) fn new(needles: &[String], fold: bool) -> Result<Self, String> {
        let needles: Vec<String> = needles
            .iter()
            .map(|needle| {
                if fold {
                    needle.to_ascii_lowercase()
                } else {
                    needle.clone()
                }
            })
            .collect();

        match needles.as_slice() {
            [] => Err("contains_text needs at least one needle".to_owned()),
            [needle] => Ok(Self {
                finder: Some(Finder::new(needle.as_bytes()).into_owned()),
                automaton: None,
                fold,
            }),
            _ => Ok(Self {
                finder: None,
                automaton: Some(
                    AhoCorasickBuilder::new()
                        .match_kind(MatchKind::LeftmostFirst)
                        .build(&needles)
                        .map_err(|error| error.to_string())?,
                ),
                fold,
            }),
        }
    }

    /// The first needle in `haystack`: where it starts and how long it is.
    fn find(&self, haystack: &[u8]) -> Option<(usize, usize)> {
        if let Some(finder) = &self.finder {
            return finder
                .find(haystack)
                .map(|start| (start, finder.needle().len()));
        }

        self.automaton
            .as_ref()?
            .find(Input::new(haystack))
            .map(|found| (found.start(), found.len()))
    }

    /// Whether `text` holds a needle.
    pub(super) fn holds(&self, text: &str, scratch: &mut Vec<u8>) -> bool {
        self.find(self.folded(text.as_bytes(), scratch)).is_some()
    }

    fn folded<'a>(&self, bytes: &'a [u8], scratch: &'a mut Vec<u8>) -> &'a [u8] {
        if !self.fold {
            return bytes;
        }

        scratch.clear();
        scratch.extend(bytes.iter().map(u8::to_ascii_lowercase));
        scratch
    }

    /// For each row of `array`, whether its value holds a needle; null where the value is.
    pub(super) fn rows<O: OffsetSizeTrait>(
        &self,
        array: &GenericStringArray<O>,
        scratch: &mut Vec<u8>,
    ) -> BooleanArray {
        let rows = array.len();
        let offsets = array.value_offsets();
        let first = offsets[0].as_usize();
        let values = &array.value_data()[first..offsets[rows].as_usize()];
        let haystack = self.folded(values, scratch);

        let mut hits = BooleanBufferBuilder::new(rows);
        hits.append_n(rows, false);

        let end_of = |row: usize| offsets[row + 1].as_usize() - first;
        let mut row = 0;
        let mut position = 0;

        while row < rows && position < haystack.len() {
            let Some((start, length)) = self.find(&haystack[position..]) else {
                break;
            };

            let at = position + start;

            // The row the hit starts in: the first whose value ends after `at`.
            row += offsets[row + 1..=rows].partition_point(|end| end.as_usize() - first <= at);

            if row == rows {
                break;
            }

            if at + length <= end_of(row) {
                hits.set_bit(row, true);
                position = end_of(row);
                row += 1;
            } else {
                position = at + 1;
            }
        }

        BooleanArray::new(hits.finish(), array.nulls().cloned())
    }
}

#[cfg(test)]
mod tests {
    use arrow::array::StringArray;

    use super::*;

    fn rows(values: &[Option<&str>], needles: &[&str], fold: bool) -> Vec<Option<bool>> {
        let needles: Vec<String> = needles.iter().map(|needle| (*needle).to_owned()).collect();
        let search = Search::new(&needles, fold).unwrap();
        let array = StringArray::from(values.to_vec());

        search.rows(&array, &mut Vec::new()).iter().collect()
    }

    /// Every case against the row-by-row answer.
    #[test]
    fn hits_land_on_their_rows_and_never_span_two() {
        let values = [
            Some("needle at the start"),
            Some("ends with nee"),
            Some("dle starts the next"),
            None,
            Some(""),
            Some("twice: needle needle"),
            Some("NEEDLE in capitals"),
            Some("no"),
        ];

        assert_eq!(
            rows(&values, &["needle"], false),
            [
                Some(true),
                Some(false),
                Some(false),
                None,
                Some(false),
                Some(true),
                Some(false),
                Some(false)
            ]
        );

        assert_eq!(
            rows(&values, &["needle"], true),
            [
                Some(true),
                Some(false),
                Some(false),
                None,
                Some(false),
                Some(true),
                Some(true),
                Some(false)
            ]
        );
    }

    #[test]
    fn several_needles_match_any() {
        let values = [Some("alpha"), Some("beta"), Some("gamma"), Some("alphabet")];

        assert_eq!(
            rows(&values, &["beta", "gam"], false),
            [Some(false), Some(true), Some(true), Some(false)]
        );
        assert_eq!(
            rows(&values, &["BET", "zzz"], true),
            [Some(false), Some(true), Some(false), Some(true)]
        );
    }

    /// A sliced array starts its values part way into the buffer.
    #[test]
    fn a_slice_of_a_column_is_searched_from_its_own_first_value() {
        let array = StringArray::from(vec!["needle", "hay", "needle", "hay"]);
        let slice = array.slice(1, 3);
        let slice = slice.as_any().downcast_ref::<StringArray>().unwrap();
        let search = Search::new(&["needle".to_owned()], false).unwrap();

        let found: Vec<_> = search.rows(slice, &mut Vec::new()).iter().collect();

        assert_eq!(found, [Some(false), Some(true), Some(false)]);
    }

    #[test]
    fn non_ascii_text_is_searched_byte_for_byte() {
        let values = [
            Some("调用完成 needle"),
            Some("ÄÖÜ needle"),
            Some("调用失败"),
        ];

        assert_eq!(
            rows(&values, &["调用完成"], false),
            [Some(true), Some(false), Some(false)]
        );
        assert_eq!(
            rows(&values, &["NEEDLE"], true),
            [Some(true), Some(true), Some(false)]
        );
    }
}
