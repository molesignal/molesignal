// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 分词字段的分词：字母数字的连续段是词，其中的中日韩文字另切成相邻两字的二元组。
//!
//! `MATCH` 按子串执行，裁剪要保证“索引说没有就真的没有”。一个只由 ASCII 字母数字组成的子串
//! 出现在值里，就整个落在某个词里：词是不含中日韩文字的字母数字连续段。一段中日韩文字出现在值里，
//! 它的每个二元组都是值的词；单个字落在某个二元组里，或者自己就是一个单字词（前后都不是中日韩
//! 文字时）。所以文件里有没有这样的子串都能由词典回答，见 [`super::IndexHandle::may_contain`]。
//!
//! Tantivy 的默认分词把一整句中文连成一个词，超过 40 字节就丢掉，中文的子串因此查不了；词典分词
//! （jieba 一类）会漏掉落在长词中间的子串（“人民”在“中华人民共和国”里），不能用来裁剪。

use std::{iter::Peekable, str::CharIndices};

use tantivy::tokenizer::{
    LowerCaser, RemoveLongFilter, TextAnalyzer, Token, TokenStream, Tokenizer,
};

/// 分词字段注册的分词器名。
pub const TEXT_TOKENIZER: &str = "molesignal_text";

/// 丢掉的长词的最小字节数，与 Tantivy 的默认分词一致（`RemoveLongFilter::limit(40)` 只留更短
/// 的词）。
pub(super) const LONG_TOKEN_BYTES: usize = 40;

/// 写入与打开归档时注册的分析器：切词、丢掉长词、逐字符小写化。
pub(super) fn text_analyzer() -> TextAnalyzer {
    TextAnalyzer::builder(TextTokenizer::default())
        .filter(RemoveLongFilter::limit(LONG_TOKEN_BYTES))
        .filter(LowerCaser)
        .build()
}

/// 中日韩文字：汉字、假名与谚文里的字母（标点与符号不算）。
pub fn is_cjk(character: char) -> bool {
    matches!(
        character as u32,
        0x1100..=0x11FF
            | 0x3040..=0x30FF
            | 0x3130..=0x318F
            | 0x31F0..=0x31FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2FA1F
            | 0x30000..=0x323AF
    ) && character.is_alphanumeric()
}

/// 值里不含中日韩文字的字母数字连续段：分词出的普通词（小写化之前）。
pub(super) fn plain_runs(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(|character: char| !character.is_alphanumeric() || is_cjk(character))
        .filter(|run| !run.is_empty())
}

/// 子串里分词索引能查的部分：ASCII 字母数字连续段，与中日韩文字的连续段。别的字符（标点、
/// 别的文字）不查：大小写折叠在它们上面与索引的小写化未必一致。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Piece {
    Word(String),
    Cjk(Vec<char>),
}

/// 小写的子串 `term` 里能查的部分，按出现的顺序。
pub(super) fn pieces(term: &str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut word = String::new();
    let mut cjk = Vec::new();

    for character in term.chars() {
        if character.is_ascii_alphanumeric() {
            word.push(character);
        } else if !word.is_empty() {
            pieces.push(Piece::Word(std::mem::take(&mut word)));
        }

        if is_cjk(character) {
            cjk.push(character);
        } else if !cjk.is_empty() {
            pieces.push(Piece::Cjk(std::mem::take(&mut cjk)));
        }
    }

    if !word.is_empty() {
        pieces.push(Piece::Word(word));
    }

    if !cjk.is_empty() {
        pieces.push(Piece::Cjk(cjk));
    }

    pieces
}

#[derive(Clone, Default)]
pub(super) struct TextTokenizer {
    token: Token,
}

pub(super) struct TextTokenStream<'a> {
    text: &'a str,
    chars: Peekable<CharIndices<'a>>,
    token: &'a mut Token,
    /// 上一个中日韩字的起点与字节数：下一个二元组从它开始。
    previous: Option<(usize, usize)>,
    /// 这一段中日韩文字已经出过二元组；没出过就是单字的段，出单字词。
    paired: bool,
}

impl Tokenizer for TextTokenizer {
    type TokenStream<'a> = TextTokenStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> TextTokenStream<'a> {
        self.token.reset();

        TextTokenStream {
            text,
            chars: text.char_indices().peekable(),
            token: &mut self.token,
            previous: None,
            paired: false,
        }
    }
}

impl TextTokenStream<'_> {
    fn emit(&mut self, from: usize, to: usize) {
        self.token.offset_from = from;
        self.token.offset_to = to;
        self.token.text.push_str(&self.text[from..to]);
    }
}

impl TokenStream for TextTokenStream<'_> {
    fn advance(&mut self) -> bool {
        self.token.text.clear();
        self.token.position = self.token.position.wrapping_add(1);

        loop {
            if let Some((from, length)) = self.previous {
                match self.chars.peek().copied() {
                    Some((offset, character)) if is_cjk(character) => {
                        self.chars.next();
                        self.previous = Some((offset, character.len_utf8()));
                        self.paired = true;
                        self.emit(from, offset + character.len_utf8());
                        return true;
                    }
                    _ => {
                        self.previous = None;

                        if !self.paired {
                            self.emit(from, from + length);
                            return true;
                        }
                    }
                }
            }

            let Some((offset, character)) = self.chars.next() else {
                return false;
            };

            if is_cjk(character) {
                self.previous = Some((offset, character.len_utf8()));
                self.paired = false;
            } else if character.is_alphanumeric() {
                let mut end = offset + character.len_utf8();

                while let Some(&(next, following)) = self.chars.peek() {
                    if !following.is_alphanumeric() || is_cjk(following) {
                        break;
                    }

                    end = next + following.len_utf8();
                    self.chars.next();
                }

                self.emit(offset, end);
                return true;
            }
        }
    }

    fn token(&self) -> &Token {
        self.token
    }

    fn token_mut(&mut self) -> &mut Token {
        self.token
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(text: &str) -> Vec<String> {
        let mut analyzer = text_analyzer();
        let mut stream = analyzer.token_stream(text);
        let mut out = Vec::new();

        while stream.advance() {
            out.push(stream.token().text.clone());
        }

        out
    }

    #[test]
    fn words_are_alphanumeric_runs_and_cjk_runs_become_bigrams() {
        assert_eq!(
            tokens("订单order123创建失败, Timeout=30s 中"),
            [
                "订单", "order123", "创建", "建失", "失败", "timeout", "30s", "中"
            ]
        );
        assert_eq!(
            tokens("ログイン失敗 사용자"),
            ["ログ", "グイ", "イン", "ン失", "失敗", "사용", "용자"]
        );
    }

    #[test]
    fn long_plain_words_are_dropped_and_bigrams_never_are() {
        let long = "a".repeat(LONG_TOKEN_BYTES);
        let sentence = "用户登录失败请检查网络连接后重新尝试操作";

        assert_eq!(tokens(&format!("{long} ok")), ["ok"]);
        assert_eq!(tokens(sentence).len(), sentence.chars().count() - 1);
    }

    #[test]
    fn punctuation_in_cjk_ranges_is_a_separator() {
        assert_eq!(tokens("登录、失败。"), ["登录", "失败"]);
        assert!(!is_cjk('、'));
        assert!(is_cjk('汉'));
    }

    #[test]
    fn a_term_splits_into_the_pieces_the_index_can_answer() {
        assert_eq!(
            pieces("my-api 登录失败café"),
            [
                Piece::Word("my".into()),
                Piece::Word("api".into()),
                Piece::Cjk("登录失败".chars().collect()),
                Piece::Word("caf".into()),
            ]
        );
        assert!(pieces("--").is_empty());
    }

    #[test]
    fn plain_runs_are_the_words_before_lower_casing() {
        assert_eq!(
            plain_runs("订单Order123创建, x").collect::<Vec<_>>(),
            ["Order123", "x"]
        );
    }
}
