// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `MATCH`、`MATCH_TEXT` 与 `LIKE` 的字面量按 `ILIKE '%term%'` 执行：值里任何位置出现这个子串
//! 都算，`panic` 也命中 `panicked`。分词字段的词是不含中日韩文字的字母数字连续段，与中日韩文字的
//! 二元组，小写化后进词典（见 [`super::text`]）。子串里的一段 ASCII 字母数字出现在值里，就整个
//! 落在某个词里：文件里有它，当且仅当词典里有哪个词包含它，或者没进词典的长词里有它（见
//! [`super::grams`]）。一段中日韩文字出现在值里，它的二元组就都在词典里。子串出现，它的每一段都
//! 出现；有一段一定没有，子串就一定没有。

use std::collections::BTreeSet;

use anyhow::{Result, anyhow};
use tantivy::{
    Searcher,
    query::{BooleanQuery, EmptyQuery, Query},
    schema::{Field, FieldType as TantivyFieldType},
};

use super::{
    IndexHandle, LONG_GRAMS_VERSION, TEXT_TOKENIZER,
    grams::LongGrams,
    long_grams_property, term_query,
    text::{Piece, pieces},
};

/// 词典里的词超过这么多时不逐个查：文件照常扫描。
const MAX_SCANNED_TERMS: usize = 1_000_000;

/// 包含子串的词超过这么多时不按它们数行：文件照常扫描。
const MAX_CONTAINING_TERMS: usize = 4096;

/// 分词字段的分词方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tokenizer {
    /// Tantivy 的默认分词（格式 4 之前）：中日韩文字与字母数字连成一个词，查不了。
    Default,
    /// [`TEXT_TOKENIZER`]：中日韩文字切成二元组。
    Text,
}

impl IndexHandle {
    /// `field` 的值里可能出现子串 `term`。`Some(false)` 时一定没有：`term` 有一段在词典里与长词
    /// 的三元组里都找不到。`term` 没有分词索引能查的部分（只有标点、别的文字，或旧归档里的中日韩
    /// 文字）、归档早于格式 3（不知道丢掉了哪些长词）、字段不是分词字段时为 `None`。调用方须在
    /// blocking 线程上调用，见 [`Self::count_term`]。
    pub fn may_contain(&self, field: &str, term: &str) -> Result<Option<bool>> {
        let Some((target, tokenizer)) = self.substring_field(field) else {
            return Ok(None);
        };

        let lowered = term
            .chars()
            .flat_map(char::to_lowercase)
            .collect::<String>();
        let searcher = self.searcher()?;
        let mut answered = false;

        for piece in pieces(&lowered) {
            let found = match piece {
                Piece::Word(word) => self.word_may_occur(&searcher, target, field, &word)?,
                Piece::Cjk(text) if tokenizer == Tokenizer::Text => {
                    cjk_may_occur(&searcher, target, &text)?
                }
                Piece::Cjk(_) => None,
            };

            match found {
                Some(false) => return Ok(Some(false)),
                Some(true) => answered = true,
                None => {}
            }
        }

        Ok(answered.then_some(true))
    }

    /// 一段 ASCII 字母数字 `word` 可能出现：它本身是词，词典里有包含它的词，或长词里可能有它。
    /// 词典太大不逐个查时为 `None`。
    fn word_may_occur(
        &self,
        searcher: &Searcher,
        target: Field,
        field: &str,
        word: &str,
    ) -> Result<Option<bool>> {
        // 最常见的情形：这个词本身就在词典里。
        let whole = term_query(target, word)
            .count(searcher)
            .map_err(|e| anyhow!("tantivy count: {e}"))?;

        if whole > 0 {
            return Ok(Some(true));
        }

        let mut found = false;

        let scanned = scan_containing(searcher, target, word.as_bytes(), &mut |_| {
            found = true;
            false
        })?;

        if !scanned {
            return Ok(None);
        }

        Ok(Some(found || self.long_tokens_may_contain(field, word)))
    }

    /// 值里出现子串 `term` 的文档：词典里包含它的词的并集。没有这样的词时一个文档也不命中。
    /// 只答只由小写 ASCII 字母数字组成的 `term`：这样的子串整个落在一个词里，包含它的词的文档
    /// 正好是有它的行。长词里可能有它（长词不进词典，数不出来）、包含它的词超过
    /// [`MAX_CONTAINING_TERMS`] 个，以及字段不是分词字段时为 `None`。调用方须在 blocking 线程上
    /// 调用。
    pub fn contains_query(
        &self,
        searcher: &Searcher,
        field: &str,
        term: &str,
    ) -> Result<Option<Box<dyn Query>>> {
        let plain = !term.is_empty()
            && term
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());

        let Some((target, _)) = self.substring_field(field).filter(|_| plain) else {
            return Ok(None);
        };

        if self.long_tokens_may_contain(field, term) {
            return Ok(None);
        }

        let mut terms = BTreeSet::new();
        let mut too_many = false;

        let scanned = scan_containing(searcher, target, term.as_bytes(), &mut |key| {
            if terms.len() == MAX_CONTAINING_TERMS && !terms.contains(key) {
                too_many = true;
                return false;
            }

            terms.insert(key.to_vec());
            true
        })?;

        if !scanned || too_many {
            return Ok(None);
        }

        let mut queries = Vec::with_capacity(terms.len());

        for key in &terms {
            let Ok(text) = std::str::from_utf8(key) else {
                return Ok(None);
            };

            queries.push(term_query(target, text));
        }

        Ok(Some(match queries.len() {
            0 => Box::new(EmptyQuery),
            1 => queries.pop().expect("one query"),
            _ => Box::new(BooleanQuery::union(queries)),
        }))
    }

    /// 能查子串的分词字段与它的分词方式：归档记下了长词（格式 3 起）。
    fn substring_field(&self, field: &str) -> Option<(Field, Tokenizer)> {
        if self.format_version() < LONG_GRAMS_VERSION {
            return None;
        }

        self.tokenized_field(field)
    }

    /// 没进词典的长词里可能有 `term`。读不出的记录当作什么都可能有。
    fn long_tokens_may_contain(&self, field: &str, term: &str) -> bool {
        match self.puffin_dir.property(&long_grams_property(field)) {
            None => false,
            Some(text) => LongGrams::decode(text).is_none_or(|grams| grams.may_contain(term)),
        }
    }

    /// 分词建索引的文本字段，与它的分词方式。
    fn tokenized_field(&self, name: &str) -> Option<(Field, Tokenizer)> {
        let field = self.schema.get_field(name).ok()?;

        let TantivyFieldType::Str(options) = self.schema.get_field_entry(field).field_type() else {
            return None;
        };

        match options.get_indexing_options()?.tokenizer() {
            "default" => Some((field, Tokenizer::Default)),
            TEXT_TOKENIZER => Some((field, Tokenizer::Text)),
            _ => None,
        }
    }
}

/// 一段中日韩文字 `text` 可能出现：两个字以上时它的每个二元组都是词；一个字时它是单字词，或落在
/// 某个二元组里。词典太大不逐个查时为 `None`。
fn cjk_may_occur(searcher: &Searcher, target: Field, text: &[char]) -> Result<Option<bool>> {
    if let [single] = text {
        let mut buffer = [0_u8; 4];
        let needle = single.encode_utf8(&mut buffer).as_bytes();
        let mut found = false;

        let scanned = scan_containing(searcher, target, needle, &mut |_| {
            found = true;
            false
        })?;

        return Ok(scanned.then_some(found));
    }

    for pair in text.windows(2) {
        let bigram = pair.iter().collect::<String>();

        let count = term_query(target, &bigram)
            .count(searcher)
            .map_err(|e| anyhow!("tantivy count: {e}"))?;

        if count == 0 {
            return Ok(Some(false));
        }
    }

    Ok(Some(true))
}

/// 依次把 `field` 各段词典里包含 `needle` 的词交给 `visit`，`visit` 返回 `false` 时停下。
/// 词典合计超过 [`MAX_SCANNED_TERMS`] 个词时一个也不看，返回 `false`。
fn scan_containing(
    searcher: &Searcher,
    field: Field,
    needle: &[u8],
    visit: &mut dyn FnMut(&[u8]) -> bool,
) -> Result<bool> {
    let indexes = searcher
        .segment_readers()
        .iter()
        .map(|segment| segment.inverted_index(field))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow!("tantivy terms: {e}"))?;

    let terms = indexes
        .iter()
        .map(|index| index.terms().num_terms())
        .sum::<usize>();

    if terms > MAX_SCANNED_TERMS {
        return Ok(false);
    }

    for index in &indexes {
        let mut stream = index
            .terms()
            .stream()
            .map_err(|e| anyhow!("tantivy terms: {e}"))?;

        while stream.advance() {
            let key = stream.key();

            if key.windows(needle.len()).any(|window| window == needle) && !visit(key) {
                return Ok(true);
            }
        }
    }

    Ok(true)
}
