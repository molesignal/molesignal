// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The words of a log message, each one fixed or a place where every log says something of its
//! own.
//!
//! The same reading as the Patterns view of Logs (`web/src/routes/logs/patterns/model.ts`), so
//! one message forms the same pattern on the page and in the tool. A word that is a number, an
//! identifier, an address or a moment, or that has a digit in it, is a place that changes; the
//! words left are the pattern.

use std::sync::LazyLock;

use regex::Regex;

/// A message is read to its first line break, and no further than this many words.
const MAX_WORDS: usize = 48;

/// Shorter than this, a run of words would match far more than the pattern.
const MIN_FRAGMENT_LENGTH: usize = 4;

/// What stands in for the words after [`MAX_WORDS`].
pub(super) const ELLIPSIS: &str = "…";

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("a log pattern expression is valid")
}

static LEAD: LazyLock<Regex> = LazyLock::new(|| pattern(r#"^[(\[{"'<]+"#));
static TRAIL: LazyLock<Regex> = LazyLock::new(|| pattern(r#"[)\]}"'>,;:.!?]+$"#));
static KEY_VALUE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^([A-Za-z_][A-Za-z0-9_.-]*[=:])(.+)$"));
static URL: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^[a-z][a-z0-9+.-]*://\S+$"));
static IP: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^(?:[0-9]{1,3}\.){3}[0-9]{1,3}(?::[0-9]+)?$"));
static TIME: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r"^(?:[0-9]{4}-[0-9]{2}-[0-9]{2}(?:[T ][0-9]{2}:[0-9]{2}(?::[0-9]{2}(?:\.[0-9]+)?)?(?:Z|[+-][0-9]{2}:?[0-9]{2})?)?|[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]+)?)$",
    )
});
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    pattern(r"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
});
static HEX: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^(?:0x)?[0-9a-f]{8,}$"));

/// A number, and what it is counted in when that is written against it: a time, a size, a rate,
/// a share. Other letters after digits are not a unit: `12ae` is an identifier.
static NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r"(?i)^[-+]?[0-9][0-9,]*(?:\.[0-9]+)?(%|[nuµμm]?s|min|[hd]|[kmgtp]?i?b(?:/s)?|[kmg]|x|px|rps|qps|hz)?$",
    )
});

/// An object or a list written out, as against a word in brackets such as `[worker]`.
static DATA_START: LazyLock<Regex> = LazyLock::new(|| pattern(r#"^(?:\{["'}]|\[[{"0-9\]])"#));

/// What a place that changes from log to log holds, where it can be told.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Wild {
    Number,
    Id,
    Ip,
    Url,
    Time,
    Data,
    Any,
}

impl Wild {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Number => "number",
            Self::Id => "id",
            Self::Ip => "ip",
            Self::Url => "url",
            Self::Time => "time",
            Self::Data => "data",
            Self::Any => "any",
        }
    }
}

/// A word of a pattern: said by every log of it, or a place where each says something of its
/// own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Token {
    /// Punctuation the word carries before and after it, the same in every log.
    pub(super) lead: String,
    pub(super) trail: String,
    /// The word itself; for a place that changes, a sample of what stands there.
    pub(super) text: String,
    pub(super) wild: Option<Wild>,
    /// What follows a number without a space: the unit it is counted in.
    pub(super) unit: String,
}

impl Token {
    fn fixed(text: &str) -> Self {
        Self {
            lead: String::new(),
            trail: String::new(),
            text: text.to_string(),
            wild: None,
            unit: String::new(),
        }
    }

    /// The word as the pattern writes it: itself, or the kind of thing that stands in its place.
    pub(super) fn key(&self) -> String {
        match self.wild {
            Some(wild) => format!(
                "{}<{}>{}{}",
                self.lead,
                wild.as_str(),
                self.unit,
                self.trail
            ),
            None => format!("{}{}{}", self.lead, self.text, self.trail),
        }
    }
}

/// The words of a message, each said to be fixed or changing.
pub(super) fn pattern_tokens(message: &str) -> Vec<Token> {
    let line = message.split('\n').next().unwrap_or_default().trim();
    let words = line.split_whitespace().collect::<Vec<_>>();
    let mut tokens = Vec::new();

    for (index, word) in words.iter().enumerate() {
        if index == MAX_WORDS {
            tokens.push(Token::fixed(ELLIPSIS));
            break;
        }

        // Structured data set into a message runs to its end, and is never the same twice.
        if DATA_START.is_match(word) && (line.ends_with('}') || line.ends_with(']')) {
            tokens.push(Token {
                text: words[index..].join(" "),
                wild: Some(Wild::Data),
                ..Token::fixed("")
            });
            break;
        }

        tokens.push(pattern_token(word));
    }

    tokens
}

fn pattern_token(word: &str) -> Token {
    let lead = LEAD.find(word).map_or("", |found| found.as_str());
    let rest = &word[lead.len()..];
    let trail = TRAIL.find(rest).map_or("", |found| found.as_str());
    let core = &rest[..rest.len() - trail.len()];

    if core.is_empty() {
        return Token::fixed(word);
    }

    let wild = wild_kind(core);

    // An address or a moment has a colon of its own: it is one thing, not a name and a value.
    if let Some(kind) = wild.filter(|kind| *kind != Wild::Any) {
        return Token {
            lead: lead.to_string(),
            trail: trail.to_string(),
            text: core.to_string(),
            wild: Some(kind),
            unit: unit_for(kind, core),
        };
    }

    // `sku=SKU-48213`: the name is the pattern's, the value is the log's own.
    if let Some(pair) = KEY_VALUE.captures(core) {
        let value = strip_quotes(&pair[2]);

        if let Some(kind) = wild_kind(&value) {
            return Token {
                lead: format!("{lead}{}", &pair[1]),
                trail: trail.to_string(),
                unit: unit_for(kind, &value),
                text: value,
                wild: Some(kind),
            };
        }
    }

    Token {
        lead: lead.to_string(),
        trail: trail.to_string(),
        text: core.to_string(),
        wild,
        unit: String::new(),
    }
}

fn has_digit(text: &str) -> bool {
    text.bytes().any(|byte| byte.is_ascii_digit())
}

fn wild_kind(core: &str) -> Option<Wild> {
    if URL.is_match(core) {
        return Some(Wild::Url);
    }

    if IP.is_match(core) {
        return Some(Wild::Ip);
    }

    if TIME.is_match(core) {
        return Some(Wild::Time);
    }

    if UUID.is_match(core) || (HEX.is_match(core) && has_digit(core)) {
        return Some(Wild::Id);
    }

    if NUMBER.is_match(core) {
        return Some(Wild::Number);
    }

    // What has a digit in it is, far more often than not, a value and not a word.
    has_digit(core).then_some(Wild::Any)
}

fn unit_for(kind: Wild, core: &str) -> String {
    if kind != Wild::Number {
        return String::new();
    }

    NUMBER
        .captures(core)
        .and_then(|captures| captures.get(1))
        .map_or_else(String::new, |unit| unit.as_str().to_string())
}

fn strip_quotes(value: &str) -> String {
    let value = value.strip_prefix(['"', '\'']).unwrap_or(value);

    value.strip_suffix(['"', '\'']).unwrap_or(value).to_string()
}

/// The pattern as text, a changing place written as its kind.
pub(super) fn pattern_key(tokens: &[Token]) -> String {
    tokens.iter().map(Token::key).collect::<Vec<_>>().join(" ")
}

/// The longest stretch of the pattern with no changing place in it, as the logs write it: what a
/// query can look for to read logs of the pattern.
pub(super) fn longest_fixed_run(tokens: &[Token]) -> String {
    fn close(run: &mut Vec<String>, best: &mut String) {
        let text = run.join(" ");

        if text.chars().count() > best.chars().count() {
            *best = text;
        }

        run.clear();
    }

    let mut best = String::new();
    let mut run = Vec::new();

    for token in tokens {
        if token.wild.is_some() || token.text == ELLIPSIS {
            // What stands right before and right after a changing place is still said by every log.
            if !token.lead.is_empty() {
                run.push(token.lead.clone());
            }

            close(&mut run, &mut best);

            if !token.unit.is_empty() || !token.trail.is_empty() {
                run.push(format!("{}{}", token.unit, token.trail));
            }

            continue;
        }

        run.push(format!("{}{}{}", token.lead, token.text, token.trail));
    }

    close(&mut run, &mut best);

    let text = best.trim();

    if text.chars().count() >= MIN_FRAGMENT_LENGTH {
        text.to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_of(message: &str) -> String {
        pattern_key(&pattern_tokens(message))
    }

    #[test]
    fn the_words_stay_and_the_places_that_change_open() {
        assert_eq!(
            key_of("payment authorized for order 8f31c2 in 182ms"),
            "payment authorized for order <any> in <number>ms"
        );
        assert_eq!(key_of("request completed"), "request completed");
        assert_eq!(
            key_of("retrying (3) after 1.5s, gave up."),
            "retrying (<number>) after <number>s, gave up."
        );
    }

    #[test]
    fn an_address_a_moment_and_an_identifier_are_told_from_a_number() {
        assert_eq!(
            key_of("GET https://bank.example.com/v2/charges from 10.0.3.17:8443"),
            "GET <url> from <ip>"
        );
        assert_eq!(
            key_of("started at 2026-05-23T09:59:54.120Z took 12:00:01"),
            "started at <time> took <time>"
        );
        assert_eq!(
            key_of(
                "trace 7c1f0e0004a94b4d2e8f3a5b6c7d8e9f span 550e8400-e29b-41d4-a716-446655440000"
            ),
            "trace <id> span <id>"
        );
    }

    #[test]
    fn letters_after_a_number_are_a_unit_only_when_they_are_one() {
        assert_eq!(
            key_of("used 1.5GB of 4gb at 99% for 30s, 250ms and 3h"),
            "used <number>GB of <number>gb at <number>% for <number>s, <number>ms and <number>h"
        );
        // An identifier that begins with digits is not so many of something.
        assert_eq!(
            key_of("order 12ae and order 5ea shipped"),
            "order <any> and order <any> shipped"
        );
    }

    #[test]
    fn the_name_of_a_named_value_stays_and_the_value_opens() {
        assert_eq!(
            key_of(
                "failed to reserve stock: sku=SKU-48213 requested=4 available=1 warehouse=eu-central-1a"
            ),
            "failed to reserve stock: sku=<any> requested=<number> available=<number> warehouse=<any>"
        );
        // A value that is a word is part of what the message says.
        assert_eq!(
            key_of("cache refresh status=ok method=GET"),
            "cache refresh status=ok method=GET"
        );
    }

    #[test]
    fn data_written_out_at_the_end_is_one_place_but_a_word_in_brackets_is_not() {
        assert_eq!(
            key_of(r#"resolved feature flags {"new_checkout":true,"express_pay":false}"#),
            "resolved feature flags <data>"
        );
        assert_eq!(
            key_of("[worker] finished job [ok]"),
            "[worker] finished job [ok]"
        );
    }

    #[test]
    fn only_the_first_line_of_a_message_is_read() {
        assert_eq!(
            key_of(
                "failed to connect\n    at Pool.acquire (pool.ts:214)\n    at main (index.ts:8)"
            ),
            "failed to connect"
        );
        assert_eq!(key_of("failed to connect\r\nstack"), "failed to connect");
    }

    #[test]
    fn reading_stops_at_a_length_a_pattern_can_still_be_read_at() {
        let message = (0..80)
            .map(|index| format!("word{}", "a".repeat(index % 3)))
            .collect::<Vec<_>>()
            .join(" ");
        let tokens = pattern_tokens(&message);

        assert_eq!(tokens.len(), 49);
        assert_eq!(tokens.last().unwrap().text, ELLIPSIS);
    }

    #[test]
    fn a_quote_before_a_named_value_is_not_part_of_it() {
        // The closing quote is trailing punctuation of the word, which every log of it has.
        assert_eq!(
            key_of(r#"user="42" logged in"#),
            r#"user=<number>" logged in"#
        );
        assert_eq!(strip_quotes("\""), "");
        assert_eq!(strip_quotes("'a'"), "a");
    }

    #[test]
    fn what_a_query_can_ask_for_is_the_longest_fixed_run_as_the_logs_write_it() {
        assert_eq!(
            longest_fixed_run(&pattern_tokens(
                "payment authorized for order 8f31c2 in 182ms"
            )),
            "payment authorized for order"
        );
        assert_eq!(
            longest_fixed_run(&pattern_tokens(
                "failed to reserve stock: sku=SKU-48213 requested=4"
            )),
            "failed to reserve stock: sku="
        );
        assert_eq!(
            longest_fixed_run(&pattern_tokens("took 182ms to answer the request")),
            "ms to answer the request"
        );
        assert_eq!(longest_fixed_run(&pattern_tokens("42 7 ok")), "");
        assert_eq!(longest_fixed_run(&pattern_tokens(r#"{"a":1}"#)), "");
    }
}
