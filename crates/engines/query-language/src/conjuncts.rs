// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 顶层 `WHERE` 里对每一条结果都成立的条件，供查询侧用索引剔除不可能命中的文件。
//!
//! 只看顶层 SELECT 的合取链：合取项对结果的每一行都成立，才能用来剔除整个文件；子查询与 CTE 的
//! 条件作用于别的表。`FROM` 必须只有一张表、不连接（见 [`reads_one_table`]）：连接、自连接与
//! `FROM (子查询)` 里，`WHERE` 的列可能来自别的表或经过计算。同一列上的几个条件用 `OR` 连起来
//! 时整体算一个条件（列满足其中之一）；落在不同列上的 `OR` 整支忽略，文件可经另一支满足。`NOT`
//! 一律忽略。解析失败时没有条件：不裁剪，保守且安全。

use sqlparser::{
    ast::{
        BinaryOperator, CastKind, DataType, Expr, Select, SetExpr, Statement, TableFactor, Value,
    },
    dialect::GenericDialect,
    parser::Parser,
};

use crate::column_name;

/// 一列的值是 `values` 之一。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EqualitySet {
    pub column: String,
    pub values: Vec<String>,
}

/// 一列的文本匹配 `patterns` 之一。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LikeSet {
    pub column: String,
    pub patterns: Vec<LikePattern>,
}

/// `LIKE` / `ILIKE` 的一个模式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LikePattern {
    /// 模式里 `%`、`_` 之间的文字，转义的按原字符计：值匹配模式时，每一段都原样出现在值里
    /// （`ILIKE` 时不计大小写）。
    pub literals: Vec<String>,
    /// 模式以文字开头（`'abc%'`），不以 `%`、`_` 开头：匹配的值以第一段文字开头。
    pub anchored: bool,
    /// `ILIKE`：不计大小写。
    pub case_insensitive: bool,
}

impl LikePattern {
    /// 匹配的值都以它开头的文字：`'abc%'` 的 `abc`。模式以通配符开头时没有。
    pub fn prefix(&self) -> Option<&str> {
        self.literals
            .first()
            .filter(|_| self.anchored)
            .map(String::as_str)
    }
}

/// 等值条件：`col = 'x'`、`'x' = col`、`col IN ('x', 'y')`，以及同一列上这些条件的 `OR`。只收
/// 单引号字符串字面量：整值索引里的词就是这样的字面量。列名按 DataFusion 的解析给出，见
/// [`column_name`]。
pub fn equality_sets(sql: &str) -> Vec<EqualitySet> {
    conjuncts(sql, &equality)
        .into_iter()
        .map(|(column, values)| EqualitySet { column, values })
        .collect()
}

/// 文本匹配条件：`col LIKE 'p'`、`col ILIKE 'p'`（列可以包一层 `CAST(col AS VARCHAR)`），以及
/// 同一列上这些条件的 `OR`。
pub fn like_sets(sql: &str) -> Vec<LikeSet> {
    conjuncts(sql, &like)
        .into_iter()
        .map(|(column, patterns)| LikeSet { column, patterns })
        .collect()
}

/// 一个原子条件（[`crate::WhereTree`] 的叶子）是等值条件时，它落在的列与可取的值：`col = 'x'`、
/// `'x' = col`、`col IN ('x', …)`。
pub fn equality_atom(expr: &Expr) -> Option<EqualitySet> {
    equality(expr).map(|(column, values)| EqualitySet { column, values })
}

/// 一个原子条件是 `LIKE` / `ILIKE` 时，它落在的列与它的模式。
pub fn like_atom(expr: &Expr) -> Option<LikeSet> {
    like(expr).map(|(column, patterns)| LikeSet { column, patterns })
}

/// 一列与它满足其一的候选。
type Alternatives<T> = (String, Vec<T>);

/// 从单个表达式读出一列上的条件。
type Leaf<'a, T> = &'a dyn Fn(&Expr) -> Option<Alternatives<T>>;

/// 顶层合取链上每个能由 `leaf` 读出的条件，`OR` 按上面的规则合并。
fn conjuncts<T>(sql: &str, leaf: Leaf<'_, T>) -> Vec<Alternatives<T>> {
    let Ok(statements) = Parser::parse_sql(&GenericDialect, sql) else {
        return Vec::new();
    };

    let mut out = Vec::new();

    for statement in &statements {
        if let Statement::Query(query) = statement
            && let SetExpr::Select(select) = query.body.as_ref()
            && reads_one_table(select)
            && let Some(selection) = &select.selection
        {
            collect(selection, leaf, &mut out);
        }
    }

    out
}

/// `FROM` 只有一张表、不连接：`WHERE` 里的列（带不带表名限定）就是这张表原样的列。
pub fn reads_one_table(select: &Select) -> bool {
    matches!(
        select.from.as_slice(),
        [from] if from.joins.is_empty()
            && matches!(from.relation, TableFactor::Table { args: None, .. })
    )
}

fn collect<T>(expr: &Expr, leaf: Leaf<'_, T>, out: &mut Vec<Alternatives<T>>) {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            collect(left, leaf, out);
            collect(right, leaf, out);
        }
        Expr::Nested(inner) => collect(inner, leaf, out),
        other => out.extend(alternatives(other, leaf)),
    }
}

/// 一个条件：`leaf` 本身，或每一支都落在同一列上的 `OR`。
fn alternatives<T>(expr: &Expr, leaf: Leaf<'_, T>) -> Option<Alternatives<T>> {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Or,
            right,
        } => {
            let (column, mut values) = alternatives(left, leaf)?;
            let (other, rest) = alternatives(right, leaf)?;

            if column != other {
                return None;
            }

            values.extend(rest);
            Some((column, values))
        }
        Expr::Nested(inner) => alternatives(inner, leaf),
        other => leaf(other),
    }
}

fn equality(expr: &Expr) -> Option<(String, Vec<String>)> {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Eq,
            right,
        } => {
            let (column, value) =
                column_and_string(left, right).or_else(|| column_and_string(right, left))?;

            Some((column, vec![value]))
        }
        Expr::InList {
            expr,
            list,
            negated: false,
        } => {
            let column = column_name(expr)?;
            let values = list.iter().map(string).collect::<Option<Vec<_>>>()?;

            (!values.is_empty()).then_some((column, values))
        }
        _ => None,
    }
}

fn like(expr: &Expr) -> Option<(String, Vec<LikePattern>)> {
    let (expr, pattern, escape_char, case_insensitive) = match expr {
        Expr::Like {
            negated: false,
            any: false,
            expr,
            pattern,
            escape_char,
        } => (expr, pattern, escape_char, false),
        Expr::ILike {
            negated: false,
            any: false,
            expr,
            pattern,
            escape_char,
        } => (expr, pattern, escape_char, true),
        _ => return None,
    };

    let column = text_column(expr)?;
    let pattern = string(pattern)?;

    // 不写 ESCAPE 时 `\` 转义（DataFusion 的约定）；`ESCAPE ''` 不转义。
    let escape = match escape_char {
        None => Some('\\'),
        Some(value) => match &value.value {
            Value::SingleQuotedString(text) => {
                let mut chars = text.chars();

                match (chars.next(), chars.next()) {
                    (None, _) => None,
                    (Some(escape), None) => Some(escape),
                    _ => return None,
                }
            }
            _ => return None,
        },
    };

    // 第一个字符是文字（含转义的通配符）时，匹配的值以第一段文字开头。
    let anchored = pattern
        .chars()
        .next()
        .is_some_and(|first| Some(first) == escape || !matches!(first, '%' | '_'));

    Some((
        column,
        vec![LikePattern {
            literals: literals(&pattern, escape),
            anchored,
            case_insensitive,
        }],
    ))
}

/// 列本身，或把它转成文本的 `CAST`：日志筛选器给每一种字段都写 `CAST(col AS VARCHAR)`。
fn text_column(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Cast {
            kind: CastKind::Cast | CastKind::DoubleColon,
            expr,
            data_type:
                DataType::Varchar(_)
                | DataType::CharacterVarying(_)
                | DataType::CharVarying(_)
                | DataType::Nvarchar(_)
                | DataType::Text
                | DataType::String(_),
            ..
        } => column_name(expr),
        Expr::Nested(inner) => text_column(inner),
        other => column_name(other),
    }
}

/// 模式里 `%`、`_` 之间的文字。
fn literals(pattern: &str, escape: Option<char>) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = pattern.chars();

    while let Some(character) = chars.next() {
        if Some(character) == escape {
            current.push(chars.next().unwrap_or(character));
            continue;
        }

        if matches!(character, '%' | '_') {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }

            continue;
        }

        current.push(character);
    }

    if !current.is_empty() {
        out.push(current);
    }

    out
}

fn column_and_string(column: &Expr, value: &Expr) -> Option<(String, String)> {
    Some((column_name(column)?, string(value)?))
}

fn string(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Value(value) => match &value.value {
            Value::SingleQuotedString(text) => Some(text.clone()),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(column: &str, values: &[&str]) -> EqualitySet {
        EqualitySet {
            column: column.to_owned(),
            values: values.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    #[test]
    fn equalities_read_plain_mirrored_and_qualified_comparisons() {
        assert_eq!(
            equality_sets(
                "SELECT * FROM traces WHERE trace_id = '0af7651916cd43dd' ORDER BY _timestamp"
            ),
            [set("trace_id", &["0af7651916cd43dd"])]
        );
        assert_eq!(
            equality_sets("SELECT * FROM t WHERE 'x' = span_id"),
            [set("span_id", &["x"])]
        );
        assert_eq!(
            equality_sets("SELECT * FROM traces t WHERE t.service_name = 'api'"),
            [set("service_name", &["api"])]
        );
        assert_eq!(
            equality_sets("SELECT * FROM t WHERE trace_id = 'a' AND span_id = 'b'").len(),
            2
        );
    }

    /// 列名按 DataFusion 的解析给出：`TRACE_ID` 过滤的是 `trace_id` 列，`"Trace_Id"` 才是
    /// 大小写原样的那一列。按写法给出会拿另一列的值去裁剪文件。
    #[test]
    fn equalities_name_columns_the_way_datafusion_resolves_them() {
        assert_eq!(
            equality_sets(
                "SELECT * FROM t WHERE TRACE_ID = 'a' AND \"Span_Id\" = 'b' AND T.Service = 'c'"
            ),
            [
                set("trace_id", &["a"]),
                set("Span_Id", &["b"]),
                set("service", &["c"])
            ]
        );
    }

    /// 链路页按 ID 取一批 span：`IN` 列表的每个值都是候选。
    #[test]
    fn an_in_list_and_an_or_of_one_column_are_one_set() {
        assert_eq!(
            equality_sets(
                "SELECT * FROM spans WHERE \"trace_id\" IN ('t1', 't2') AND \"span_id\" IN ('r1')"
            ),
            [set("trace_id", &["t1", "t2"]), set("span_id", &["r1"])]
        );
        assert_eq!(
            equality_sets(
                "SELECT * FROM t WHERE trace_id = 'a' AND (span_id = 'b' OR (span_id IN ('c', 'd')))"
            ),
            [set("trace_id", &["a"]), set("span_id", &["b", "c", "d"])]
        );
    }

    #[test]
    fn equalities_ignore_other_shapes() {
        for sql in [
            "SELECT * FROM t WHERE code = 500",
            "SELECT * FROM t WHERE n > 3",
            "SELECT * FROM t WHERE msg LIKE '%x%'",
            "SELECT * FROM t WHERE a = b",
            "SELECT * FROM t WHERE id NOT IN ('a')",
            "SELECT * FROM t WHERE id IN ('a', 1)",
            "SELECT * FROM t WHERE NOT (id = 'a')",
            "NOT SQL @@@",
        ] {
            assert!(equality_sets(sql).is_empty(), "{sql}");
        }
    }

    /// `FROM` 不是单张表时一个都不提取：连接另一侧、或子查询算出来的同名列不是这张表的列。
    #[test]
    fn joins_and_derived_tables_give_no_conditions() {
        for sql in [
            "SELECT * FROM logs l JOIN traces t ON l.span_id = t.span_id WHERE t.trace_id = 'a'",
            "SELECT * FROM logs a, logs b WHERE b.level = 'error'",
            "SELECT * FROM (SELECT upper(level) AS level FROM logs) t WHERE level = 'ERROR'",
        ] {
            assert!(equality_sets(sql).is_empty(), "{sql}");
        }

        assert_eq!(
            equality_sets("SELECT * FROM logs AS l WHERE l.level = 'error'"),
            [set("level", &["error"])]
        );
    }

    /// `OR` 的两支落在不同列上时一个都不提取：文件可经另一支满足，裁了会漏数据。子查询的条件
    /// 作用于别的表，也不提取。
    #[test]
    fn equalities_skip_disjunctions_of_columns_and_subqueries() {
        assert!(equality_sets("SELECT * FROM t WHERE trace_id = 'a' OR span_id = 'b'").is_empty());
        assert_eq!(
            equality_sets(
                "SELECT * FROM t WHERE trace_id = 'a' AND (span_id = 'b' OR other = 'c')"
            ),
            [set("trace_id", &["a"])]
        );
        assert_eq!(
            equality_sets("SELECT * FROM t WHERE (trace_id = 'a' AND span_id = 'b')").len(),
            2
        );
        assert!(
            equality_sets("SELECT * FROM t WHERE id IN (SELECT id FROM u WHERE k = 'v')")
                .is_empty()
        );
    }

    fn pattern(literals: &[&str], case_insensitive: bool) -> LikePattern {
        LikePattern {
            literals: literals.iter().map(|value| (*value).to_owned()).collect(),
            anchored: false,
            case_insensitive,
        }
    }

    fn anchored(literals: &[&str], case_insensitive: bool) -> LikePattern {
        LikePattern {
            anchored: true,
            ..pattern(literals, case_insensitive)
        }
    }

    #[test]
    fn like_patterns_give_the_literal_runs_between_wildcards() {
        assert_eq!(
            like_sets("SELECT * FROM t WHERE url LIKE '%/api/v1/%orders_x%'"),
            [LikeSet {
                column: "url".into(),
                patterns: vec![pattern(&["/api/v1/", "orders", "x"], false)],
            }]
        );
        assert_eq!(
            like_sets("SELECT * FROM t WHERE CAST(\"host\" AS VARCHAR) ILIKE '%my-api%'"),
            [LikeSet {
                column: "host".into(),
                patterns: vec![pattern(&["my-api"], true)],
            }]
        );
    }

    /// `MATCH` 改写出的 `ILIKE` 把 `%`、`_`、`\` 转义成字面量。
    #[test]
    fn escaped_wildcards_are_literal_characters() {
        assert_eq!(
            like_sets(r"SELECT * FROM t WHERE message ILIKE '%100\%\_done\\%'"),
            [LikeSet {
                column: "message".into(),
                patterns: vec![pattern(&[r"100%_done\"], true)],
            }]
        );
        assert_eq!(
            like_sets("SELECT * FROM t WHERE message LIKE '%a!%b%' ESCAPE '!'"),
            [LikeSet {
                column: "message".into(),
                patterns: vec![pattern(&["a%b"], false)],
            }]
        );
    }

    /// 以文字开头的模式给出前缀；以 `%`、`_` 开头的没有，转义的通配符算文字。
    #[test]
    fn a_pattern_that_starts_with_text_has_a_prefix() {
        let prefixes = |sql: &str| {
            like_sets(sql)
                .into_iter()
                .flat_map(|set| set.patterns)
                .map(|pattern| pattern.prefix().map(str::to_owned))
                .collect::<Vec<_>>()
        };

        assert_eq!(
            prefixes("SELECT * FROM t WHERE trace_id LIKE '0af7%'"),
            [Some("0af7".to_owned())]
        );
        assert_eq!(
            prefixes("SELECT * FROM t WHERE host ILIKE 'web_%'"),
            [Some("web".to_owned())]
        );
        assert_eq!(
            prefixes(r"SELECT * FROM t WHERE host LIKE '\%x%'"),
            [Some("%x".to_owned())]
        );
        assert_eq!(
            prefixes("SELECT * FROM t WHERE host LIKE '%web%' OR host LIKE '_eb%'"),
            [None, None]
        );
        assert_eq!(prefixes("SELECT * FROM t WHERE host LIKE ''"), [None]);
    }

    #[test]
    fn likes_of_one_column_joined_by_or_are_alternatives() {
        assert_eq!(
            like_sets(
                "SELECT * FROM t WHERE level = 'x' AND (message ILIKE '%a%' OR message LIKE 'b%')"
            ),
            [LikeSet {
                column: "message".into(),
                patterns: vec![pattern(&["a"], true), anchored(&["b"], false)],
            }]
        );
        assert!(
            like_sets("SELECT * FROM t WHERE message ILIKE '%a%' OR host ILIKE '%b%'").is_empty()
        );
        assert!(like_sets("SELECT * FROM t WHERE message NOT LIKE '%a%'").is_empty());
    }
}
