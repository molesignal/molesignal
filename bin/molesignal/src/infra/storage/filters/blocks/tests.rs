// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use bytes::{BufMut, Bytes, BytesMut};

use super::*;
use crate::infra::storage::filters::grams::{fold, pack};

/// The blocks of `rows` values, one value per row.
fn blocks_of(rows: &[&str]) -> Option<GramBlocks> {
    let mut blocks = BlockGrams::new();
    let mut buffer = String::new();

    for value in rows {
        blocks.add(fold(value, &mut buffer));
        blocks.next_row();
    }

    blocks.finish()
}

fn grams(literal: &str) -> LiteralGrams {
    let mut grams = LiteralGrams::default();
    grams.add(literal, false);
    grams
}

fn holds(blocks: &GramBlocks, literal: &str) -> Vec<bool> {
    blocks
        .may_hold(&grams(literal), &[])
        .expect("filters in memory")
}

/// `blocks` of `field` in `segment`, written after `at` bytes of something else, as an object.
fn stored(blocks: GramBlocks, at: usize) -> (Bytes, Blocks) {
    let mut section = Blocks::new();
    section
        .entry("message".to_owned())
        .or_default()
        .insert("segment".to_owned(), blocks);

    let mut out = BytesMut::from(&vec![7_u8; at][..]);
    encode(&section, &mut out).unwrap();

    let object = out.freeze();
    let decoded = decode(&object, at).unwrap();

    (object, decoded)
}

#[test]
fn a_literal_is_looked_for_only_in_the_blocks_that_hold_it() {
    let mut rows = ["request processed"; BLOCK_ROWS * 2 + 10];
    rows[BLOCK_ROWS + 7] = "request failed: needle0001";

    let blocks = blocks_of(&rows).expect("three blocks");

    assert_eq!((blocks.rows(), blocks.len()), (BLOCK_ROWS, 3));
    assert_eq!(holds(&blocks, "needle0001"), [false, true, false]);

    // Every block holds what every row holds, the last one too.
    assert_eq!(holds(&blocks, "processed"), [true; 3]);
}

/// The trigrams of `needle0010` and `needle0100` together hold every trigram of `needle0001`;
/// its five-byte grams do not.
#[test]
fn blocks_tell_apart_literals_that_share_their_trigrams() {
    let mut rows = ["id needle0010 needle0100"; BLOCK_ROWS * 2];
    rows[BLOCK_ROWS + 3] = "id needle0001";

    let blocks = blocks_of(&rows).expect("two blocks");

    assert_eq!(holds(&blocks, "needle0001"), [false, true]);
}

#[test]
fn a_literal_shorter_than_a_gram_reads_every_block() {
    let mut rows = ["request processed"; BLOCK_ROWS * 2];
    rows[0] = "code E017";

    let blocks = blocks_of(&rows).expect("two blocks");

    assert_eq!(holds(&blocks, "E017"), [true, true]);
    assert_eq!(holds(&blocks, "code E017"), [true, false]);
}

#[test]
fn a_segment_of_one_block_gets_none() {
    assert_eq!(blocks_of(&["needle"; BLOCK_ROWS]), None);
    assert!(blocks_of(&["needle"; BLOCK_ROWS + 1]).is_some());
}

#[test]
fn rows_without_a_value_still_fill_blocks() {
    let mut blocks = BlockGrams::new();
    blocks.skip(BLOCK_ROWS);
    blocks.add(b"needle");
    blocks.next_row();

    let blocks = blocks.finish().expect("two blocks");

    assert_eq!(holds(&blocks, "needle"), [false, true]);
}

/// A block with too many distinct grams has a filter that holds everything.
#[test]
fn a_block_with_too_many_grams_is_always_read() {
    let mut blocks = BlockGrams::new();
    let words = (0..=32 * 1024)
        .map(|value| format!("{value:08}"))
        .collect::<Vec<_>>();

    blocks.add(words.join(" ").as_bytes());
    blocks.skip(BLOCK_ROWS);
    blocks.add(b"needle");
    blocks.skip(1);

    let blocks = blocks.finish().expect("two blocks");

    assert_eq!(holds(&blocks, "needle"), [true, true]);
    assert_eq!(holds(&blocks, "absent"), [true, false]);
}

/// Each block's filter holds the slices of its own rows' grams, and only those: the first block's
/// grams are collected and counted, a later block's set their bits as they come, the ones already
/// set skipped.
#[test]
fn each_block_filter_holds_the_slices_of_its_own_grams() {
    let rows = (0..BLOCK_ROWS * 3 + 100)
        .map(|row| {
            let user = row * 7919 % 100_000;
            format!(
                "GET /api/orders/{} user=u{user:05} took {}ms",
                row % 37,
                row % 1000
            )
        })
        .collect::<Vec<_>>();

    let mut buffer = String::new();
    let mut built = BlockGrams::new();
    let mut grams = vec![HashSet::new(); 4];

    for (row, value) in rows.iter().enumerate() {
        let folded = fold(value, &mut buffer);
        grams[row / BLOCK_ROWS].extend(folded.windows(GRAM_BYTES).map(pack));
        built.add(folded);
        built.next_row();
    }

    let blocks = built.finish().expect("four blocks");

    let BlockFilters::Sliced(Slices {
        groups,
        hashes,
        data: SliceData::Built(chunks),
    }) = &blocks.filters
    else {
        panic!("filters built here are sliced");
    };

    let slices = groups * GROUP_SLICES;

    for (block, grams) in grams.iter().enumerate() {
        let mut wanted = vec![0_u64; *groups];

        for gram in grams {
            let (group, bits) = probe(*gram, *groups, *hashes);
            wanted[group] |= bits;
        }

        for slice in 0..slices {
            let held = (chunks[block / 64 * slices + slice] >> (block % 64)) & 1 == 1;
            let set = (wanted[slice / GROUP_SLICES] >> (slice % GROUP_SLICES)) & 1 == 1;

            assert_eq!(held, set, "block {block}, slice {slice}");
        }
    }
}

#[test]
fn stored_filters_answer_from_the_slices_they_read() {
    let mut rows = vec!["request processed"; BLOCK_ROWS * 70];
    rows[BLOCK_ROWS * 66 + 5] = "request failed: needle0001";
    rows[3] = "request failed: needle0001";

    let built = blocks_of(&rows).expect("seventy blocks");
    let (object, decoded) = stored(built.clone(), 13);
    let stored = &decoded["message"]["segment"];

    for literal in ["needle0001", "processed", "absent here", "E017"] {
        let ranges = stored.ranges(&grams(literal));

        let read = ranges
            .iter()
            .map(|range| object.slice(range.start as usize..range.end as usize))
            .collect::<Vec<_>>();

        assert_eq!(
            stored.may_hold(&grams(literal), &read),
            built.may_hold(&grams(literal), &[])
        );
    }

    let wanted = stored.may_hold(&grams("needle0001"), &[]);
    let expected = (0..70)
        .map(|block| block == 0 || block == 66)
        .collect::<Vec<_>>();

    assert_eq!(built.may_hold(&grams("needle0001"), &[]), Some(expected));
    assert_eq!(wanted, None);
    assert!(built.ranges(&grams("needle0001")).is_empty());
}

#[test]
fn a_section_cut_short_is_invalid_and_none_is_empty() {
    let built = blocks_of(&["needle"; BLOCK_ROWS + 1]).unwrap();
    let (object, _) = stored(built, 0);

    assert!(decode(&object[..object.len() - 8], 0).is_err());
    assert!(decode(b"", 0).unwrap().is_empty());
    assert!(decode(b"MSPB....", 8).unwrap().is_empty());
}

#[test]
fn grams_of_another_length_are_left_out() {
    let header = serde_json::to_vec(&serde_json::json!([{
        "field": "message",
        "segment": "segment",
        "rows": BLOCK_ROWS,
        "blocks": 2,
        "groups": 1,
        "hashes": 4,
        "gram": 7,
    }]))
    .unwrap();

    let mut out = BytesMut::new();
    out.put_slice(b"MSGS");
    out.put_u32_le(header.len() as u32);
    out.put_slice(&header);
    out.put_slice(&[0; GROUP_SLICES * 8]);

    assert!(decode(&out, 0).unwrap().is_empty());
}

/// The trigram section of an older segment: one filter of ten bits per trigram for each block of
/// `blocks`, written as that version wrote it.
fn trigram_section(blocks: &[&[&str]]) -> Vec<u8> {
    let mut words = Vec::new();
    let mut sizes = Vec::new();

    for rows in blocks {
        let mut trigrams = HashSet::new();

        for row in *rows {
            trigrams.extend(row.as_bytes().windows(3).map(pack));
        }

        let mut bloom = Bloom::with_bits(trigrams.len(), 10, 7);

        for gram in trigrams {
            bloom.insert_probe(Probe::of_packed(gram));
        }

        sizes.push(bloom.words.len());
        words.extend(bloom.words);
    }

    let header = serde_json::to_vec(&serde_json::json!([{
        "field": "message",
        "segment": "old",
        "rows": BLOCK_ROWS,
        "hashes": 7,
        "words": sizes,
    }]))
    .unwrap();

    let mut out = BytesMut::new();
    out.put_slice(b"MSGB");
    out.put_u32_le(header.len() as u32);
    out.put_slice(&header);

    for word in words {
        out.put_u64_le(word);
    }

    out.to_vec()
}

#[test]
fn older_segments_keep_their_trigram_blocks() {
    let section = trigram_section(&[&["request processed"], &["request failed: needle"]]);
    let blocks = decode(&section, 0).unwrap();
    let old = &blocks["message"]["old"];

    assert_eq!(old.len(), 2);
    assert_eq!(holds(old, "needle"), [false, true]);
    assert!(old.ranges(&grams("needle")).is_empty());

    // A literal too short for the longer grams still has trigrams to probe these with.
    assert_eq!(holds(old, "eedl"), [false, true]);

    // They are not written again: the filters built here are.
    let mut out = BytesMut::new();
    encode(&blocks, &mut out).unwrap();

    assert!(out.is_empty());
}
