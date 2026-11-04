// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 文件级过滤器先排除不可能命中的文件：封存分区一个分区只读一个对象，不必打开每个文件的索引；
//! 还没封存的段读它自己的过滤器 Artifact。过滤器只会放过不该放过的，不会漏掉含有该值的文件。

use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use futures::{StreamExt, stream};
use sqlparser::ast::{BinaryOperator, Expr, UnaryOperator, Value};
use tantivy::tokenizer::MAX_TOKEN_LEN;

use super::DataFusionEngine;
use crate::{
    domain::{
        storage::QueryFile,
        stream::{FieldType, StreamDefinition, StreamIndexType},
    },
    infra::{
        query::{
            parser::{
                Refuted, WhereTree, atom_bit, column_name, equality_atom, key_value_field,
                key_value_terms, like_atom,
            },
            refutations::Refutations,
        },
        storage::filters::{Filters, Gram, literal_grams},
    },
};

/// 同时读取的过滤器对象数。
const CONCURRENT_READS: usize = 16;

/// 过滤器能回答的原子条件。
#[derive(Debug, Default)]
pub(super) struct FilterTerms {
    /// 原子、字段与它可取的整值：`exact` 与 `bloom` 字段上的 `=`、`IN`（整数字段的值写成十进制）；
    /// 键值索引的 JSON 字段上 `json_text(字段, 'key')` 的条件，字段是键值索引在归档里的名字，值是
    /// 键值词。
    values: Vec<(usize, String, Vec<String>)>,
    /// 原子、字段与模式里所有字面量的三元组：`ngram` 字段上的 `LIKE`、`ILIKE`（`MATCH` 改写成
    /// 的也是）。三元组全部出现它才可能成立。
    grams: Vec<(usize, String, Vec<Gram>)>,
}

impl FilterTerms {
    /// `definition` 的字段上，条件树里过滤器能回答的原子条件。
    pub(super) fn new(definition: Option<&StreamDefinition>, tree: &WhereTree) -> Self {
        let Some(definition) = definition else {
            return Self::default();
        };

        let index_of = |column: &str| {
            definition
                .schema
                .fields
                .iter()
                .find(|field| field.name == column && !field.encrypted)
                .map(|field| (field.data_type, field.effective_index_type()))
        };

        let mut terms = Self::default();

        for (atom, expr) in tree.atoms() {
            if let Some((column, words)) = key_value_terms(expr) {
                // 分区过滤器从索引词典建，同整值字段。
                if index_of(&column) == Some((FieldType::Json, StreamIndexType::KeyValue))
                    && words.iter().all(|word| word.len() <= MAX_TOKEN_LEN)
                {
                    terms.values.push((atom, key_value_field(&column), words));
                }
            } else if let Some((column, numbers)) = integer_atom(expr)
                && matches!(
                    index_of(&column),
                    Some((
                        FieldType::Int64,
                        StreamIndexType::Bloom | StreamIndexType::Exact
                    ))
                )
            {
                let numbers = numbers.iter().map(i64::to_string).collect();
                terms.values.push((atom, column, numbers));
            } else if let Some(set) = equality_atom(expr) {
                let limit = match index_of(&set.column) {
                    // 分区过滤器从索引词典建，词典里没有长到没能建索引的值。
                    Some((FieldType::Utf8 | FieldType::Json, StreamIndexType::Exact)) => {
                        MAX_TOKEN_LEN
                    }
                    Some((FieldType::Utf8, StreamIndexType::Bloom)) => usize::MAX,
                    _ => continue,
                };

                if set
                    .values
                    .iter()
                    .all(|value| !value.is_empty() && value.len() <= limit)
                {
                    terms.values.push((atom, set.column, set.values));
                }
            } else if let Some(set) = like_atom(expr)
                && index_of(&set.column) == Some((FieldType::Utf8, StreamIndexType::Ngram))
            {
                let grams = set
                    .patterns
                    .iter()
                    .flat_map(|pattern| {
                        pattern
                            .literals
                            .iter()
                            .filter_map(|literal| literal_grams(literal, pattern.case_insensitive))
                            .flatten()
                    })
                    .collect::<Vec<_>>();

                if !grams.is_empty() {
                    terms.grams.push((atom, set.column, grams));
                }
            }
        }

        terms
    }

    fn is_empty(&self) -> bool {
        self.values.is_empty() && self.grams.is_empty()
    }

    /// 这些原子的位。
    pub(super) fn mask(&self) -> Refuted {
        self.values
            .iter()
            .map(|(atom, ..)| atom)
            .chain(self.grams.iter().map(|(atom, ..)| atom))
            .fold(0, |mask, atom| mask | atom_bit(*atom))
    }

    /// `filters` 证明 `segment` 里不成立的原子。
    fn refuted(&self, filters: &Filters, segment: &str) -> Refuted {
        let values = self.values.iter().filter(|(_, field, values)| {
            values
                .iter()
                .all(|value| filters.may_contain(field, segment, value) == Some(false))
        });

        let grams = self.grams.iter().filter(|(_, field, grams)| {
            filters.may_contain_grams(field, segment, grams) == Some(false)
        });

        values
            .map(|(atom, ..)| atom)
            .chain(grams.map(|(atom, ..)| atom))
            .fold(0, |refuted, atom| refuted | atom_bit(*atom))
    }
}

/// 整数上的 `=`、`IN`：列与它可取的整数。字面量可以带引号：`'42'` 与整数列比较时就是 42。
fn integer_atom(expr: &Expr) -> Option<(String, Vec<i64>)> {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Eq,
            right,
        } => {
            let (column, number) = column_name(left)
                .zip(integer(right))
                .or_else(|| column_name(right).zip(integer(left)))?;

            Some((column, vec![number]))
        }
        Expr::InList {
            expr,
            list,
            negated: false,
        } => {
            let column = column_name(expr)?;
            let numbers = list.iter().map(integer).collect::<Option<Vec<_>>>()?;

            (!numbers.is_empty()).then_some((column, numbers))
        }
        _ => None,
    }
}

fn integer(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Value(value) => match &value.value {
            Value::Number(text, _) | Value::SingleQuotedString(text) => text.parse().ok(),
            _ => None,
        },
        Expr::UnaryOp {
            op: UnaryOperator::Minus,
            expr,
        } => integer(expr)?.checked_neg(),
        Expr::Nested(expr) => integer(expr),
        _ => None,
    }
}

/// `files` 里，过滤器证明 `terms` 的一些原子不成立之后条件树仍可能成立的文件。`bloom_keys` 是
/// 封存分区里每个段的分区过滤器对象，`filter_keys` 是每个段自己的过滤器 Artifact；分区对象读不出
/// 时用段自己的。没有过滤器、或过滤器读不出的文件证明不了什么。
pub(super) async fn prune(
    engine: &DataFusionEngine,
    files: Vec<QueryFile>,
    terms: &FilterTerms,
    refutations: &mut Refutations,
    bloom_keys: &HashMap<String, String>,
    filter_keys: &HashMap<String, String>,
) -> Vec<QueryFile> {
    let Some(source) = &engine.catalog_source else {
        return files;
    };

    if terms.is_empty() || (bloom_keys.is_empty() && filter_keys.is_empty()) {
        return files;
    }

    let keys = files
        .iter()
        .flat_map(|file| {
            [
                bloom_keys.get(file.id.as_str()),
                filter_keys.get(file.id.as_str()),
            ]
        })
        .flatten()
        .cloned()
        .collect::<BTreeSet<_>>();

    let loaded: HashMap<String, Arc<Filters>> = stream::iter(keys)
        .map(|key| async move {
            match source.partition_blooms(&key).await {
                Ok(filters) => filters.map(|filters| (key, filters)),
                Err(error) => {
                    tracing::warn!(object_key = key, %error, "filters unreadable");
                    None
                }
            }
        })
        .buffer_unordered(CONCURRENT_READS)
        .filter_map(std::future::ready)
        .collect()
        .await;

    refutations.retain(files, |file| {
        let segment = file.id.as_str();

        bloom_keys
            .get(segment)
            .and_then(|key| loaded.get(key))
            .or_else(|| filter_keys.get(segment).and_then(|key| loaded.get(key)))
            .map_or(0, |filters| terms.refuted(filters, segment))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::stream::{FieldDef, Schema, StreamType},
        shared::{ids::Id, time::TimestampMicros},
    };

    fn definition() -> StreamDefinition {
        let field = |name: &str, index_type: StreamIndexType| {
            let mut field = FieldDef {
                name: name.into(),
                data_type: FieldType::Utf8,
                nullable: true,
                index_type: None,
                indexed: false,
                encrypted: false,
                exact: false,
            };

            field.configure_index(true, index_type);
            field
        };

        StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org"),
            name: "logs".into(),
            stream_type: StreamType::LOGS,
            schema: Schema {
                fields: vec![
                    field("trace_id", StreamIndexType::Exact),
                    field("request_id", StreamIndexType::Bloom),
                    field("url", StreamIndexType::Ngram),
                    field("message", StreamIndexType::FullText),
                ],
            },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn terms(sql: &str) -> FilterTerms {
        FilterTerms::new(Some(&definition()), &WhereTree::parse(sql))
    }

    /// 过滤器证明一些原子不成立之后，`sql` 的条件树在 `segment` 里仍可能成立。
    fn may_match(sql: &str, filters: &Filters, segment: &str) -> bool {
        WhereTree::parse(sql).may_hold(terms(sql).refuted(filters, segment))
    }

    #[test]
    fn whole_values_come_from_exact_and_bloom_fields_and_trigrams_from_ngram_fields() {
        let terms = terms(
            "SELECT * FROM logs WHERE trace_id IN ('a', 'b') AND request_id = 'r' \
             AND message = 'm' AND url ILIKE '%/api/v1/%' AND message ILIKE '%disk%'",
        );

        assert_eq!(
            terms.values,
            [
                (
                    0,
                    "trace_id".to_string(),
                    vec!["a".to_string(), "b".to_string()]
                ),
                (1, "request_id".to_string(), vec!["r".to_string()]),
            ]
        );
        assert_eq!(terms.grams.len(), 1);
        assert_eq!((terms.grams[0].0, terms.grams[0].1.as_str()), (3, "url"));
    }

    #[test]
    fn literals_too_short_for_a_trigram_or_too_long_for_the_index_prove_nothing() {
        let long = "x".repeat(MAX_TOKEN_LEN + 1);

        assert!(terms(&format!("SELECT * FROM logs WHERE trace_id = '{long}'")).is_empty());
        assert!(terms("SELECT * FROM logs WHERE trace_id = ''").is_empty());

        // 一支没有三元组，这个 OR 怎样都可能成立。
        let either = "SELECT * FROM logs WHERE url LIKE '%ab%' OR url LIKE '%abcdef%'";
        assert!(WhereTree::parse(either).may_hold(terms(either).mask()));
    }

    /// 整数字段的值写成十进制进过滤器：带不带引号的整数字面量都能查。
    #[test]
    fn integer_fields_answer_their_equalities() {
        let mut definition = definition();
        let mut user_id = definition.schema.fields[1].clone();
        user_id.name = "user_id".into();
        user_id.data_type = FieldType::Int64;
        definition.schema.fields.push(user_id);

        let terms = |sql: &str| FilterTerms::new(Some(&definition), &WhereTree::parse(sql));

        assert_eq!(
            terms("SELECT * FROM logs WHERE user_id = 42 OR user_id IN ('-7', 9)").values,
            [
                (0, "user_id".to_string(), vec!["42".to_string()]),
                (
                    1,
                    "user_id".to_string(),
                    vec!["-7".to_string(), "9".to_string()]
                ),
            ]
        );
        assert!(terms("SELECT * FROM logs WHERE user_id = 4.2").is_empty());

        let mut filters = Filters::default();
        let mut bloom = crate::infra::storage::filters::Bloom::for_values(4).unwrap();
        bloom.insert(b"42");

        filters.insert(
            crate::infra::storage::filters::FilterKind::Value,
            "user_id",
            "segment",
            bloom,
        );

        let sql = "SELECT * FROM logs WHERE user_id = 7";
        assert!(!WhereTree::parse(sql).may_hold(terms(sql).refuted(&filters, "segment")));

        let sql = "SELECT * FROM logs WHERE user_id IN (7, 42)";
        assert!(WhereTree::parse(sql).may_hold(terms(sql).refuted(&filters, "segment")));
    }

    #[test]
    fn a_file_is_kept_unless_some_condition_is_certainly_absent() {
        let mut filters = Filters::default();
        let mut bloom = crate::infra::storage::filters::Bloom::for_values(4).unwrap();
        bloom.insert(b"r");

        filters.insert(
            crate::infra::storage::filters::FilterKind::Value,
            "request_id",
            "segment",
            bloom,
        );

        assert!(may_match(
            "SELECT * FROM logs WHERE request_id IN ('x', 'r')",
            &filters,
            "segment"
        ));
        assert!(!may_match(
            "SELECT * FROM logs WHERE request_id = 'x'",
            &filters,
            "segment"
        ));
        assert!(may_match(
            "SELECT * FROM logs WHERE trace_id = 'x'",
            &filters,
            "segment"
        ));
        assert!(may_match(
            "SELECT * FROM logs WHERE request_id = 'x'",
            &filters,
            "other"
        ));
        assert!(!may_match(
            "SELECT * FROM logs WHERE request_id = 'x' OR request_id = 'y'",
            &filters,
            "segment"
        ));
        assert!(may_match(
            "SELECT * FROM logs WHERE request_id = 'x' OR trace_id = 'y'",
            &filters,
            "segment"
        ));
    }
}
