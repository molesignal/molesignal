// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Byte trigrams of folded text: what an `ngram` field's filter holds.
//!
//! A value matching `LIKE '%lit%'` contains `lit`. Folded one character at a time, a mapping that
//! commutes with concatenation, it contains the folded `lit`, and with it every trigram of it, so a
//! file whose filter lacks one of those trigrams has no such value. `ILIKE` compares without case,
//! which agrees with the folding only where Unicode case folding does: a literal with any other
//! cased letter (é, Σ) proves nothing and gives no trigrams.

/// Three consecutive bytes of folded UTF-8 text.
pub type Gram = [u8; 3];

/// `text` lower-cased one character at a time, the long s folded to `s` as case-insensitive
/// matching folds it, appended to `out`.
fn fold_into(text: &str, out: &mut String) {
    for character in text.chars() {
        match character {
            'ſ' => out.push('s'),
            other => out.extend(other.to_lowercase()),
        }
    }
}

/// Hands every trigram of the folded `value` to `visit`; `buffer` is reused between values.
pub(super) fn value_grams(value: &str, buffer: &mut String, visit: &mut dyn FnMut(Gram)) {
    buffer.clear();
    fold_into(value, buffer);

    for window in buffer.as_bytes().windows(3) {
        visit([window[0], window[1], window[2]]);
    }
}

/// The trigrams every value matching a `LIKE` (or, with `case_insensitive`, `ILIKE`) literal
/// holds, or `None` when they prove nothing: the folded literal is shorter than three bytes, or
/// case-insensitive matching might fold one of its letters differently.
pub fn literal_grams(literal: &str, case_insensitive: bool) -> Option<Vec<Gram>> {
    if case_insensitive && !literal.chars().all(folds_plainly) {
        return None;
    }

    let mut folded = String::new();
    fold_into(literal, &mut folded);

    let grams = folded
        .as_bytes()
        .windows(3)
        .map(|window| [window[0], window[1], window[2]])
        .collect::<Vec<_>>();

    (!grams.is_empty()).then_some(grams)
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
    fn cjk_literals_give_byte_trigrams() {
        let value = grams_of("用户登录失败");

        let wanted = literal_grams("登", true).unwrap();
        assert!(wanted.iter().all(|gram| value.contains(gram)));
        assert!(literal_grams("登录", true).unwrap().len() > 1);
    }

    #[test]
    fn short_or_unevenly_folded_literals_prove_nothing() {
        assert_eq!(literal_grams("ab", false), None);
        assert_eq!(literal_grams("Ωmega", true), None);
        assert!(literal_grams("Ωmega", false).is_some());
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
