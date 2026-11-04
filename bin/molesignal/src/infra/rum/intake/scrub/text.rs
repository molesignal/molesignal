// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Sensitive things in free text: the message of an error, a stack, the name of a button, the
//! text of a request. Each kind has the shapes it is known by; where a shape is easily
//! mistaken for a number that is not sensitive (a card, an identity number), the checks the
//! number itself has (its check digit, the numbers a card begins with) must hold too.

use std::{borrow::Cow, sync::LazyLock};

use regex::{Captures, Regex};

use crate::domain::rum::ScrubKind;

const SECRET: &str = "[secret]";
const EMAIL: &str = "[email]";
const PHONE: &str = "[phone]";
const CARD: &str = "[card]";
const ID: &str = "[id]";

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("a scrubbing pattern is a valid expression")
}

static PRIVATE_KEY: LazyLock<Regex> = LazyLock::new(|| {
    pattern(r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----")
});

static JWT: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"\beyJ[A-Za-z0-9_-]{5,}\.eyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]*"));

/// The tokens and keys of the services that are written with a fixed prefix.
static KNOWN_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b|\bgh[pousr]_[A-Za-z0-9]{30,}|\bgithub_pat_[A-Za-z0-9_]{40,}|\bglpat-[A-Za-z0-9_-]{20,}|\bxox[abprs]-[A-Za-z0-9-]{10,}|\b[sr]k_(?:live|test)_[A-Za-z0-9]{16,}|\bsk-[A-Za-z0-9_-]{20,}|\bAIza[0-9A-Za-z_-]{35}\b|\bnpm_[A-Za-z0-9]{36}\b",
    )
});

static AUTHORIZATION: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]{12,}"));

/// A word that says the next thing is a secret, and what follows it: `password=hunter2`,
/// `"api_key": "abc"`, `Authorization: abc`.
static ASSIGNED: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r#"(?i)\b(password|passwd|pwd|passphrase|secret|client[_-]?secret|api[_-]?key|apikey|(?:access|refresh|id|auth)[_-]?token|authorization|private[_-]?key)("?\s*[:=]\s*)("?)([^\s"'&,;}\[\]][^\s"'&,;}\]]{2,})"#,
    )
});

static EMAIL_ADDRESS: LazyLock<Regex> = LazyLock::new(|| {
    pattern(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}\b")
});

static INTERNATIONAL_PHONE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"\+\d{1,3}(?:[\s.-]?\(?\d{1,4}\)?){2,5}"));
static CHINESE_MOBILE: LazyLock<Regex> = LazyLock::new(|| pattern(r"\b1[3-9]\d{9}\b"));
static NORTH_AMERICAN_PHONE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?:\([2-9]\d{2}\)|\b[2-9]\d{2})[\s.-]?\d{3}[\s.-]\d{4}\b"));

static CARD_NUMBER: LazyLock<Regex> = LazyLock::new(|| pattern(r"\b[2-6](?:[ -]?\d){12,18}\b"));

static CHINESE_ID: LazyLock<Regex> = LazyLock::new(|| {
    pattern(r"\b[1-9]\d{5}(?:19|20)\d{2}(?:0[1-9]|1[0-2])(?:0[1-9]|[12]\d|3[01])\d{3}[\dXx]\b")
});
static SOCIAL_SECURITY: LazyLock<Regex> = LazyLock::new(|| pattern(r"\b\d{3}-\d{2}-\d{4}\b"));

/// The digits of a number written with spaces or dashes.
fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

fn luhn(digits: &str) -> bool {
    let sum = digits
        .bytes()
        .rev()
        .enumerate()
        .map(|(place, byte)| {
            let digit = u32::from(byte - b'0');

            if place % 2 == 1 {
                let doubled = digit * 2;

                if doubled > 9 { doubled - 9 } else { doubled }
            } else {
                digit
            }
        })
        .sum::<u32>();

    sum % 10 == 0
}

/// The numbers the cards of the networks begin with.
fn card_prefix(digits: &str) -> bool {
    let first = |count: usize| digits[..count].parse::<u32>().unwrap_or(0);

    digits.starts_with('4')
        || (51..=55).contains(&first(2))
        || (2221..=2720).contains(&first(4))
        || matches!(
            first(2),
            34 | 37 | 35 | 36 | 38 | 62 | 65 | 50 | 56..=58 | 67
        )
        || (300..=305).contains(&first(3))
        || first(4) == 6011
        || (644..=649).contains(&first(3))
}

/// Whether the digits of a number are those of a card: a length cards have, a number a card
/// begins with, and the check digit.
fn is_card(digits: &str) -> bool {
    (13..=19).contains(&digits.len()) && card_prefix(digits) && luhn(digits)
}

/// The check character of a Chinese resident identity number (ISO 7064, mod 11-2).
fn chinese_id_checks(id: &str) -> bool {
    const WEIGHTS: [u32; 17] = [7, 9, 10, 5, 8, 4, 2, 1, 6, 3, 7, 9, 10, 5, 8, 4, 2];
    const CHECKS: &[u8; 11] = b"10X98765432";

    let sum = id
        .bytes()
        .take(17)
        .zip(WEIGHTS)
        .map(|(byte, weight)| u32::from(byte - b'0') * weight)
        .sum::<u32>();

    id.as_bytes()
        .get(17)
        .is_some_and(|last| last.to_ascii_uppercase() == CHECKS[(sum % 11) as usize])
}

/// A United States social security number: no area of 000, 666 or 9xx, no group of 00 and no
/// serial of 0000.
fn is_social_security(number: &str) -> bool {
    let mut parts = number.split('-');
    let (area, group, serial) = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    );

    area != "000" && area != "666" && !area.starts_with('9') && group != "00" && serial != "0000"
}

fn replaced<'a>(text: Cow<'a, str>, pattern: &Regex, with: &str) -> Cow<'a, str> {
    if !pattern.is_match(&text) {
        return text;
    }

    Cow::Owned(pattern.replace_all(&text, with).into_owned())
}

fn replaced_with<'a>(
    text: Cow<'a, str>,
    pattern: &Regex,
    replacement: impl Fn(&Captures<'_>) -> String,
) -> Cow<'a, str> {
    if !pattern.is_match(&text) {
        return text;
    }

    Cow::Owned(pattern.replace_all(&text, replacement).into_owned())
}

fn secrets(text: Cow<'_, str>) -> Cow<'_, str> {
    let mut text = text;

    if text.contains("-----BEGIN") {
        text = replaced(text, &PRIVATE_KEY, SECRET);
    }

    if text.contains("eyJ") {
        text = replaced(text, &JWT, SECRET);
    }

    text = replaced(text, &KNOWN_TOKEN, SECRET);

    if text.len() > 12 {
        text = replaced_with(text, &AUTHORIZATION, |found| {
            format!("{} {SECRET}", &found[1])
        });
    }

    if text.contains([':', '=']) {
        text = replaced_with(text, &ASSIGNED, |found| {
            format!("{}{}{}{SECRET}", &found[1], &found[2], &found[3])
        });
    }

    text
}

fn national_ids(text: Cow<'_, str>) -> Cow<'_, str> {
    if !text.bytes().any(|byte| byte.is_ascii_digit()) {
        return text;
    }

    let text = replaced_with(text, &CHINESE_ID, |found| {
        if chinese_id_checks(&found[0]) {
            ID.to_string()
        } else {
            found[0].to_string()
        }
    });

    replaced_with(text, &SOCIAL_SECURITY, |found| {
        if is_social_security(&found[0]) {
            ID.to_string()
        } else {
            found[0].to_string()
        }
    })
}

fn cards(text: Cow<'_, str>) -> Cow<'_, str> {
    if text.bytes().filter(u8::is_ascii_digit).count() < 13 {
        return text;
    }

    replaced_with(text, &CARD_NUMBER, |found| {
        if is_card(&digits(&found[0])) {
            CARD.to_string()
        } else {
            found[0].to_string()
        }
    })
}

fn phones(text: Cow<'_, str>) -> Cow<'_, str> {
    if text.bytes().filter(u8::is_ascii_digit).count() < 8 {
        return text;
    }

    let text = replaced_with(text, &INTERNATIONAL_PHONE, |found| {
        if (8..=15).contains(&digits(&found[0]).len()) {
            PHONE.to_string()
        } else {
            found[0].to_string()
        }
    });

    let text = replaced(text, &CHINESE_MOBILE, PHONE);

    replaced(text, &NORTH_AMERICAN_PHONE, PHONE)
}

/// The text with the kinds asked for removed, or `None` where it holds none of them.
pub fn scrub(kinds: &[ScrubKind], text: &str) -> Option<String> {
    let mut found = Cow::Borrowed(text);

    if kinds.contains(&ScrubKind::Secrets) {
        found = secrets(found);
    }

    if kinds.contains(&ScrubKind::Emails) && found.contains('@') {
        found = replaced(found, &EMAIL_ADDRESS, EMAIL);
    }

    if kinds.contains(&ScrubKind::NationalIds) {
        found = national_ids(found);
    }

    if kinds.contains(&ScrubKind::Cards) {
        found = cards(found);
    }

    if kinds.contains(&ScrubKind::Phones) {
        found = phones(found);
    }

    // A shape that failed its checks was matched and written back as it was: that is no change.
    (found != text).then(|| found.into_owned())
}
