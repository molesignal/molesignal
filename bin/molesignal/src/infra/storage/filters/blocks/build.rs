// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Block filters built while a segment's rows are written.

use std::{
    collections::HashSet,
    hash::{BuildHasherDefault, Hasher},
};

use super::{
    BLOCK_ROWS, BlockFilters, GRAM_BYTES, GROUP_SLICES, GramBlocks, SliceData, Slices, mix, probe,
};
use crate::infra::storage::filters::grams::pack;

/// Bits of a block filter for each distinct gram of the segment's first block with grams, and the
/// slices a gram probes: about one gram in seven gets through. A literal is ruled out by any one
/// of its grams that a block lacks, and an identifier a block does not hold differs from what it
/// holds in several grams: such a block is read about one time in a hundred.
const BITS_PER_GRAM: usize = 4;
const HASHES: u32 = 3;

/// Most distinct grams the filters are sized for. A first block with more is always read; a later
/// block with more has a filter that lets more through.
const MAX_BLOCK_GRAMS: usize = 32 * 1024;

/// Slots of the cache of grams already in the current block's filter, as a power of two. A log
/// line repeats most of its grams from the lines before it, and those are not hashed again.
const RECENT_SLOTS_LOG: u32 = 16;

/// Hashes a packed gram for the set of a block's grams by mixing its bits.
#[derive(Default)]
struct GramHasher(u64);

impl Hasher for GramHasher {
    fn finish(&self) -> u64 {
        mix(self.0)
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = self.0.rotate_left(8) ^ u64::from(*byte);
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = value;
    }
}

/// The grams of an `ngram` field block by block, while a segment's rows are written.
///
/// Every block's filter gets the bits the segment's first block with grams needed, so they can be
/// stored bit-sliced: that block's grams are collected and counted. After it, a block's grams set
/// their bits as they come. Blocks with more grams let more through.
pub(in crate::infra::storage::filters) struct BlockGrams {
    /// The grams of the first block with grams, each once.
    first: HashSet<u64, BuildHasherDefault<GramHasher>>,
    /// The first block has more grams than a filter is sized for.
    full: bool,
    /// Rows of the current block so far.
    rows: usize,
    /// Blocks closed so far.
    blocks: usize,
    /// Groups of slices of every filter; 0 until a block with grams closes.
    groups: usize,
    /// The filter of the current block once the filters are sized: a word for each group, since
    /// all the probes of a gram fall in one group.
    current: Vec<u64>,
    /// Grams already in `current`, by a hash of the gram, each with its block above its bytes.
    recent: Vec<u64>,
    /// For every 64 blocks, one word for each slice: bit `b % 64` of word `j` is bit `j` of the
    /// filter of block `b`.
    chunks: Vec<u64>,
}

impl BlockGrams {
    pub(in crate::infra::storage::filters) fn new() -> Self {
        Self {
            first: HashSet::default(),
            full: false,
            rows: 0,
            blocks: 0,
            groups: 0,
            current: Vec::new(),
            recent: Vec::new(),
            chunks: Vec::new(),
        }
    }

    /// Adds the grams of the current row's folded value.
    pub(in crate::infra::storage::filters) fn add(&mut self, folded: &[u8]) {
        if self.groups == 0 {
            self.add_to_first(folded);
            return;
        }

        // The block number, above the gram's five bytes, makes an entry stale once its block
        // closes.
        let block = (self.blocks as u64 + 1) << (8 * GRAM_BYTES);

        for window in folded.windows(GRAM_BYTES) {
            let gram = pack(window);
            let slot =
                (gram.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - RECENT_SLOTS_LOG)) as usize;

            if self.recent[slot] != gram | block {
                self.recent[slot] = gram | block;
                let (group, slices) = probe(gram, self.groups, HASHES);
                self.current[group] |= slices;
            }
        }
    }

    /// The current row is done; so is the block once it has [`BLOCK_ROWS`] rows.
    pub(in crate::infra::storage::filters) fn next_row(&mut self) {
        self.rows += 1;

        if self.rows == BLOCK_ROWS {
            self.close();
        }
    }

    /// `rows` rows without a value.
    pub(in crate::infra::storage::filters) fn skip(&mut self, rows: usize) {
        for _ in 0..rows {
            self.next_row();
        }
    }

    /// The filters of the blocks, or `None` when the segment fits in one block: its
    /// whole-segment filter says as much.
    pub(in crate::infra::storage::filters) fn finish(mut self) -> Option<GramBlocks> {
        if self.rows > 0 {
            self.close();
        }

        if self.blocks <= 1 {
            return None;
        }

        // No block had a gram: every filter is empty, and rules out every literal it is asked.
        let groups = self.groups.max(1);

        self.chunks
            .resize(self.blocks.div_ceil(64) * groups * GROUP_SLICES, 0);

        Some(GramBlocks {
            rows: BLOCK_ROWS,
            blocks: self.blocks,
            filters: BlockFilters::Sliced(Slices {
                groups,
                hashes: HASHES,
                data: SliceData::Built(self.chunks),
            }),
        })
    }

    /// Adds the grams of `folded` to the first block's, until there are more than a filter is
    /// sized for.
    fn add_to_first(&mut self, folded: &[u8]) {
        if self.full {
            return;
        }

        self.first.extend(folded.windows(GRAM_BYTES).map(pack));

        if self.first.len() > MAX_BLOCK_GRAMS {
            self.full = true;
            self.first.clear();
        }
    }

    fn close(&mut self) {
        let block = self.blocks;

        self.blocks += 1;
        self.rows = 0;

        if self.groups == 0 && !self.size_from_first() {
            return;
        }

        let slices = self.groups * GROUP_SLICES;
        let chunk = block / 64;
        let bit = 1_u64 << (block % 64);

        if self.chunks.len() < (chunk + 1) * slices {
            self.chunks.resize((chunk + 1) * slices, 0);
        }

        let words = &mut self.chunks[chunk * slices..(chunk + 1) * slices];

        for (group, filter) in self.current.iter_mut().enumerate() {
            let mut set = std::mem::take(filter);

            while set != 0 {
                words[group * GROUP_SLICES + set.trailing_zeros() as usize] |= bit;
                set &= set - 1;
            }
        }
    }

    /// Sizes every filter from the first block's grams and puts them in its filter; `false` when
    /// the block has none, and a later one sizes them.
    fn size_from_first(&mut self) -> bool {
        if self.first.is_empty() && !self.full {
            return false;
        }

        let grams = match self.full {
            true => MAX_BLOCK_GRAMS,
            false => self.first.len(),
        };

        self.groups = (grams * BITS_PER_GRAM).div_ceil(GROUP_SLICES).max(1);
        self.current = vec![if self.full { u64::MAX } else { 0 }; self.groups];
        self.recent = vec![0; 1 << RECENT_SLOTS_LOG];

        for gram in std::mem::take(&mut self.first) {
            let (group, slices) = probe(gram, self.groups, HASHES);
            self.current[group] |= slices;
        }

        self.full = false;
        true
    }
}
