// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A statement with values filled in: `{{labels.host}}` stands for a value of an alert event, and
//! those values can come from outside (an external alert source writes the labels). So a placeholder
//! may stand only inside a string of the statement, where its value is escaped as that string
//! needs, and in SQL the filled statement must read token for token as the statement it came from.

use std::ops::Range;

use sqlparser::{
    ast::Statement,
    dialect::GenericDialect,
    parser::Parser,
    tokenizer::{Token, Tokenizer},
};

use crate::{
    domain::query::QueryLanguage,
    shared::{Error, Result},
};

/// One placeholder in a statement.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Hole {
    range: Range<usize>,
    path: String,
}

/// What a placeholder is filled with to read a statement's structure.
const SAMPLE_VALUE: &str = "x";

/// The statement with every placeholder filled by a sample value, after checking that each one
/// stands inside a string and passes `known`; a SQL statement must be a single query. What the sample
/// shows is what a caller can check further: the tables it names, its syntax.
pub fn template_sample(
    language: QueryLanguage,
    statement: &str,
    known: impl Fn(&str) -> bool,
) -> Result<String> {
    let holes = holes(language, statement)?;

    if let Some(hole) = holes.iter().find(|hole| !known(&hole.path)) {
        return Err(Error::invalid(format!(
            "`{{{{{}}}}}` is not a value of the event",
            hole.path
        )));
    }

    let sample = filled(language, statement, &holes, |_| Some(SAMPLE_VALUE.into()))?;

    if language == QueryLanguage::Sql {
        let statements = Parser::parse_sql(&GenericDialect, &sample)
            .map_err(|error| Error::invalid(format!("the statement cannot be read: {error}")))?;

        if !matches!(statements.as_slice(), [Statement::Query(_)]) {
            return Err(Error::invalid("the statement must be a single query"));
        }
    }

    Ok(sample)
}

/// The statement with each placeholder replaced by the value `value_of` gives, escaped for the string
/// it stands in. A placeholder with no value is an error: a query with an empty string in it would
/// answer a different question.
pub fn fill_template(
    language: QueryLanguage,
    statement: &str,
    value_of: impl Fn(&str) -> Option<String>,
) -> Result<String> {
    let holes = holes(language, statement)?;
    let filled_in = filled(language, statement, &holes, value_of)?;

    if language == QueryLanguage::Sql {
        let sample = filled(language, statement, &holes, |_| Some(SAMPLE_VALUE.into()))?;

        if !same_shape(&sample, &filled_in)? {
            return Err(Error::invalid(
                "a value of the event would change what the statement says",
            ));
        }
    }

    Ok(filled_in)
}

fn filled(
    language: QueryLanguage,
    statement: &str,
    holes: &[Hole],
    value_of: impl Fn(&str) -> Option<String>,
) -> Result<String> {
    let mut out = String::with_capacity(statement.len());
    let mut position = 0;

    for hole in holes {
        let value = value_of(&hole.path)
            .ok_or_else(|| Error::invalid(format!("the event has no `{}`", hole.path)))?;

        out.push_str(&statement[position..hole.range.start]);

        match language {
            QueryLanguage::Sql => push_sql(&mut out, &value),
            QueryLanguage::Promql => push_promql(&mut out, &value),
        }

        position = hole.range.end;
    }

    out.push_str(&statement[position..]);
    Ok(out)
}

/// A value inside a single-quoted SQL string. Doubling the backslash as well keeps it inside the
/// string whether or not the dialect reads a backslash as an escape.
fn push_sql(out: &mut String, value: &str) {
    for c in value.chars() {
        match c {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            '\0' => {}
            c => out.push(c),
        }
    }
}

/// A value inside a double- or single-quoted PromQL string.
fn push_promql(out: &mut String, value: &str) {
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
}

/// Whether two statements read as the same tokens, the contents of their strings aside.
fn same_shape(first: &str, second: &str) -> Result<bool> {
    let tokens = |sql: &str| {
        Tokenizer::new(&GenericDialect, sql)
            .tokenize()
            .map_err(|error| Error::invalid(format!("the statement cannot be read: {error}")))
    };

    let (first, second) = (tokens(first)?, tokens(second)?);

    let string = |token: &Token| {
        matches!(
            token,
            Token::SingleQuotedString(_)
                | Token::EscapedStringLiteral(_)
                | Token::NationalStringLiteral(_)
        )
    };

    Ok(first.len() == second.len()
        && first.iter().zip(&second).all(|(a, b)| {
            std::mem::discriminant(a) == std::mem::discriminant(b) && (string(a) || a == b)
        }))
}

/// The placeholders of a statement, in order. One outside a string is refused.
fn holes(language: QueryLanguage, statement: &str) -> Result<Vec<Hole>> {
    let bytes = statement.as_bytes();
    let mut holes = Vec::new();
    let mut at = 0;

    // Where a placeholder may stand, and where the scan is now.
    #[derive(PartialEq)]
    enum Place {
        Code,
        String(u8),
        Other(u8),
        LineComment,
        BlockComment(u32),
    }

    let mut place = Place::Code;

    while at < bytes.len() {
        let byte = bytes[at];
        let next = bytes.get(at + 1).copied();

        if byte == b'{' && next == Some(b'{') {
            let Some(length) = statement[at + 2..].find("}}") else {
                return Err(Error::invalid("a `{{` in the statement is never closed"));
            };

            let path = statement[at + 2..at + 2 + length].trim();

            if !matches!(place, Place::String(_)) {
                return Err(Error::invalid(format!(
                    "`{{{{{path}}}}}` must stand inside a string of the statement, where its value \
                     is escaped: write it between quotes"
                )));
            }

            holes.push(Hole {
                range: at..at + 2 + length + 2,
                path: path.to_owned(),
            });

            at += 2 + length + 2;
            continue;
        }

        match place {
            Place::Code => match (language, byte) {
                (_, b'\'') => place = Place::String(b'\''),
                (QueryLanguage::Promql, b'"') => place = Place::String(b'"'),
                (QueryLanguage::Sql, b'"' | b'`') => place = Place::Other(byte),
                (QueryLanguage::Promql, b'`') => place = Place::Other(byte),
                (QueryLanguage::Sql, b'-') if next == Some(b'-') => place = Place::LineComment,
                (QueryLanguage::Sql, b'/') if next == Some(b'*') => {
                    place = Place::BlockComment(1);
                    at += 1;
                }
                (QueryLanguage::Promql, b'#') => place = Place::LineComment,
                _ => {}
            },
            Place::String(quote) => match language {
                // A doubled quote stays in the string.
                QueryLanguage::Sql if byte == quote && next == Some(quote) => at += 1,
                QueryLanguage::Promql if byte == b'\\' => at += 1,
                _ if byte == quote => place = Place::Code,
                _ => {}
            },
            Place::Other(quote) => {
                if byte == quote {
                    place = Place::Code;
                }
            }
            Place::LineComment => {
                if byte == b'\n' {
                    place = Place::Code;
                }
            }
            Place::BlockComment(depth) => {
                if byte == b'*' && next == Some(b'/') {
                    at += 1;
                    place = if depth == 1 {
                        Place::Code
                    } else {
                        Place::BlockComment(depth - 1)
                    };
                } else if byte == b'/' && next == Some(b'*') {
                    at += 1;
                    place = Place::BlockComment(depth + 1);
                }
            }
        }

        at += 1;
    }

    if matches!(place, Place::String(_) | Place::Other(_)) {
        return Err(Error::invalid("a quote in the statement is never closed"));
    }

    Ok(holes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sql(statement: &str, value: &str) -> Result<String> {
        fill_template(QueryLanguage::Sql, statement, |_| Some(value.to_owned()))
    }

    fn promql(statement: &str, value: &str) -> Result<String> {
        fill_template(QueryLanguage::Promql, statement, |_| Some(value.to_owned()))
    }

    const BY_HOST: &str = "SELECT count(*) FROM logs WHERE host = '{{labels.host}}'";

    #[test]
    fn a_value_goes_into_its_string() {
        assert_eq!(
            sql(BY_HOST, "web-01").unwrap(),
            "SELECT count(*) FROM logs WHERE host = 'web-01'"
        );
        assert_eq!(
            sql("SELECT 1 FROM logs WHERE m LIKE '%{{ summary }}%'", "disk").unwrap(),
            "SELECT 1 FROM logs WHERE m LIKE '%disk%'"
        );
        assert_eq!(
            promql("up{instance=\"{{labels.host}}\"}", "web-01:9100").unwrap(),
            "up{instance=\"web-01:9100\"}"
        );
    }

    #[test]
    fn a_value_cannot_leave_its_string() {
        // Whatever the value holds, it stays one string of the one statement.
        for value in [
            "' OR 1=1 --",
            "\\' OR 1=1 --",
            "x'; DROP TABLE logs; --",
            "''' UNION SELECT * FROM _audit --",
            "a\nb",
        ] {
            let filled = sql(BY_HOST, value).unwrap();
            let statements = Parser::parse_sql(&GenericDialect, &filled).unwrap();

            assert_eq!(statements.len(), 1, "{filled}");
            assert!(same_shape(BY_HOST, &filled).unwrap(), "{filled}");
            assert!(!crate::statement_names_table(&filled, "_audit"), "{filled}");
        }

        assert_eq!(
            promql("up{a=\"{{x}}\"}", "\"} or up{b=\"c").unwrap(),
            "up{a=\"\\\"} or up{b=\\\"c\"}"
        );
        assert_eq!(
            promql("up{a='{{x}}'}", "'\n\\").unwrap(),
            "up{a='\\'\\n\\\\'}"
        );
    }

    #[test]
    fn a_placeholder_stands_only_inside_a_string() {
        let refused = |language, statement: &str| {
            template_sample(language, statement, |_| true).expect_err(statement)
        };

        for statement in [
            "SELECT 1 FROM logs WHERE host = {{labels.host}}",
            "SELECT 1 FROM {{labels.table}}",
            "SELECT 1 FROM \"{{labels.table}}\"",
            "SELECT 1 FROM logs -- '{{labels.host}}'",
            "SELECT 1 FROM logs /* '{{labels.host}}' */",
            // The quote is in a comment: the placeholder is not in a string.
            "SELECT 1 FROM logs -- it's\nWHERE host = {{labels.host}}",
        ] {
            refused(QueryLanguage::Sql, statement);
        }

        for statement in ["up{a={{x}}}", "up{a=`{{x}}`}", "up # \"\nand {{x}}"] {
            refused(QueryLanguage::Promql, statement);
        }

        assert!(
            refused(QueryLanguage::Sql, "SELECT 1 FROM logs WHERE a = '{{x")
                .to_string()
                .contains("never closed")
        );
    }

    #[test]
    fn a_doubled_quote_stays_in_the_string_and_a_comment_ends_where_it_ends() {
        assert_eq!(
            sql("SELECT 1 FROM l WHERE a = 'it''s {{x}}'", "y").unwrap(),
            "SELECT 1 FROM l WHERE a = 'it''s y'"
        );
        assert_eq!(
            sql("SELECT 1 /* it's */ FROM l WHERE a = '{{x}}'", "y").unwrap(),
            "SELECT 1 /* it's */ FROM l WHERE a = 'y'"
        );
        assert_eq!(
            promql("up{a=\"\\\"{{x}}\"}", "y").unwrap(),
            "up{a=\"\\\"y\"}"
        );
    }

    #[test]
    fn a_saved_statement_is_one_query_of_values_the_event_has() {
        let known = |path: &str| path.starts_with("labels.");

        assert_eq!(
            template_sample(QueryLanguage::Sql, BY_HOST, known).unwrap(),
            "SELECT count(*) FROM logs WHERE host = 'x'"
        );

        for statement in [
            "SELECT 1 FROM logs WHERE a = '{{fingerprint}}'",
            "DELETE FROM logs WHERE a = '{{labels.a}}'",
            "SELECT 1; SELECT 2",
            "SELECT 'a{{labels.a}}' FROM (",
        ] {
            assert!(
                template_sample(QueryLanguage::Sql, statement, known).is_err(),
                "{statement}"
            );
        }
    }

    #[test]
    fn a_placeholder_with_no_value_is_an_error() {
        let error = fill_template(QueryLanguage::Sql, BY_HOST, |_| None).expect_err("no value");

        assert!(error.to_string().contains("labels.host"));
    }
}
