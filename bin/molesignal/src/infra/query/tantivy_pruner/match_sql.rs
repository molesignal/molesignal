// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SQL 里的 `MATCH` 与 `MATCH_TEXT` 调用改写成 DataFusion 能执行的 `ILIKE`；`LIKE` / `ILIKE`
//! 条件（改写出的与语句本来就有的）给出能用来裁剪文件的子串。

use std::ops::ControlFlow;

use sqlparser::{
    ast::{Expr, FunctionArg, FunctionArgExpr, FunctionArguments, Value, visit_expressions},
    dialect::GenericDialect,
    parser::Parser,
};

use super::{
    MatchPredicate, TermMatch,
    match_text::{escape_like_literal, parse_match_text, rewrite_match_text},
};
use crate::infra::{
    query::parser::{LikeSet, column_name},
    search::tantivy_index::is_cjk,
};

/// 把 SQL 文本中的 `MATCH(field, 'term')` 与 `MATCH_TEXT(field, 'query')` 调用（spec 8.5）换成
/// DataFusion 能执行的 `ILIKE` 表达式（避免依赖未注册的 UDF）。裁剪用的子串从改写后的语句里读，
/// 见 [`like_predicates`]。
pub fn rewrite_match_calls(sql: &str) -> String {
    // 每条查询都会调这里，正则只编译一次。`MATCH_TEXT` 在前避免被 `MATCH` 前缀吞掉；开头的
    // 词边界让 `CIDR_MATCH(...)` 这类名字以 MATCH 结尾的函数不被当成 `MATCH` 调用。
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)\b(MATCH_TEXT|MATCH)\s*\(\s*([A-Za-z_][A-Za-z_0-9]*)\s*,\s*'([^']*)'\s*\)",
        )
        .expect("static regex compiles")
    });

    RE.replace_all(sql, |caps: &regex::Captures| {
        let func = caps.get(1).unwrap().as_str();
        let field = caps.get(2).unwrap().as_str();
        let arg = caps.get(3).unwrap().as_str();

        if func.eq_ignore_ascii_case("match") {
            // MATCH：通用子串匹配（ILIKE，大小写不敏感）。`%`/`_`/`\` 一律按字面量转义
            // （`*` 不是 LIKE 通配符，保持普通字符）。
            format!("{field} ILIKE '%{}%'", escape_like_literal_text(arg))
        } else {
            rewrite_match_text(&parse_match_text(arg), field)
        }
    })
    .to_string()
}

/// 一个 `LIKE` / `ILIKE` 原子条件能用来裁剪的子串谓词：模式的每段字面量各一条，值匹配模式时每段
/// 都出现在值里。字面量逐字符小写化：分词索引里的词是小写的，区分大小写的 `LIKE` 命中的值小写后
/// 也含小写的字面量。没有能查的字面量时为空。
pub(super) fn like_predicates(set: &LikeSet) -> Vec<MatchPredicate> {
    set.patterns
        .iter()
        .flat_map(|pattern| &pattern.literals)
        .filter(|literal| searchable(literal))
        .map(|literal| {
            MatchPredicate::new(set.column.clone(), lowercase(literal), TermMatch::Substring)
        })
        .collect()
}

/// 整值字段上的 `LIKE` / `ILIKE` 原子条件能用来裁剪的谓词：模式的每段字面量各一条，值匹配模式时
/// 每段都出现在值里，按词典里的整值判断。
pub(super) fn in_value_predicates(set: &LikeSet) -> Vec<MatchPredicate> {
    set.patterns
        .iter()
        .flat_map(|pattern| {
            pattern.literals.iter().map(|literal| {
                MatchPredicate::new(
                    set.column.clone(),
                    literal.clone(),
                    TermMatch::InValue {
                        case_insensitive: pattern.case_insensitive,
                    },
                )
            })
        })
        .collect()
}

/// 字面量里有分词索引能查的字符：ASCII 字母数字（词典里的词），或中日韩文字（二元组）。
fn searchable(literal: &str) -> bool {
    literal
        .chars()
        .any(|character| character.is_ascii_alphanumeric() || is_cjk(character))
}

/// 与分词索引的小写化一致：逐字符 `to_lowercase`。
fn lowercase(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

fn match_arguments(arguments: &FunctionArguments) -> Option<(String, String)> {
    let FunctionArguments::List(arguments) = arguments else {
        return None;
    };

    let [field, term] = arguments.args.as_slice() else {
        return None;
    };

    let field = column_name(unnamed_expression(field)?)?;

    let Expr::Value(value) = unnamed_expression(term)? else {
        return None;
    };

    let Value::SingleQuotedString(term) = &value.value else {
        return None;
    };

    Some((field, term.clone()))
}

fn unnamed_expression(argument: &FunctionArg) -> Option<&Expr> {
    match argument {
        FunctionArg::Unnamed(FunctionArgExpr::Expr(expression)) => Some(expression),
        _ => None,
    }
}

/// 提取 SQL 中所有 `MATCH_TEXT(field, 'query')` 调用的字段名。走 sqlparser AST 遍历
/// （`visit_expressions`），忽略注释 / 字符串字面量里的伪调用；解析失败返回空 vec。
pub fn match_text_fields(sql: &str) -> Vec<String> {
    let Ok(statements) = Parser::parse_sql(&GenericDialect, sql) else {
        return Vec::new();
    };

    let mut fields = Vec::new();

    let _ = visit_expressions(&statements, |expr| {
        if let Expr::Function(function) = expr
            && function.name.to_string().eq_ignore_ascii_case("match_text")
            && let Some((field, _)) = match_arguments(&function.args)
        {
            fields.push(field);
        }

        ControlFlow::<()>::Continue(())
    });

    fields
}

/// 把 MATCH 的 term 转为 ILIKE pattern 内容：`%`/`_`/`\` 一律按字面量转义（`*` 是普通字符，
/// 不是 LIKE 通配符，保持原样）。
fn escape_like_literal_text(text: &str) -> String {
    let mut out = String::new();

    for c in text.chars() {
        out.push_str(&escape_like_literal(c));
    }

    out
}
