// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! An `ngram` field's filters block by block: one Bloom filter for every [`BLOCK_ROWS`] rows of a
//! segment, in the order they are written, holding the [`GRAM_BYTES`]-byte grams of the folded
//! values. A `LIKE` literal spread thinly over a segment is in every segment's filter, yet absent
//! from most of its blocks; a query reads only the blocks that may hold it. A stream with an
//! `ngram` field writes pages of [`BLOCK_ROWS`] rows that start where blocks start (see the Parquet
//! writer), so every block left out is a page never read.
//!
//! The grams are longer than the whole-segment filters' trigrams because the trigrams of
//! identifiers and numbers recur in every block: trigram blocks rule out almost nothing a segment
//! holds. A literal shorter than [`GRAM_BYTES`] bytes gets no grams here, and every block is read.
//!
//! The filters of a segment's blocks all have the same bits, stored bit-sliced: slice `j` holds
//! bit `j` of every block's filter, one bit per block. A gram probes a few slices of one group of
//! 64, so a query reads only the slices its literal's grams probe, a few hundred bytes each, and the
//! filters never need to be in memory: the section is far larger than a segment's whole-segment
//! filters. Segments written before the longer grams hold a trigram filter for each block instead,
//! which is read whole and kept in memory.
//!
//! The block filters follow the whole-segment ones in the same object, in a section of their own:
//! a reader that does not know the section stops before it. Only a segment's own filter artifact
//! has them; the filters of a sealed partition copy the whole-segment ones only.

use std::ops::Range;

use bytes::Bytes;

use super::{
    Bloom, Probe,
    grams::{LiteralGrams, pack},
};

mod build;
mod section;
#[cfg(test)]
mod tests;

pub(super) use build::BlockGrams;
pub(super) use section::{Blocks, decode, encode};

/// Rows of a block, and of a page of a stream with an `ngram` field.
pub const BLOCK_ROWS: usize = 1024;

/// Bytes of a block gram.
pub(super) const GRAM_BYTES: usize = 5;

/// Slices in a group: all the probes of one gram fall in one group.
const GROUP_SLICES: usize = 64;

/// The block filters of one field of one segment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GramBlocks {
    /// Rows of each block; the last one may have fewer.
    rows: usize,
    blocks: usize,
    filters: BlockFilters,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BlockFilters {
    /// The trigram filters of an older segment, one for each block; `None` for a block without
    /// one.
    Trigrams(Vec<Option<Bloom>>),
    Sliced(Slices),
}

/// Bit-sliced block filters: `groups` groups of [`GROUP_SLICES`] slices, each slice one bit for
/// every block.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Slices {
    groups: usize,
    hashes: u32,
    data: SliceData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SliceData {
    /// Built here: for every 64 blocks, one word for each slice.
    Built(Vec<u64>),
    /// At this byte offset of the filter object, slice after slice.
    Stored(u64),
}

impl GramBlocks {
    /// Rows of each block; the last one may have fewer.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// How many blocks there are.
    pub fn len(&self) -> usize {
        self.blocks
    }

    pub fn is_empty(&self) -> bool {
        self.blocks == 0
    }

    /// The byte ranges of the filter object that [`Self::may_hold`] needs for `grams`, in order:
    /// none when the filters are in memory or `grams` has no block grams.
    pub fn ranges(&self, grams: &LiteralGrams) -> Vec<Range<u64>> {
        let BlockFilters::Sliced(slices) = &self.filters else {
            return Vec::new();
        };

        let SliceData::Stored(offset) = slices.data else {
            return Vec::new();
        };

        let bytes = self.words_per_slice() as u64 * 8;

        slices
            .probed(grams)
            .into_iter()
            .map(|slice| {
                let start = offset + slice as u64 * bytes;
                start..start + bytes
            })
            .collect()
    }

    /// For each block, `false` only when it certainly lacks one of `grams`. `read` holds the bytes
    /// of [`Self::ranges`]; `None` when it does not.
    pub fn may_hold(&self, grams: &LiteralGrams, read: &[Bytes]) -> Option<Vec<bool>> {
        match &self.filters {
            BlockFilters::Trigrams(blooms) => Some(
                blooms
                    .iter()
                    .map(|bloom| {
                        bloom
                            .as_ref()
                            .is_none_or(|bloom| trigrams_may_hold(bloom, grams))
                    })
                    .collect(),
            ),
            BlockFilters::Sliced(slices) => self.sliced_may_hold(slices, grams, read),
        }
    }

    fn sliced_may_hold(
        &self,
        slices: &Slices,
        grams: &LiteralGrams,
        read: &[Bytes],
    ) -> Option<Vec<bool>> {
        let words = self.words_per_slice();
        let probed = slices.probed(grams);

        let mut held: Vec<Vec<u64>> = Vec::with_capacity(probed.len());

        match &slices.data {
            SliceData::Built(chunks) => {
                let per_chunk = slices.groups * GROUP_SLICES;

                for slice in &probed {
                    held.push(
                        (0..words)
                            .map(|chunk| chunks[chunk * per_chunk + slice])
                            .collect(),
                    );
                }
            }
            SliceData::Stored(_) => {
                if read.len() != probed.len() || read.iter().any(|bytes| bytes.len() != words * 8) {
                    return None;
                }

                for bytes in read {
                    let slice = bytes
                        .chunks_exact(8)
                        .map(|word| u64::from_le_bytes(word.try_into().expect("eight bytes")));

                    held.push(slice.collect());
                }
            }
        }

        let mut blocks = vec![u64::MAX; words];

        for slice in &held {
            for (block, word) in blocks.iter_mut().zip(slice) {
                *block &= word;
            }
        }

        Some(
            (0..self.blocks)
                .map(|block| (blocks[block / 64] >> (block % 64)) & 1 == 1)
                .collect(),
        )
    }

    /// Words of one slice: a bit for each block.
    fn words_per_slice(&self) -> usize {
        self.blocks.div_ceil(64)
    }

    /// Bytes the filters hold in memory.
    pub(super) fn weight(&self) -> usize {
        match &self.filters {
            BlockFilters::Trigrams(blooms) => blooms
                .iter()
                .flatten()
                .map(|bloom| bloom.words.len() * 8)
                .sum(),
            BlockFilters::Sliced(Slices {
                data: SliceData::Built(chunks),
                ..
            }) => chunks.len() * 8,
            BlockFilters::Sliced(_) => 64,
        }
    }
}

impl Slices {
    /// The slices the block grams of `grams` probe, each once, in order.
    fn probed(&self, grams: &LiteralGrams) -> Vec<usize> {
        let mut slices = grams
            .block_grams()
            .iter()
            .flat_map(|gram| positions(*gram, self.groups, self.hashes))
            .collect::<Vec<_>>();

        slices.sort_unstable();
        slices.dedup();
        slices
    }
}

fn trigrams_may_hold(bloom: &Bloom, grams: &LiteralGrams) -> bool {
    grams
        .trigrams()
        .iter()
        .all(|gram| bloom.may_contain_probe(Probe::of_packed(pack(gram))))
}

/// The bits a packed gram sets in a block filter of `groups` groups: `hashes` slices of the one
/// group the gram picks, each once.
fn positions(gram: u64, groups: usize, hashes: u32) -> impl Iterator<Item = usize> {
    let (group, mut slices) = probe(gram, groups, hashes);

    std::iter::from_fn(move || {
        let slice = (slices != 0).then(|| slices.trailing_zeros() as usize)?;
        slices &= slices - 1;

        Some(group * GROUP_SLICES + slice)
    })
}

/// The group a packed gram picks in a block filter of `groups` groups, and its `hashes` slices in
/// that group as the bits of one word.
fn probe(gram: u64, groups: usize, hashes: u32) -> (usize, u64) {
    let first = mix(gram);
    let second = mix(first);
    let step = second | 1;

    let slices = (0..u64::from(hashes)).fold(0, |slices, probe| {
        let slice = (second >> 32).wrapping_add(probe.wrapping_mul(step)) % GROUP_SLICES as u64;
        slices | (1 << slice)
    });

    ((first % groups as u64) as usize, slices)
}

impl Probe {
    /// A gram's probe in a trigram block filter of an older segment: its packed bytes mixed.
    pub(super) fn of_packed(gram: u64) -> Self {
        let first = mix(gram);

        Self {
            first,
            step: mix(first) | 1,
        }
    }
}

/// The SplitMix64 finalizer: every bit of `value` changes about half of the result.
fn mix(value: u64) -> u64 {
    let value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);

    value ^ (value >> 31)
}
