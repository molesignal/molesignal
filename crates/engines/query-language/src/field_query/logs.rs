// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A field query read the way the Logs page reads it: the parts that all hold
//! as lists of filters and words to find, alternatives as a condition tree.
//! Kept in step with `readLogFieldPart` and `logMatchRequest` in
//! `web/src/routes/logs/`.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use super::expression::{FieldExpression, parse_field_expression, quoted_text};
use crate::shared::{Error, Result};

/// One comparison or search function of a log query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogClause {
    pub field: String,
    /// `=`, `!=`, `>`, `>=`, `<`, `<=`, `contains`, `match` or `match_text`.
    pub op: String,
    pub value: String,
    /// The value was written in quotes: a word, never a number or a field.
    pub quoted: bool,
}

/// What a group of alternatives says, as the log reads take it under `where`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum LogCondition {
    Clause(LogClause),
    Text { text: String },
    And { and: Vec<LogCondition> },
    Or { or: Vec<LogCondition> },
}

/// A log query as the log reads take it: what all holds as the lists, the
/// alternatives as the condition tree beside them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogMatch {
    pub filters: Vec<LogClause>,
    pub free_text: Vec<String>,
    #[serde(rename = "where", skip_serializing_if = "Option::is_none")]
    pub condition: Option<LogCondition>,
}

enum LogPart {
    Clause(LogClause),
    Text(String),
    Rejected,
}

static FUNCTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(MATCH_TEXT|MATCH)\s*\(\s*([A-Za-z_][A-Za-z0-9_.]*)\s*,\s*([\s\S]*?)\s*\)$")
        .expect("function pattern")
});

static FUNCTION_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(MATCH_TEXT|MATCH)\b").expect("function name pattern"));

static COMPARISON: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^([A-Za-z_][A-Za-z0-9_.]*)\s*(>=|<=|!=|=|>|<|eq\b|ne\b|contains\b|like\b)\s*([\s\S]+)$",
    )
    .expect("comparison pattern")
});

/// Reads a field query into what a log read matches. A part the query cannot
/// read refuses the whole query and is named in the error.
pub fn read_log_query(statement: &str) -> Result<LogMatch> {
    let parts = match parse_field_expression(statement) {
        None => Vec::new(),
        Some(FieldExpression::And(items)) => items,
        Some(other) => vec![other],
    };

    let mut read = LogMatch::default();
    let mut alternatives = Vec::new();
    let mut rejected = Vec::new();

    for part in parts {
        match part {
            FieldExpression::Part(text) => match read_part(&text) {
                LogPart::Clause(clause) => read.filters.push(clause),
                LogPart::Text(value) => read.free_text.push(value),
                LogPart::Rejected => rejected.push(text),
            },
            group => {
                if let Some(condition) = condition_of(group, &mut rejected) {
                    alternatives.push(condition);
                }
            }
        }
    }

    if !rejected.is_empty() {
        return Err(Error::invalid(format!(
            "the field query cannot read: {}",
            rejected.join(", ")
        )));
    }

    read.condition = match alternatives.len() {
        0 => None,
        1 => alternatives.pop(),
        _ => Some(LogCondition::And { and: alternatives }),
    };

    Ok(read)
}

/// A group of alternatives as a condition tree; `None` where a part of it is
/// nothing the query can read, which `rejected` is told.
fn condition_of(expression: FieldExpression, rejected: &mut Vec<String>) -> Option<LogCondition> {
    let (items, and) = match expression {
        FieldExpression::Part(text) => {
            return match read_part(&text) {
                LogPart::Clause(clause) => Some(LogCondition::Clause(clause)),
                LogPart::Text(text) => Some(LogCondition::Text { text }),
                LogPart::Rejected => {
                    rejected.push(text);
                    None
                }
            };
        }
        FieldExpression::And(items) => (items, true),
        FieldExpression::Or(items) => (items, false),
    };

    let mut read = Vec::with_capacity(items.len());
    let mut complete = true;

    for item in items {
        match condition_of(item, rejected) {
            Some(condition) => read.push(condition),
            None => complete = false,
        }
    }

    complete.then_some(if and {
        LogCondition::And { and: read }
    } else {
        LogCondition::Or { or: read }
    })
}

/// Reads one part: a search function, a comparison, or text to find, a quoted
/// phrase as the words it holds. A part that opens a bracket it does not close
/// as a group is nothing the query can read.
fn read_part(part: &str) -> LogPart {
    if let Some(found) = FUNCTION.captures(part) {
        let (value, quoted) = unquote(&found[3]);

        if !quoted {
            return LogPart::Rejected;
        }

        return LogPart::Clause(LogClause {
            field: found[2].to_string(),
            op: found[1].to_ascii_lowercase(),
            value,
            quoted: true,
        });
    }

    if FUNCTION_NAME.is_match(part) || part.starts_with('(') {
        return LogPart::Rejected;
    }

    let Some(found) = COMPARISON.captures(part) else {
        return match quoted_text(part) {
            None => LogPart::Text(part.to_string()),
            Some(phrase) if !phrase.trim().is_empty() => LogPart::Text(phrase),
            Some(_) => LogPart::Rejected,
        };
    };

    let op = match found[2].to_ascii_lowercase().as_str() {
        "eq" => "=".to_string(),
        "ne" => "!=".to_string(),
        "like" => "contains".to_string(),
        other => other.to_string(),
    };
    let (value, quoted) = unquote(&found[3]);
    let presence = quoted && value.is_empty() && (op == "=" || op == "!=");

    if value.is_empty() && !presence {
        return LogPart::Rejected;
    }

    LogPart::Clause(LogClause {
        field: found[1].to_string(),
        op,
        value,
        quoted,
    })
}

/// A value as written: in matching quotes it is a word with its escapes read,
/// bare it is the text as it stands.
fn unquote(raw: &str) -> (String, bool) {
    let trimmed = raw.trim();
    let characters: Vec<char> = trimmed.chars().collect();

    if characters.len() >= 2 {
        let first = characters[0];
        let last = characters[characters.len() - 1];

        if (first == '\'' && last == '\'') || (first == '"' && last == '"') {
            return (decode_escapes(&characters[1..characters.len() - 1]), true);
        }
    }

    (trimmed.to_string(), false)
}

fn decode_escapes(characters: &[char]) -> String {
    let mut decoded = String::with_capacity(characters.len());
    let mut index = 0;

    while index < characters.len() {
        let character = characters[index];

        if character != '\\' || index == characters.len() - 1 {
            decoded.push(character);
            index += 1;
            continue;
        }

        let replacement = match characters[index + 1] {
            '\\' => Some('\\'),
            '\'' => Some('\''),
            '"' => Some('"'),
            'n' => Some('\n'),
            'r' => Some('\r'),
            't' => Some('\t'),
            _ => None,
        };

        match replacement {
            Some(replacement) => {
                decoded.push(replacement);
                index += 2;
            }
            None => {
                decoded.push(character);
                index += 1;
            }
        }
    }

    decoded
}
