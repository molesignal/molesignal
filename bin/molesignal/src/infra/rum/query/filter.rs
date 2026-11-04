// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The filters of the explorer as SQL conditions.
//!
//! A filter names a column of the stream (the columns are fixed, see `domain::rum::event`), an
//! operator and a value. The column decides what the value is read as and which operators make
//! sense for it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    member::{Column, resolved},
    sql::{ident, literal},
};
use crate::{
    domain::{rum::event::RumStream, stream::FieldType},
    shared::{Error, Result},
};

const MAX_FILTERS: usize = 32;
const MAX_VALUE_LEN: usize = 512;
const MAX_LIST_LEN: usize = 4096;
const MAX_LIST_ITEMS: usize = 100;
const MAX_TEXT_TERMS: usize = 8;

/// Columns that name an entity and never change between its revisions. A condition on them may be
/// asked before the latest revision of a view or a session is taken.
const IDENTITY: &[&str] = &["session.id", "view.id", "application.id"];

/// The columns that say what kind of session a row belongs to, and what drove it if no person did.
const TRAFFIC_COLUMNS: [&str; 4] = [
    "session.type",
    "synthetics.test_id",
    "synthetics.result_id",
    "ci_test.test_execution_id",
];

/// A filter as the explorer sends it. `quoted` is accepted so that the filter builders of the
/// other explorers can be used as they are.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Filter {
    pub field: String,
    pub op: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub quoted: bool,
}

/// A condition rendered as SQL, with the columns it reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    pub sql: String,
    pub columns: Vec<String>,
    /// Holds for every revision of an entity or for none (see [`IDENTITY`]).
    pub identity: bool,
}

impl Condition {
    pub fn raw(sql: &str, columns: &[&str], identity: bool) -> Self {
        Self {
            sql: sql.to_string(),
            columns: columns.iter().map(|name| (*name).to_string()).collect(),
            identity,
        }
    }

    /// `column = value` on a text column, as the lookup of one entity.
    pub fn equals(column: &str, value: &str) -> Self {
        Self {
            sql: format!("{} = {}", ident(column), literal(value)),
            columns: vec![column.to_string()],
            identity: IDENTITY.contains(&column),
        }
    }
}

/// The rows of the sessions people ran, which is what a figure of an application is about: a session
/// a test drove does not count unless it is asked for. A session stays the kind it began as, so the
/// condition may be asked before the latest revision is taken.
pub fn people_only() -> Condition {
    Condition::raw(
        "(\"session.type\" IS NULL OR \"session.type\" = 'user')",
        &["session.type"],
        true,
    )
}

/// Whether filters ask about the kind of a session, about what drove it, or for sessions by name.
/// Then they are taken at their word: a reader who looks for a run of a check is not told it was no
/// person's.
pub fn asks_about_traffic(filters: &[Filter]) -> bool {
    filters.iter().any(|filter| {
        let name = filter.field.trim();

        TRAFFIC_COLUMNS.contains(&name)
            || name.starts_with("synthetics.")
            || name.starts_with("ci_test.")
            || (name == "session.id" && matches!(Op::parse(&filter.op), Ok(Op::Eq | Op::In)))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Eq,
    Ne,
    In,
    NotIn,
    Contains,
    Gt,
    Gte,
    Lt,
    Lte,
    Exists,
    Missing,
}

impl Op {
    fn parse(op: &str) -> Result<Self> {
        Ok(match op.trim().to_ascii_lowercase().as_str() {
            "=" | "eq" => Self::Eq,
            "!=" | "ne" => Self::Ne,
            "in" => Self::In,
            "not_in" | "nin" => Self::NotIn,
            "contains" | "like" | "match" | "match_text" => Self::Contains,
            ">" | "gt" => Self::Gt,
            ">=" | "gte" => Self::Gte,
            "<" | "lt" => Self::Lt,
            "<=" | "lte" => Self::Lte,
            "exists" => Self::Exists,
            "missing" | "not_exists" => Self::Missing,
            other => {
                return Err(Error::invalid(format!(
                    "unsupported filter operator `{other}`"
                )));
            }
        })
    }

    fn compares_order(self) -> bool {
        matches!(self, Self::Gt | Self::Gte | Self::Lt | Self::Lte)
    }

    fn symbol(self) -> &'static str {
        match self {
            Self::Gt => ">",
            Self::Gte => ">=",
            Self::Lt => "<",
            Self::Lte => "<=",
            Self::Ne => "<>",
            _ => "=",
        }
    }
}

/// The conditions the filters of a request stand for. A filter that names no column of the
/// stream, or asks of a column what it cannot answer, fails the request.
pub fn conditions(stream: RumStream, filters: &[Filter]) -> Result<Vec<Condition>> {
    if filters.len() > MAX_FILTERS {
        return Err(Error::invalid("too many filters"));
    }

    filters
        .iter()
        .map(|filter| {
            let column = resolved(stream, filter.field.trim())?;
            let op = Op::parse(&filter.op)?;
            let sql = render(&column, op, &filter.value)?;

            Ok(Condition {
                sql,
                columns: vec![column.stored_name().to_string()],
                identity: IDENTITY.contains(&column.name())
                    && matches!(op, Op::Eq | Op::In)
                    && column.data_type() == FieldType::Utf8,
            })
        })
        .collect()
}

/// Free text: every term must be found in at least one of `columns`.
pub fn text_conditions(columns: &[&str], terms: &[String]) -> Result<Vec<Condition>> {
    if terms.len() > MAX_TEXT_TERMS {
        return Err(Error::invalid("too many search terms"));
    }

    terms
        .iter()
        .filter(|term| !term.trim().is_empty())
        .map(|term| {
            let term = checked(term.trim())?;
            let pattern = literal(&format!("%{}%", escape_like(term)));

            let any = columns
                .iter()
                .map(|name| format!("CAST({} AS VARCHAR) ILIKE {pattern}", ident(name)))
                .collect::<Vec<_>>()
                .join(" OR ");

            Ok(Condition {
                sql: format!("({any})"),
                columns: columns.iter().map(|name| (*name).to_string()).collect(),
                identity: false,
            })
        })
        .collect()
}

fn render(column: &Column, op: Op, value: &str) -> Result<String> {
    // A key of a JSON column is compared as a number when the request asks which is larger.
    let member_order = matches!(column, Column::Member { .. }) && op.compares_order();

    let (name, kind) = if member_order {
        (column.number(), FieldType::Float64)
    } else {
        match (column, column.data_type()) {
            (Column::Member { .. }, FieldType::Int64) => (column.number(), FieldType::Float64),
            (_, kind) => (column.value(), kind),
        }
    };

    match op {
        Op::Exists => return Ok(presence(&name, kind, true)),
        Op::Missing => return Ok(presence(&name, kind, false)),
        // An empty value is no value: `env = ''` asks for the rows without one, as in the other
        // explorers, and `env != ''` for the rows with one.
        Op::Eq if value.trim().is_empty() => return Ok(presence(&name, kind, false)),
        Op::Ne if value.trim().is_empty() => return Ok(presence(&name, kind, true)),
        _ => {}
    }

    let allowed = match kind {
        FieldType::Utf8 => !op.compares_order(),
        FieldType::Int64 | FieldType::Float64 | FieldType::Timestamp => op != Op::Contains,
        FieldType::Bool => matches!(op, Op::Eq | Op::Ne),
        FieldType::Json => op == Op::Contains,
    };

    if !allowed {
        return Err(Error::invalid(format!(
            "`{}` cannot be filtered that way",
            column.name()
        )));
    }

    match op {
        Op::In | Op::NotIn => {
            let items = list_items(value)?;

            let rendered = items
                .iter()
                .map(|item| scalar(kind, item))
                .collect::<Result<Vec<_>>>()?
                .join(", ");

            let keyword = if op == Op::In { "IN" } else { "NOT IN" };

            Ok(if op == Op::In {
                format!("{name} {keyword} ({rendered})")
            } else {
                format!("({name} IS NULL OR {name} {keyword} ({rendered}))")
            })
        }
        Op::Contains => {
            let value = checked(value.trim())?;

            if value.is_empty() {
                return Err(Error::invalid("a text filter needs a value"));
            }

            Ok(format!(
                "CAST({name} AS VARCHAR) ILIKE {}",
                literal(&format!("%{}%", escape_like(value)))
            ))
        }
        Op::Eq | Op::Ne if kind == FieldType::Utf8 && value.contains('*') => {
            let pattern = literal(&wildcard(checked(value)?));

            Ok(if op == Op::Eq {
                format!("{name} LIKE {pattern}")
            } else {
                format!("({name} IS NULL OR {name} NOT LIKE {pattern})")
            })
        }
        Op::Ne => {
            let operand = scalar(kind, &Value::String(checked(value)?.to_string()))?;

            Ok(format!("({name} IS NULL OR {name} <> {operand})"))
        }
        _ => {
            let operand = scalar(kind, &Value::String(checked(value)?.to_string()))?;

            Ok(format!("{name} {} {operand}", op.symbol()))
        }
    }
}

fn presence(name: &str, kind: FieldType, exists: bool) -> String {
    match (kind, exists) {
        (FieldType::Utf8, true) => format!("({name} IS NOT NULL AND {name} <> '')"),
        (FieldType::Utf8, false) => format!("({name} IS NULL OR {name} = '')"),
        (_, true) => format!("{name} IS NOT NULL"),
        (_, false) => format!("{name} IS NULL"),
    }
}

/// A value as a literal of the column type.
fn scalar(kind: FieldType, value: &Value) -> Result<String> {
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        _ => {
            return Err(Error::invalid(
                "a filter value must be text, a number or a flag",
            ));
        }
    };

    let text = checked(text.trim())?;

    match kind {
        FieldType::Utf8 => Ok(literal(text)),
        FieldType::Int64 | FieldType::Timestamp => text
            .parse::<i64>()
            .map(|number| number.to_string())
            .or_else(|_| {
                text.parse::<f64>()
                    .ok()
                    .filter(|number| number.is_finite())
                    .map(|number| number.to_string())
                    .ok_or(())
            })
            .map_err(|()| Error::invalid(format!("`{text}` is not a number"))),
        FieldType::Float64 => text
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
            .map(|number| number.to_string())
            .ok_or_else(|| Error::invalid(format!("`{text}` is not a number"))),
        FieldType::Bool => match text.to_ascii_lowercase().as_str() {
            "true" => Ok("TRUE".into()),
            "false" => Ok("FALSE".into()),
            _ => Err(Error::invalid(format!("`{text}` is not true or false"))),
        },
        FieldType::Json => Ok(literal(text)),
    }
}

fn list_items(value: &str) -> Result<Vec<Value>> {
    if value.len() > MAX_LIST_LEN {
        return Err(Error::invalid("the list of values is too long"));
    }

    let items = serde_json::from_str::<Vec<Value>>(value)
        .map_err(|_| Error::invalid("`in` takes a JSON list of values"))?;

    if items.is_empty() || items.len() > MAX_LIST_ITEMS {
        return Err(Error::invalid(format!(
            "`in` takes between 1 and {MAX_LIST_ITEMS} values"
        )));
    }

    Ok(items)
}

fn checked(value: &str) -> Result<&str> {
    if value.len() > MAX_VALUE_LEN || value.contains('\0') {
        return Err(Error::invalid("a filter value is too long or not text"));
    }

    Ok(value)
}

/// `%`, `_` and the escape character as plain characters of a `LIKE` pattern.
fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// A value with `*` for "anything" as a `LIKE` pattern.
fn wildcard(value: &str) -> String {
    value
        .split('*')
        .map(escape_like)
        .collect::<Vec<_>>()
        .join("%")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(field: &str, op: &str, value: &str) -> Filter {
        Filter {
            field: field.into(),
            op: op.into(),
            value: value.into(),
            quoted: false,
        }
    }

    fn sql(stream: RumStream, field: &str, op: &str, value: &str) -> String {
        conditions(stream, &[filter(field, op, value)])
            .unwrap()
            .remove(0)
            .sql
    }

    #[test]
    fn a_filter_for_named_sessions_reads_them_whatever_ran_them() {
        assert!(asks_about_traffic(&[filter("session.id", "=", "a")]));
        assert!(asks_about_traffic(&[filter("session.id", "in", "[\"a\"]")]));
        assert!(!asks_about_traffic(&[filter("session.id", "!=", "a")]));
        assert!(!asks_about_traffic(&[filter("session.id", "exists", "")]));
        assert!(!asks_about_traffic(&[filter("env", "=", "prod")]));
    }

    #[test]
    fn text_columns_compare_as_text() {
        assert_eq!(
            sql(RumStream::Views, "env", "=", "prod"),
            "\"env\" = 'prod'"
        );
        assert_eq!(
            sql(RumStream::Views, "env", "!=", "it's"),
            "(\"env\" IS NULL OR \"env\" <> 'it''s')"
        );
        assert_eq!(
            sql(RumStream::Views, "env", "in", r#"["prod","staging"]"#),
            "\"env\" IN ('prod', 'staging')"
        );
        assert_eq!(
            sql(RumStream::Views, "env", "not_in", r#"["dev"]"#),
            "(\"env\" IS NULL OR \"env\" NOT IN ('dev'))"
        );
    }

    #[test]
    fn contains_escapes_the_pattern_characters() {
        assert_eq!(
            sql(RumStream::Resources, "resource.url", "contains", "50%_off"),
            "CAST(\"resource.url\" AS VARCHAR) ILIKE '%50\\%\\_off%'"
        );
    }

    #[test]
    fn a_star_in_text_means_anything() {
        assert_eq!(
            sql(RumStream::Views, "view.url_path", "=", "/users/*/orders"),
            "\"view.url_path\" LIKE '/users/%/orders'"
        );
        assert_eq!(
            sql(RumStream::Views, "view.url_path", "!=", "/admin*"),
            "(\"view.url_path\" IS NULL OR \"view.url_path\" NOT LIKE '/admin%')"
        );
    }

    #[test]
    fn numbers_compare_as_numbers() {
        assert_eq!(
            sql(RumStream::Resources, "resource.status_code", ">=", "500"),
            "\"resource.status_code\" >= 500"
        );
        assert_eq!(
            sql(RumStream::Views, "view.performance.cls.score", "<", "0.1"),
            "\"view.performance.cls.score\" < 0.1"
        );
        assert_eq!(
            sql(
                RumStream::Resources,
                "resource.status_code",
                "in",
                "[404, \"500\"]"
            ),
            "\"resource.status_code\" IN (404, 500)"
        );
        assert!(
            conditions(
                RumStream::Resources,
                &[filter("resource.status_code", "=", "ok")]
            )
            .is_err()
        );
    }

    #[test]
    fn flags_compare_as_true_or_false() {
        assert_eq!(
            sql(RumStream::Sessions, "session.has_replay", "=", "true"),
            "\"session.has_replay\" = TRUE"
        );
        assert!(
            conditions(
                RumStream::Sessions,
                &[filter("session.has_replay", ">", "1")]
            )
            .is_err()
        );
    }

    #[test]
    fn an_empty_value_asks_for_the_rows_without_one() {
        assert_eq!(
            sql(RumStream::Views, "env", "=", ""),
            "(\"env\" IS NULL OR \"env\" = '')"
        );
        assert_eq!(
            sql(RumStream::Views, "env", "!=", "  "),
            "(\"env\" IS NOT NULL AND \"env\" <> '')"
        );
        assert_eq!(
            sql(RumStream::Resources, "resource.status_code", "=", ""),
            "\"resource.status_code\" IS NULL"
        );
        assert_eq!(
            sql(RumStream::Views, "env", "match_text", "pro"),
            sql(RumStream::Views, "env", "contains", "pro")
        );
    }

    #[test]
    fn presence_is_asked_with_exists_and_missing() {
        assert_eq!(
            sql(RumStream::Views, "usr.id", "exists", ""),
            "(\"usr.id\" IS NOT NULL AND \"usr.id\" <> '')"
        );
        assert_eq!(
            sql(
                RumStream::Views,
                "view.performance.lcp.timestamp",
                "missing",
                ""
            ),
            "\"view.performance.lcp.timestamp\" IS NULL"
        );
    }

    #[test]
    fn a_json_column_can_only_be_searched_or_checked_for_presence() {
        assert!(sql(RumStream::Views, "context", "contains", "pro").contains("ILIKE"));
        assert!(conditions(RumStream::Views, &[filter("context", "=", "x")]).is_err());
        assert!(conditions(RumStream::Views, &[filter("context", "exists", "")]).is_ok());
    }

    #[test]
    fn unknown_fields_and_operators_are_refused() {
        assert!(conditions(RumStream::Views, &[filter("nope", "=", "x")]).is_err());
        assert!(conditions(RumStream::Views, &[filter("env", "matches", "x")]).is_err());
        assert!(conditions(RumStream::Views, &[filter("env", "=", "a\0b")]).is_err());
        assert!(conditions(RumStream::Views, &[filter("env", "in", "prod")]).is_err());
    }

    #[test]
    fn only_the_identity_of_an_entity_may_be_asked_early() {
        let all = conditions(
            RumStream::Views,
            &[
                filter("view.id", "=", "v-1"),
                filter("session.id", "in", r#"["s-1"]"#),
                filter("view.id", "!=", "v-2"),
                filter("env", "=", "prod"),
            ],
        )
        .unwrap();

        let identity = all
            .iter()
            .map(|condition| condition.identity)
            .collect::<Vec<_>>();

        assert_eq!(identity, vec![true, true, false, false]);
    }

    #[test]
    fn free_text_must_match_every_term_somewhere() {
        let found = text_conditions(
            &["usr.email", "view.url"],
            &["ada".into(), "  ".into(), "cart".into()],
        )
        .unwrap();

        assert_eq!(found.len(), 2);
        assert_eq!(
            found[0].sql,
            "(CAST(\"usr.email\" AS VARCHAR) ILIKE '%ada%' OR CAST(\"view.url\" AS VARCHAR) ILIKE '%ada%')"
        );
        assert!(text_conditions(&["usr.email"], &vec!["x".to_string(); 9]).is_err());
    }

    #[test]
    fn the_equality_of_an_entity_lookup_is_identity() {
        assert!(Condition::equals("view.id", "v-1").identity);
        assert!(!Condition::equals("env", "prod").identity);
    }
}
