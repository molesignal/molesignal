// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 条件树里 Tantivy 能判断的原子条件。

use std::collections::HashSet;

use tantivy::tokenizer::MAX_TOKEN_LEN;

use super::{
    MatchPredicate, TermMatch,
    match_sql::{in_value_predicates, like_predicates},
};
use crate::{
    domain::stream::{FieldType, StreamDefinition, StreamIndexType},
    infra::query::parser::{
        KEY_VALUE_SUFFIX, Refuted, WhereTree, atom_bit, equality_atom, key_value_field,
        key_value_terms, like_atom,
    },
};

/// 一个原子条件在 Tantivy 里的判断：谓词都成立它才可能成立，有一条证明文件里没有，它在这个文件里
/// 就不成立。`LIKE` / `ILIKE` 模式的每段字面量各是一条子串谓词；整值字段上的 `=`、`IN` 是一条整值
/// 谓词，它的命中数就是这个文件里满足这个原子的行数。
#[derive(Debug, Clone)]
pub struct IndexedAtom {
    pub atom: usize,
    pub predicates: Vec<MatchPredicate>,
}

impl IndexedAtom {
    /// 命中数是满足它的行数。
    pub fn counts_rows(&self) -> bool {
        self.predicates
            .iter()
            .any(|predicate| predicate.matching == TermMatch::Value)
    }
}

/// `tree` 的原子里 Tantivy 能判断的：全文字段上的 `LIKE` / `ILIKE`（`MATCH`、`MATCH_TEXT` 改写成的
/// 也是）按子串判断；整值字段上的 `=`、`IN`、`LIKE` / `ILIKE`，与键值索引的 JSON 字段上的
/// `json_text(字段, 'key')` 的 `=`、`IN`、`IS NOT NULL`，按词典里的整值判断。字段按 `definition` 现在的索引类型挑；文件的
/// 归档是按写入时的索引类型建的，对不上的答不出，见
/// [`crate::infra::search::tantivy_index::IndexHandle::count_term`]。
pub fn indexed_atoms(tree: &WhereTree, definition: &StreamDefinition) -> Vec<IndexedAtom> {
    let fields = |index_type: StreamIndexType, text_only: bool| {
        definition
            .schema
            .fields
            .iter()
            .filter(|field| {
                !field.encrypted
                    && field.effective_index_type() == index_type
                    && (!text_only || matches!(field.data_type, FieldType::Utf8 | FieldType::Json))
            })
            .map(|field| field.name.as_str())
            .collect::<HashSet<_>>()
    };

    let full_text = fields(StreamIndexType::FullText, false);
    let exact = fields(StreamIndexType::Exact, true);
    let key_value = fields(StreamIndexType::KeyValue, true);

    tree.atoms()
        .filter_map(|(atom, expr)| {
            if let Some((column, terms)) = key_value_terms(expr) {
                return (key_value.contains(column.as_str())
                    && terms.iter().all(|term| term.len() <= MAX_TOKEN_LEN))
                .then(|| IndexedAtom {
                    atom,
                    predicates: vec![MatchPredicate {
                        field: key_value_field(&column),
                        terms,
                        matching: TermMatch::Value,
                    }],
                });
            }

            let predicates = match equality_atom(expr) {
                // 索引丢掉了过长的值：这样的字面量查不到也不说明文件里没有。空串不用来裁剪，与
                // 计数一致。
                Some(set) => (exact.contains(set.column.as_str())
                    && set
                        .values
                        .iter()
                        .all(|value| !value.is_empty() && value.len() <= MAX_TOKEN_LEN))
                .then(|| {
                    vec![MatchPredicate {
                        field: set.column,
                        terms: set.values,
                        matching: TermMatch::Value,
                    }]
                })?,
                None => {
                    let set = like_atom(expr)?;

                    if full_text.contains(set.column.as_str()) {
                        like_predicates(&set)
                    } else if exact.contains(set.column.as_str()) {
                        in_value_predicates(&set)
                    } else {
                        return None;
                    }
                }
            };

            (!predicates.is_empty()).then_some(IndexedAtom { atom, predicates })
        })
        .collect()
}

/// 顶层合取链上的键值原子：字段（键值索引在归档里的名字）与满足它的行必有其一的词。每一行都
/// 满足它们，命中的行就是要读的行的超集。
pub fn key_value_conjuncts(tree: &WhereTree, atoms: &[IndexedAtom]) -> Vec<(String, Vec<String>)> {
    let conjuncts = tree.conjuncts();

    atoms
        .iter()
        .filter(|indexed| conjuncts.contains(&indexed.atom))
        .filter_map(|indexed| match indexed.predicates.as_slice() {
            [predicate] if predicate.field.ends_with(KEY_VALUE_SUFFIX) => {
                Some((predicate.field.clone(), predicate.terms.clone()))
            }
            _ => None,
        })
        .collect()
}

/// `atoms` 的位。
pub fn mask(atoms: &[IndexedAtom]) -> Refuted {
    atoms
        .iter()
        .fold(0, |mask, atom| mask | atom_bit(atom.atom))
}

/// 整值原子的命中数能给出满足整棵树的行数上界：按时间取一页的查询据此决定先读哪些文件。
pub fn counts_rows(tree: &WhereTree, atoms: &[IndexedAtom]) -> bool {
    tree.bound(&|atom| {
        atoms
            .iter()
            .any(|indexed| indexed.atom == atom && indexed.counts_rows())
            .then_some(0)
    })
    .is_some()
}
