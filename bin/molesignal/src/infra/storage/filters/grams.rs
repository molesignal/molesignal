// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Byte grams of folded text: what an `ngram` field's filters hold. The whole-segment filters hold
//! trigrams, the block filters [`GRAM_BYTES`]-byte grams.
//!
//! A value matching `LIKE '%lit%'` contains `lit`. Folded one character at a time, a mapping that
//! commutes with concatenation, it contains the folded `lit`, and with it every gram of it, so a
//! filter that lacks one of those grams has no such value. `ILIKE` compares without case, which
//! agrees with the folding only where Unicode case folding does: a literal with any other cased
//! letter (é, Σ) proves nothing and gives no grams.

use super::blocks::GRAM_BYTES;

/// Three consecutive bytes of folded UTF-8 text.
pub type Gram = [u8; 3];

/// Words of a bitmap with one bit for each of the 2^24 possible trigrams: 2 MiB.
const SEEN_WORDS: usize = (1 << 24) / 64;

/// The trigrams met so far, one bit each. A text field repeats the same few trigrams row after
/// row, so the filter builder hashes a trigram only the first time this set meets it.
pub(super) struct SeenGrams {
    words: Box<[u64]>,
}

impl SeenGrams {
    pub(super) fn new() -> Self {
        Self {
            words: vec![0; SEEN_WORDS].into_boxed_slice(),
        }
    }

    /// Adds `gram`; `true` when it was not in the set yet.
    pub(super) fn insert(&mut self, gram: Gram) -> bool {
        let (word, bit) = position(gram);
        let fresh = self.words[word] & bit == 0;

        self.words[word] |= bit;
        fresh
    }
}

/// The word of the bitmap that holds `gram`'s bit, and the bit.
fn position(gram: Gram) -> (usize, u64) {
    let index = (usize::from(gram[0]) << 16) | (usize::from(gram[1]) << 8) | usize::from(gram[2]);

    (index / 64, 1_u64 << (index % 64))
}

/// `text` lower-cased one character at a time, the long s folded to `s` as case-insensitive
/// matching folds it, appended to `out`. A run of ASCII, most of a log line, is copied and
/// lower-cased at once: an ASCII character's lower case is its ASCII lower case.
fn fold_into(text: &str, out: &mut String) {
    let mut rest = text;

    while !rest.is_empty() {
        let ascii = rest
            .bytes()
            .position(|byte| !byte.is_ascii())
            .unwrap_or(rest.len());

        let start = out.len();
        out.push_str(&rest[..ascii]);
        out[start..].make_ascii_lowercase();

        let mut characters = rest[ascii..].chars();

        match characters.next() {
            Some('ſ') => out.push('s'),
            Some(other) => out.extend(other.to_lowercase()),
            None => {}
        }

        rest = characters.as_str();
    }
}

/// The folded `value`, held in `buffer`, which is reused between values.
pub(super) fn fold<'a>(value: &str, buffer: &'a mut String) -> &'a [u8] {
    buffer.clear();
    fold_into(value, buffer);
    buffer.as_bytes()
}

/// Hands every trigram of the folded `value` to `visit`; `buffer` is reused between values.
#[cfg(test)]
pub(super) fn value_grams(value: &str, buffer: &mut String, visit: &mut dyn FnMut(Gram)) {
    for window in fold(value, buffer).windows(3) {
        visit([window[0], window[1], window[2]]);
    }
}

/// Up to eight bytes as one number, the first byte highest: a gram as filters probe it.
pub(super) fn pack(gram: &[u8]) -> u64 {
    gram.iter()
        .fold(0, |packed, byte| (packed << 8) | u64::from(*byte))
}

/// The folded `literal`, or `None` when case-insensitive matching might fold one of its letters
/// differently.
fn folded_literal(literal: &str, case_insensitive: bool) -> Option<String> {
    if case_insensitive && !literal.chars().all(folds_plainly) {
        return None;
    }

    let mut folded = String::new();
    fold_into(literal, &mut folded);

    Some(folded)
}

fn trigrams(folded: &[u8]) -> impl Iterator<Item = Gram> + '_ {
    folded
        .windows(3)
        .map(|window| [window[0], window[1], window[2]])
}

/// The trigrams every value matching a `LIKE` (or, with `case_insensitive`, `ILIKE`) literal
/// holds, or `None` when they prove nothing: the folded literal is shorter than three bytes, or
/// case-insensitive matching might fold one of its letters differently.
pub fn literal_grams(literal: &str, case_insensitive: bool) -> Option<Vec<Gram>> {
    let folded = folded_literal(literal, case_insensitive)?;
    let grams = trigrams(folded.as_bytes()).collect::<Vec<_>>();

    (!grams.is_empty()).then_some(grams)
}

/// The grams every value matching a `LIKE` / `ILIKE` condition holds, from the literals of its
/// patterns: trigrams for the whole-segment filters, and the longer grams of the block filters. A
/// literal that proves nothing (see [`literal_grams`]) adds none, and one shorter than a block
/// gram adds trigrams only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LiteralGrams {
    trigrams: Vec<Gram>,
    /// Block grams, packed.
    blocks: Vec<u64>,
}

impl LiteralGrams {
    pub fn add(&mut self, literal: &str, case_insensitive: bool) {
        let Some(folded) = folded_literal(literal, case_insensitive) else {
            return;
        };

        self.trigrams.extend(trigrams(folded.as_bytes()));
        self.blocks
            .extend(folded.as_bytes().windows(GRAM_BYTES).map(pack));
    }

    pub fn trigrams(&self) -> &[Gram] {
        &self.trigrams
    }

    pub(super) fn block_grams(&self) -> &[u64] {
        &self.blocks
    }

    /// No literal gave a trigram, so no filter can rule the condition out.
    pub fn is_empty(&self) -> bool {
        self.trigrams.is_empty()
    }
}

/// Whether every character case-insensitively equal to `character` folds as it does: ASCII
/// letters (their only non-ASCII equals, the Kelvin sign and the long s, fold to them), and
/// characters without case.
fn folds_plainly(character: char) -> bool {
    character.is_ascii() || !(character.is_lowercase() || character.is_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grams_of(value: &str) -> Vec<Gram> {
        let mut out = Vec::new();
        value_grams(value, &mut String::new(), &mut |gram| out.push(gram));
        out
    }

    #[test]
    fn a_matching_value_holds_every_trigram_of_the_literal() {
        let value = grams_of("GET /API/v1/Orders?id=7");

        for literal in ["/api/v1/", "Orders", "id=7"] {
            let wanted = literal_grams(literal, true).unwrap();
            assert!(wanted.iter().all(|gram| value.contains(gram)), "{literal}");
        }

        assert!(
            !literal_grams("/api/v2/", true)
                .unwrap()
                .iter()
                .all(|gram| value.contains(gram))
        );
    }

    #[test]
    fn folding_runs_of_ascii_at_once_folds_as_each_character_alone() {
        let one_by_one = |text: &str| {
            text.chars()
                .flat_map(|character| match character {
                    'ſ' => "s".chars().collect::<Vec<_>>(),
                    other => other.to_lowercase().collect(),
                })
                .collect::<String>()
        };

        for text in [
            "",
            "GET /API/v1/Orders?ID=7",
            "request completed code=E000 调用完成 ip=10.0.0.1 USER=User-0157165",
            "调用完成",
            "ſtatus \u{212A}ELVIN İstanbul ΣΑΣ Straße",
            "trailing é",
            "é leading",
        ] {
            assert_eq!(
                fold(text, &mut String::new()),
                one_by_one(text).as_bytes(),
                "{text}"
            );
        }
    }

    #[test]
    fn cjk_literals_give_byte_trigrams() {
        let value = grams_of("用户登录失败");

        let wanted = literal_grams("登", true).unwrap();
        assert!(wanted.iter().all(|gram| value.contains(gram)));
        assert!(literal_grams("登录", true).unwrap().len() > 1);
    }

    #[test]
    fn a_literal_gives_block_grams_only_when_it_is_long_enough() {
        let mut grams = LiteralGrams::default();
        grams.add("Abcd", true);

        assert_eq!(grams.trigrams(), [*b"abc", *b"bcd"]);
        assert!(grams.block_grams().is_empty());

        grams.add("needle", false);

        assert_eq!(grams.trigrams().len(), 6);
        assert_eq!(grams.block_grams(), [pack(b"needl"), pack(b"eedle")]);

        let mut unfolded = LiteralGrams::default();
        unfolded.add("Ωmega", true);

        assert!(unfolded.is_empty());
        assert!(unfolded.block_grams().is_empty());
    }

    #[test]
    fn short_or_unevenly_folded_literals_prove_nothing() {
        assert_eq!(literal_grams("ab", false), None);
        assert_eq!(literal_grams("Ωmega", true), None);
        assert!(literal_grams("Ωmega", false).is_some());
    }

    #[test]
    fn a_trigram_is_fresh_only_the_first_time() {
        let mut seen = SeenGrams::new();

        assert!(seen.insert(*b"abc"));
        assert!(!seen.insert(*b"abc"));
        assert!(seen.insert(*b"abd"));
        assert!(seen.insert([0, 0, 0]));
        assert!(seen.insert([255, 255, 255]));
        assert!(!seen.insert([255, 255, 255]));
    }

    #[test]
    fn the_long_s_and_the_kelvin_sign_fold_to_ascii() {
        let value = grams_of("ſtatus \u{212A}ey");

        for literal in ["status", "key"] {
            let wanted = literal_grams(literal, true).unwrap();
            assert!(wanted.iter().all(|gram| value.contains(gram)), "{literal}");
        }
    }
}
