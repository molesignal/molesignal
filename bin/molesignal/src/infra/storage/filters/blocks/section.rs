// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The section of block filters in a filter object, after the whole-segment filters: magic,
//! header length, a JSON header of the entries, then their data.
//!
//! The current section holds bit-sliced filters: for each entry, slice after slice, a word for
//! every 64 blocks. Decoding records where they are and reads none of them. The section of older
//! segments holds a trigram filter for each block, decoded into memory.

use std::collections::HashMap;

use bytes::{BufMut, BytesMut};
use kernel::{Error, Result};
use serde::{Deserialize, Serialize};

use super::{BlockFilters, GRAM_BYTES, GROUP_SLICES, GramBlocks, SliceData, Slices};
use crate::infra::storage::filters::Bloom;

/// Block filters by field, then by segment id.
pub(in crate::infra::storage::filters) type Blocks = HashMap<String, HashMap<String, GramBlocks>>;

const SLICED_MAGIC: &[u8; 4] = b"MSGS";
/// The section of trigram filters that segments written before longer grams have.
const TRIGRAM_MAGIC: &[u8; 4] = b"MSGB";

#[derive(Serialize, Deserialize)]
struct SlicedEntry {
    field: String,
    segment: String,
    rows: usize,
    blocks: usize,
    groups: usize,
    hashes: u32,
    /// Bytes of the grams.
    gram: usize,
}

#[derive(Deserialize)]
struct TrigramEntry {
    field: String,
    segment: String,
    rows: usize,
    hashes: u32,
    /// Words of each block's filter, in block order; 0 for a block without one.
    words: Vec<usize>,
}

/// Appends the section of the block filters built here to `out`; filters read from an object
/// stay there. Nothing when there are none.
pub(in crate::infra::storage::filters) fn encode(
    blocks: &Blocks,
    out: &mut BytesMut,
) -> Result<()> {
    let mut entries = Vec::new();
    let mut data = Vec::new();

    for (field, segments) in blocks {
        for (segment, gram_blocks) in segments {
            let BlockFilters::Sliced(Slices {
                groups,
                hashes,
                data: SliceData::Built(chunks),
            }) = &gram_blocks.filters
            else {
                continue;
            };

            entries.push(SlicedEntry {
                field: field.clone(),
                segment: segment.clone(),
                rows: gram_blocks.rows,
                blocks: gram_blocks.blocks,
                groups: *groups,
                hashes: *hashes,
                gram: GRAM_BYTES,
            });

            data.push((groups * GROUP_SLICES, gram_blocks.words_per_slice(), chunks));
        }
    }

    if entries.is_empty() {
        return Ok(());
    }

    let header = serde_json::to_vec(&entries)
        .map_err(|error| Error::internal(format!("encode block filters: {error}")))?;

    out.put_slice(SLICED_MAGIC);
    out.put_u32_le(u32::try_from(header.len()).map_err(|_| Error::internal("block header"))?);
    out.put_slice(&header);

    for (slices, words, chunks) in data {
        for slice in 0..slices {
            for chunk in 0..words {
                out.put_u64_le(chunks[chunk * slices + slice]);
            }
        }
    }

    Ok(())
}

/// The section of block filters at byte `at` of the filter object `bytes`, or none when there is
/// none there. Entries this code cannot probe are left out.
pub(in crate::infra::storage::filters) fn decode(bytes: &[u8], at: usize) -> Result<Blocks> {
    let section = bytes.get(at..).unwrap_or_default();

    if section.len() < 8 {
        return Ok(Blocks::new());
    }

    let invalid = || Error::internal("block filters are invalid");
    let header_len = u32::from_le_bytes(section[4..8].try_into().map_err(|_| invalid())?) as usize;
    let header = section.get(8..8 + header_len).ok_or_else(invalid)?;
    let data = at + 8 + header_len;

    match &section[..4] {
        magic if magic == SLICED_MAGIC => sliced(header, data, bytes.len()),
        magic if magic == TRIGRAM_MAGIC => trigrams(header, bytes.get(data..).ok_or_else(invalid)?),
        _ => Ok(Blocks::new()),
    }
}

/// The entries of a sliced section whose data starts at byte `data` of an object of `len` bytes.
fn sliced(header: &[u8], data: usize, len: usize) -> Result<Blocks> {
    let invalid = || Error::internal("block filters are invalid");

    let entries: Vec<SlicedEntry> = serde_json::from_slice(header)
        .map_err(|error| Error::internal(format!("decode block filters: {error}")))?;

    let mut blocks = Blocks::new();
    let mut offset = data;

    for entry in entries {
        if entry.rows == 0 || entry.blocks == 0 || entry.groups == 0 || entry.hashes == 0 {
            return Err(invalid());
        }

        let bytes = (entry.groups * GROUP_SLICES)
            .checked_mul(entry.blocks.div_ceil(64) * 8)
            .ok_or_else(invalid)?;

        let start = offset;
        offset = offset.checked_add(bytes).ok_or_else(invalid)?;

        if offset > len {
            return Err(invalid());
        }

        if entry.gram != GRAM_BYTES {
            continue;
        }

        let filters = BlockFilters::Sliced(Slices {
            groups: entry.groups,
            hashes: entry.hashes,
            data: SliceData::Stored(start as u64),
        });

        blocks.entry(entry.field).or_default().insert(
            entry.segment,
            GramBlocks {
                rows: entry.rows,
                blocks: entry.blocks,
                filters,
            },
        );
    }

    Ok(blocks)
}

/// The entries of a trigram section, with `data` their words.
fn trigrams(header: &[u8], data: &[u8]) -> Result<Blocks> {
    let invalid = || Error::internal("block filters are invalid");

    let entries: Vec<TrigramEntry> = serde_json::from_slice(header)
        .map_err(|error| Error::internal(format!("decode block filters: {error}")))?;

    let mut blocks = Blocks::new();
    let mut offset = 0_usize;

    for entry in entries {
        if entry.rows == 0 || entry.hashes == 0 {
            return Err(invalid());
        }

        let mut blooms = Vec::with_capacity(entry.words.len());

        for words in &entry.words {
            if *words == 0 {
                blooms.push(None);
                continue;
            }

            let end = words
                .checked_mul(8)
                .and_then(|len| offset.checked_add(len))
                .ok_or_else(invalid)?;

            let words = data
                .get(offset..end)
                .ok_or_else(invalid)?
                .chunks_exact(8)
                .map(|word| u64::from_le_bytes(word.try_into().expect("eight bytes")))
                .collect();

            blooms.push(Some(Bloom {
                words,
                hashes: entry.hashes,
            }));

            offset = end;
        }

        blocks.entry(entry.field).or_default().insert(
            entry.segment,
            GramBlocks {
                rows: entry.rows,
                blocks: blooms.len(),
                filters: BlockFilters::Trigrams(blooms),
            },
        );
    }

    Ok(blocks)
}
