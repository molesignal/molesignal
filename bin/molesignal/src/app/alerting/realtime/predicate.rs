// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 实时规则的 `WHERE` 子句：把 SQL 表达式编译成可直接对事件字段求值的谓词。
//!
//! 支持字段与字面量的比较（`=`、`!=`、`<>`、`<`、`<=`、`>`、`>=`）、`IN`、`LIKE`、`ILIKE`、
//! `BETWEEN`、`IS [NOT] NULL`，以及 `AND`、`OR`、`NOT` 与括号。求值遵循 SQL 的三值逻辑：
//! 字段缺失或为 null 的比较结果未知，未知按不匹配处理。

use std::cmp::Ordering;

use serde_json::{Map, Value as Json};
use sqlparser::ast::{BinaryOperator, Expr, UnaryOperator, Value};

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Literal {
    Str(String),
    Num(f64),
    Bool(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Cmp {
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Predicate {
    Compare {
        field: String,
        op: Cmp,
        value: Literal,
    },
    In {
        field: String,
        values: Vec<Literal>,
        negated: bool,
    },
    Like {
        field: String,
        pattern: Vec<char>,
        case_insensitive: bool,
        negated: bool,
    },
    Between {
        field: String,
        low: Literal,
        high: Literal,
        negated: bool,
    },
    IsNull {
        field: String,
        negated: bool,
    },
    Not(Box<Predicate>),
    And(Box<Predicate>, Box<Predicate>),
    Or(Box<Predicate>, Box<Predicate>),
}

impl Predicate {
    pub(super) fn compile(expr: &Expr) -> Result<Self, String> {
        match expr {
            Expr::Nested(inner) => Self::compile(inner),
            Expr::BinaryOp { left, op, right } => compile_binary(left, op, right),
            Expr::UnaryOp {
                op: UnaryOperator::Not,
                expr,
            } => Ok(Self::Not(Box::new(Self::compile(expr)?))),
            Expr::IsNull(inner) => Ok(Self::IsNull {
                field: field_name(inner)?,
                negated: false,
            }),
            Expr::IsNotNull(inner) => Ok(Self::IsNull {
                field: field_name(inner)?,
                negated: true,
            }),
            Expr::InList {
                expr,
                list,
                negated,
            } => Ok(Self::In {
                field: field_name(expr)?,
                values: list.iter().map(literal).collect::<Result<_, _>>()?,
                negated: *negated,
            }),
            Expr::Between {
                expr,
                negated,
                low,
                high,
            } => Ok(Self::Between {
                field: field_name(expr)?,
                low: literal(low)?,
                high: literal(high)?,
                negated: *negated,
            }),
            Expr::Like {
                negated,
                any: false,
                expr,
                pattern,
                escape_char: None,
            } => compile_like(expr, pattern, *negated, false),
            Expr::ILike {
                negated,
                any: false,
                expr,
                pattern,
                escape_char: None,
            } => compile_like(expr, pattern, *negated, true),
            other => Err(format!("unsupported condition `{other}`")),
        }
    }

    pub(super) fn matches(&self, fields: &Map<String, Json>) -> bool {
        self.eval(fields) == Some(true)
    }

    fn eval(&self, fields: &Map<String, Json>) -> Option<bool> {
        match self {
            Self::Compare { field, op, value } => {
                let ordering = compare(present(fields, field)?, value)?;

                Some(match op {
                    Cmp::Eq => ordering == Ordering::Equal,
                    Cmp::NotEq => ordering != Ordering::Equal,
                    Cmp::Lt => ordering == Ordering::Less,
                    Cmp::LtEq => ordering != Ordering::Greater,
                    Cmp::Gt => ordering == Ordering::Greater,
                    Cmp::GtEq => ordering != Ordering::Less,
                })
            }
            Self::In {
                field,
                values,
                negated,
            } => {
                let value = present(fields, field)?;

                let found = values
                    .iter()
                    .any(|literal| compare(value, literal) == Some(Ordering::Equal));

                Some(found != *negated)
            }
            Self::Like {
                field,
                pattern,
                case_insensitive,
                negated,
            } => {
                let Json::String(text) = present(fields, field)? else {
                    return None;
                };

                Some(like_matches(text, pattern, *case_insensitive) != *negated)
            }
            Self::Between {
                field,
                low,
                high,
                negated,
            } => {
                let value = present(fields, field)?;
                let within = compare(value, low)? != Ordering::Less
                    && compare(value, high)? != Ordering::Greater;
                Some(within != *negated)
            }
            Self::IsNull { field, negated } => Some(present(fields, field).is_none() != *negated),
            Self::Not(inner) => inner.eval(fields).map(|value| !value),
            Self::And(left, right) => match (left.eval(fields), right.eval(fields)) {
                (Some(false), _) | (_, Some(false)) => Some(false),
                (Some(true), Some(true)) => Some(true),
                _ => None,
            },
            Self::Or(left, right) => match (left.eval(fields), right.eval(fields)) {
                (Some(true), _) | (_, Some(true)) => Some(true),
                (Some(false), Some(false)) => Some(false),
                _ => None,
            },
        }
    }
}

fn compile_binary(left: &Expr, op: &BinaryOperator, right: &Expr) -> Result<Predicate, String> {
    let cmp = match op {
        BinaryOperator::And => {
            return Ok(Predicate::And(
                Box::new(Predicate::compile(left)?),
                Box::new(Predicate::compile(right)?),
            ));
        }
        BinaryOperator::Or => {
            return Ok(Predicate::Or(
                Box::new(Predicate::compile(left)?),
                Box::new(Predicate::compile(right)?),
            ));
        }
        BinaryOperator::Eq => Cmp::Eq,
        BinaryOperator::NotEq => Cmp::NotEq,
        BinaryOperator::Lt => Cmp::Lt,
        BinaryOperator::LtEq => Cmp::LtEq,
        BinaryOperator::Gt => Cmp::Gt,
        BinaryOperator::GtEq => Cmp::GtEq,
        other => return Err(format!("unsupported operator `{other}`")),
    };

    // `field OP literal`，或字段在右侧时翻转比较方向。
    if let (Ok(field), Ok(value)) = (field_name(left), literal(right)) {
        return Ok(Predicate::Compare {
            field,
            op: cmp,
            value,
        });
    }

    if let (Ok(value), Ok(field)) = (literal(left), field_name(right)) {
        let flipped = match cmp {
            Cmp::Lt => Cmp::Gt,
            Cmp::LtEq => Cmp::GtEq,
            Cmp::Gt => Cmp::Lt,
            Cmp::GtEq => Cmp::LtEq,
            same => same,
        };

        return Ok(Predicate::Compare {
            field,
            op: flipped,
            value,
        });
    }

    Err(format!(
        "`{left} {op} {right}` must compare a field with a literal"
    ))
}

fn compile_like(
    expr: &Expr,
    pattern: &Expr,
    negated: bool,
    case_insensitive: bool,
) -> Result<Predicate, String> {
    let Literal::Str(pattern) = literal(pattern)? else {
        return Err("LIKE needs a string pattern".into());
    };

    let pattern = if case_insensitive {
        pattern.to_lowercase()
    } else {
        pattern
    };

    Ok(Predicate::Like {
        field: field_name(expr)?,
        pattern: pattern.chars().collect(),
        case_insensitive,
        negated,
    })
}

fn field_name(expr: &Expr) -> Result<String, String> {
    match expr {
        Expr::Identifier(ident) => Ok(ident.value.clone()),
        // `service.name` 这类带点的字段名按整体查找。
        Expr::CompoundIdentifier(parts) => Ok(parts
            .iter()
            .map(|part| part.value.as_str())
            .collect::<Vec<_>>()
            .join(".")),
        Expr::Nested(inner) => field_name(inner),
        other => Err(format!("`{other}` is not a field")),
    }
}

fn literal(expr: &Expr) -> Result<Literal, String> {
    match expr {
        Expr::Value(value) => match &value.value {
            Value::SingleQuotedString(text) => Ok(Literal::Str(text.clone())),
            Value::Number(number, _) => number
                .parse()
                .map(Literal::Num)
                .map_err(|_| format!("`{number}` is not a number")),
            Value::Boolean(flag) => Ok(Literal::Bool(*flag)),
            other => Err(format!("unsupported literal `{other}`")),
        },
        Expr::UnaryOp {
            op: UnaryOperator::Minus,
            expr,
        } => match literal(expr)? {
            Literal::Num(number) => Ok(Literal::Num(-number)),
            _ => Err(format!("`-{expr}` is not a number")),
        },
        Expr::UnaryOp {
            op: UnaryOperator::Plus,
            expr,
        } => match literal(expr)? {
            number @ Literal::Num(_) => Ok(number),
            _ => Err(format!("`+{expr}` is not a number")),
        },
        Expr::Nested(inner) => literal(inner),
        other => Err(format!("`{other}` is not a literal")),
    }
}

/// 字段存在且不为 null 时的值。
fn present<'a>(fields: &'a Map<String, Json>, field: &str) -> Option<&'a Json> {
    fields.get(field).filter(|value| !value.is_null())
}

fn compare(value: &Json, literal: &Literal) -> Option<Ordering> {
    match (value, literal) {
        (Json::String(text), Literal::Str(other)) => Some(text.as_str().cmp(other.as_str())),
        (Json::Number(number), Literal::Num(other)) => number.as_f64()?.partial_cmp(other),
        (Json::Bool(flag), Literal::Bool(other)) => Some(flag.cmp(other)),
        _ => None,
    }
}

/// SQL `LIKE`：`%` 匹配任意长度，`_` 匹配单个字符。`pattern` 在不区分大小写时已转小写。
fn like_matches(text: &str, pattern: &[char], case_insensitive: bool) -> bool {
    let text: Vec<char> = if case_insensitive {
        text.to_lowercase().chars().collect()
    } else {
        text.chars().collect()
    };

    let (mut at, mut next) = (0, 0);
    let mut resume: Option<(usize, usize)> = None;

    while at < text.len() {
        match pattern.get(next) {
            Some('%') => {
                resume = Some((next + 1, at));
                next += 1;
            }
            Some(&symbol) if symbol == '_' || symbol == text[at] => {
                at += 1;
                next += 1;
            }
            _ => match resume {
                Some((pattern_at, text_at)) => {
                    resume = Some((pattern_at, text_at + 1));
                    next = pattern_at;
                    at = text_at + 1;
                }
                None => return false,
            },
        }
    }

    pattern[next..].iter().all(|symbol| *symbol == '%')
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use sqlparser::{dialect::GenericDialect, parser::Parser};

    use super::*;

    fn compile(condition: &str) -> Result<Predicate, String> {
        let expr = Parser::new(&GenericDialect)
            .try_with_sql(condition)
            .and_then(|mut parser| parser.parse_expr())
            .map_err(|error| error.to_string())?;
        Predicate::compile(&expr)
    }

    fn matches(condition: &str, event: serde_json::Value) -> bool {
        let Json::Object(fields) = event else {
            panic!("event must be an object");
        };
        compile(condition)
            .expect("condition compiles")
            .matches(&fields)
    }

    #[test]
    fn compares_fields_with_literals() {
        let event = json!({"level": "error", "status": 503, "ok": false, "latency": 12.5});
        assert!(matches("level = 'error'", event.clone()));
        assert!(!matches("level != 'error'", event.clone()));
        assert!(matches("status >= 500 AND status < 600", event.clone()));
        assert!(matches("500 < status", event.clone()));
        assert!(matches("latency > -1", event.clone()));
        assert!(matches("ok = false", event.clone()));
        assert!(!matches("status = 'x'", event));
    }

    #[test]
    fn missing_and_null_fields_never_match_a_comparison() {
        let event = json!({"level": null});
        assert!(!matches("level = 'error'", event.clone()));
        assert!(!matches("level != 'error'", event.clone()));
        assert!(!matches("NOT (level = 'error')", event.clone()));
        assert!(!matches("absent = 1", event.clone()));
        assert!(matches("level IS NULL", event.clone()));
        assert!(matches("absent IS NULL", event.clone()));
        assert!(!matches("level IS NOT NULL", event));
    }

    #[test]
    fn boolean_connectives_follow_three_valued_logic() {
        let event = json!({"a": 1});
        // 未知 OR 真 = 真；未知 AND 真 = 未知（不匹配）；未知 AND 假 = 假，其取反为真。
        assert!(matches("missing = 1 OR a = 1", event.clone()));
        assert!(!matches("missing = 1 AND a = 1", event.clone()));
        assert!(matches("NOT (missing = 1 AND a = 2)", event.clone()));
        assert!(matches("NOT (a = 2)", event));
    }

    #[test]
    fn supports_in_between_and_like() {
        let event = json!({"level": "warn", "latency": 20, "message": "Disk Full on /var"});
        assert!(matches("level IN ('warn', 'error')", event.clone()));
        assert!(matches("level NOT IN ('info')", event.clone()));
        assert!(matches("latency BETWEEN 10 AND 20", event.clone()));
        assert!(!matches("latency NOT BETWEEN 10 AND 20", event.clone()));
        assert!(matches("message LIKE 'Disk%'", event.clone()));
        assert!(matches("message LIKE '%Full%/var'", event.clone()));
        assert!(!matches("message LIKE 'disk%'", event.clone()));
        assert!(matches("message ILIKE 'disk%'", event.clone()));
        assert!(matches("message LIKE 'Disk Full on /va_'", event.clone()));
        assert!(matches("message NOT LIKE 'x%'", event));
    }

    #[test]
    fn dotted_field_names_are_looked_up_whole() {
        let event = json!({"service.name": "checkout"});
        assert!(matches("\"service.name\" = 'checkout'", event.clone()));
        assert!(matches("service.name = 'checkout'", event));
    }

    #[test]
    fn like_wildcards_backtrack() {
        let pattern = |text: &str| text.chars().collect::<Vec<_>>();
        assert!(like_matches("abcabc", &pattern("%abc"), false));
        assert!(like_matches("aXbXc", &pattern("a%b%c"), false));
        assert!(!like_matches("abc", &pattern("a%d"), false));
        assert!(like_matches("", &pattern("%"), false));
        assert!(!like_matches("", &pattern("_"), false));
    }

    #[test]
    fn rejects_conditions_the_matcher_cannot_run() {
        for condition in [
            "lower(level) = 'error'",
            "level = other_field",
            "level + 1 = 2",
            "level IN (SELECT 1)",
            "level LIKE 'a' ESCAPE '!'",
            "level = NULL",
            "level SIMILAR TO 'x'",
        ] {
            assert!(compile(condition).is_err(), "must be rejected: {condition}");
        }
    }
}
