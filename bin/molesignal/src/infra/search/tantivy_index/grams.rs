// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 长词里的三元组。分词丢掉 40 字节及以上的词（见 [`super::text`]），`MATCH(field, 'term')`
//! 执行时的 `ILIKE '%term%'` 却可能正落在这样的词里（一串 64 位的十六进制摘要里的 `cafe`）。归档
//! 为每个分词字段记下它的长词里出现过哪些三元组（小写 ASCII 字母与数字）：查询词的三元组不全在
//! 里面时，这个字段的长词里一定没有它。

use base64::{Engine, engine::general_purpose::STANDARD};

use super::text::{LONG_TOKEN_BYTES, plain_runs};

/// 三元组的字母表：a–z 与 0–9。
const SYMBOLS: usize = 36;
const TRIGRAMS: usize = SYMBOLS * SYMBOLS * SYMBOLS;
const WORDS: usize = TRIGRAMS.div_ceil(64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LongGrams {
    /// 见过长词。没有三元组的长词也算：不到三个字符的查询词可能落在里面。
    seen: bool,
    bits: Vec<u64>,
}

impl Default for LongGrams {
    fn default() -> Self {
        Self {
            seen: false,
            bits: vec![0; WORDS],
        }
    }
}

impl LongGrams {
    /// 记下 `value` 分词时丢掉的长词。
    pub(super) fn add_value(&mut self, value: &str) {
        if value.len() < LONG_TOKEN_BYTES {
            return;
        }

        // 只有不含中日韩文字的字母数字连续段会长：中日韩文字切成了二元组。
        for token in plain_runs(value).filter(|token| token.len() >= LONG_TOKEN_BYTES) {
            self.seen = true;
            self.add_token(token);
        }
    }

    /// 词小写后（与索引的小写一致：逐字符 `to_lowercase`）的每个三元组。
    fn add_token(&mut self, token: &str) {
        let mut window = [None; 3];

        for symbol in token.chars().flat_map(char::to_lowercase).map(symbol) {
            window = [window[1], window[2], symbol];

            if let [Some(a), Some(b), Some(c)] = window {
                let gram = trigram(a, b, c);
                self.bits[gram / 64] |= 1 << (gram % 64);
            }
        }
    }

    pub(super) fn seen(&self) -> bool {
        self.seen
    }

    /// 长词里可能有 `term`：`term` 的三元组都出现过。不到三个字符或含字母数字以外字符的词总是
    /// 可能。
    pub(super) fn may_contain(&self, term: &str) -> bool {
        if !self.seen {
            return false;
        }

        let Some(symbols) = term.chars().map(symbol).collect::<Option<Vec<_>>>() else {
            return true;
        };

        symbols.windows(3).all(|window| {
            let gram = trigram(window[0], window[1], window[2]);
            self.bits[gram / 64] & (1 << (gram % 64)) != 0
        })
    }

    pub(super) fn encode(&self) -> String {
        let bytes = self
            .bits
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect::<Vec<_>>();

        STANDARD.encode(bytes)
    }

    /// 读回 [`Self::encode`] 的结果；能读出来就说明这个字段见过长词。
    pub(super) fn decode(text: &str) -> Option<Self> {
        let bytes = STANDARD.decode(text).ok()?;

        if bytes.len() != WORDS * 8 {
            return None;
        }

        Some(Self {
            seen: true,
            bits: bytes
                .chunks_exact(8)
                .map(|word| u64::from_le_bytes(word.try_into().expect("eight bytes")))
                .collect(),
        })
    }
}

fn symbol(c: char) -> Option<usize> {
    match c {
        'a'..='z' => Some(c as usize - 'a' as usize),
        '0'..='9' => Some(26 + c as usize - '0' as usize),
        _ => None,
    }
}

fn trigram(a: usize, b: usize, c: usize) -> usize {
    (a * SYMBOLS + b) * SYMBOLS + c
}

#[cfg(test)]
mod tests {
    use tantivy::tokenizer::{TokenStream, Tokenizer};

    use super::{super::text::TextTokenizer, *};

    const DIGEST: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    #[test]
    fn long_tokens_keep_their_trigrams_and_short_ones_none() {
        let mut grams = LongGrams::default();
        grams.add_value(&format!("sha256={DIGEST} by Alice"));

        assert!(grams.seen());
        assert!(grams.may_contain("d081"));
        assert!(grams.may_contain("0c55ad"));
        assert!(
            !grams.may_contain("alice"),
            "a short token is in the dictionary"
        );
        assert!(!grams.may_contain("cafe"));
        // Too short to tell from trigrams.
        assert!(grams.may_contain("zq"));
    }

    #[test]
    fn values_without_long_tokens_record_nothing() {
        let mut grams = LongGrams::default();
        grams.add_value("GET /api/p3 200 4.2ms request_id=7f3a2b1c9d8e7f6a");

        assert!(!grams.seen());
        assert!(!grams.may_contain("7f3a"));
    }

    #[test]
    fn upper_case_long_tokens_are_matched_lower_case() {
        let mut grams = LongGrams::default();
        grams.add_value(&DIGEST.to_ascii_uppercase());

        assert!(grams.may_contain("d081"));
    }

    #[test]
    fn the_encoding_reads_back() {
        let mut grams = LongGrams::default();
        grams.add_value(DIGEST);

        assert_eq!(LongGrams::decode(&grams.encode()), Some(grams));
        assert_eq!(LongGrams::decode("bm9wZQ=="), None);
    }

    /// The long tokens are the ones the analyzer drops: runs of 40 bytes or more, cut where the
    /// tokenizer cuts, CJK text included.
    #[test]
    fn tokens_are_cut_where_the_tokenizer_cuts() {
        let value = format!(
            "{DIGEST}\u{e9}x-{}_y {}订单 é{}",
            "a".repeat(39),
            "B".repeat(40),
            "ç".repeat(25)
        );

        let mut tokenizer = TextTokenizer::default();
        let mut stream = tokenizer.token_stream(&value);
        let mut expected = Vec::new();

        while stream.advance() {
            if stream.token().text.len() >= LONG_TOKEN_BYTES {
                expected.push(stream.token().text.clone());
            }
        }

        let actual = plain_runs(&value)
            .filter(|token| token.len() >= LONG_TOKEN_BYTES)
            .map(str::to_owned)
            .collect::<Vec<_>>();

        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 3);
    }
}
