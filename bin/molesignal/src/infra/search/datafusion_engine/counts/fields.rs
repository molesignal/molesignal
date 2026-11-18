// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Which conditions and columns of a count the index archives can answer, by the stream's
//! fields.

use domain::stream::{FieldType, StreamDefinition, StreamIndexType};
use query_language::{KEY_VALUE_SUFFIX, NumberRange};

/// 能原样成为一个整值索引 term 的字面量长度上限。远小于 Tantivy 丢弃 term 的长度，命中这样
/// 字面量的值一定被索引过。
const MAX_INDEXED_TERM_BYTES: usize = 1024;

/// 等值条件都落在整值索引字段上，字面量能原样成为一个 term；`json_text(列, 'key')` 的条件落在
/// 键值索引的 JSON 字段上。
pub(in crate::infra::search::datafusion_engine) fn exact(
    filters: &[(String, String)],
    definition: &StreamDefinition,
) -> bool {
    filters.iter().all(|(column, value)| {
        !value.is_empty()
            && value.len() <= MAX_INDEXED_TERM_BYTES
            && definition.schema.fields.iter().any(|field| {
                !field.encrypted
                    && match column.strip_suffix(KEY_VALUE_SUFFIX) {
                        // `json_text(列, 'key')` 的条件：键值索引里的一个词。
                        Some(json) => {
                            field.name == json
                                && field.data_type == FieldType::Json
                                && field.index_type == StreamIndexType::KeyValue
                        }
                        None => {
                            field.name == *column
                                && matches!(field.data_type, FieldType::Utf8 | FieldType::Json)
                                && field.index_type == StreamIndexType::Exact
                        }
                    }
            })
    })
}

/// 子串条件都落在全文索引的文本字段上，子串只由小写 ASCII 字母与数字组成：这样的子串整个落在
/// 一个词里，有它的行能从词典数出来，见 [`IndexHandle::contains_query`]。
///
/// [`IndexHandle::contains_query`]: crate::infra::search::tantivy_index::IndexHandle::contains_query
pub(super) fn contained(contains: &[(String, String)], definition: &StreamDefinition) -> bool {
    contains.iter().all(|(column, term)| {
        !term.is_empty()
            && term.len() <= MAX_INDEXED_TERM_BYTES
            && term
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && definition.schema.fields.iter().any(|field| {
                field.name == *column
                    && !field.encrypted
                    && field.data_type == FieldType::Utf8
                    && field.index_type == StreamIndexType::FullText
            })
    })
}

/// Every column a count compares with a number is a `range` field, which the index holds by its
/// numbers; an archive written before the field was one answers nothing and its file is scanned.
pub(super) fn ranged(ranges: &[(String, NumberRange)], definition: &StreamDefinition) -> bool {
    ranges.iter().all(|(column, _)| {
        definition.schema.fields.iter().any(|field| {
            field.name == *column && !field.encrypted && field.index_type == StreamIndexType::Range
        })
    })
}

/// 按整值建索引的文本字段：`CAST(列 AS VARCHAR)`、`NULLIF(列, '')` 读到的就是索引里的值。
pub(super) fn text_exact(column: &str, definition: &StreamDefinition) -> bool {
    definition.schema.fields.iter().any(|field| {
        field.name == column
            && !field.encrypted
            && field.data_type == FieldType::Utf8
            && field.index_type == StreamIndexType::Exact
    })
}
