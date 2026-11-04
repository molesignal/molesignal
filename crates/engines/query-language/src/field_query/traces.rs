// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A field query read the way the Traces page reads it: what all holds as the
//! list of filters and one phrase of free text, alternatives as a condition
//! tree. Whether a field exists and takes the operator is checked against the
//! stream by the reads, as for filters sent as a list. Kept in step with
//! `parseTraceStatement` in `web/src/routes/traces/fieldQueryModel.ts`.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use super::expression::{FieldExpression, has_top_level_or, parse_field_expression, quoted_text};
use crate::shared::{Error, Result};

/// The most values one `in` list holds, as the trace reads take it.
pub const MAX_TRACE_IN_VALUES: usize = 64;

/// One comparison of a trace query: a value, or for `in` a list of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TraceClause {
    pub field: String,
    /// `=`, `!=`, `>`, `>=`, `<`, `<=`, `contains` or `in`.
    pub op: String,
    /// Empty for `in`, which holds for any of `values`.
    pub value: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
}

/// What a group of alternatives says, as the trace reads take it under `where`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum TraceCondition {
    Clause(TraceClause),
    Text { text: String },
    And { and: Vec<TraceCondition> },
    Or { or: Vec<TraceCondition> },
}

/// A trace query as the trace reads take it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TraceMatch {
    /// The text to find, one phrase.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    pub filters: Vec<TraceClause>,
    #[serde(rename = "where", skip_serializing_if = "Option::is_none")]
    pub condition: Option<TraceCondition>,
}

enum TracePart {
    Clause(TraceClause),
    Text(String),
    Rejected,
}

static COMPARISON: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^([A-Za-z_][A-Za-z0-9_.]*)\s*(>=|<=|!=|=|>|<|eq\b|ne\b|contains\b|like\b)\s*([\s\S]*)$",
    )
    .expect("comparison pattern")
});

static IN_LIST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^([A-Za-z_][A-Za-z0-9_.]*)\s+in\s*\(([\s\S]*)\)$").expect("in pattern")
});

/// Reads a field query into what a trace read matches. A part the query
/// cannot read refuses the whole query and is named in the error.
pub fn read_trace_query(statement: &str) -> Result<TraceMatch> {
    let parts = match parse_field_expression(statement) {
        None => Vec::new(),
        Some(FieldExpression::And(items)) => items,
        Some(other) => vec![other],
    };

    let mut read = TraceMatch::default();
    let mut free_text = Vec::new();
    let mut alternatives = Vec::new();
    let mut rejected = Vec::new();

    for part in parts {
        match part {
            FieldExpression::Part(text) => match read_part(&text) {
                TracePart::Clause(clause) => read.filters.push(clause),
                TracePart::Text(value) => free_text.push(value),
                TracePart::Rejected => rejected.push(text),
            },
            group => match condition_of(&group) {
                Some(condition) => alternatives.push(condition),
                None => rejected.push(written(&group)),
            },
        }
    }

    if !rejected.is_empty() {
        return Err(Error::invalid(format!(
            "the field query cannot read: {}",
            rejected.join(", ")
        )));
    }

    read.q = (!free_text.is_empty()).then(|| free_text.join(" "));
    read.condition = match alternatives.len() {
        0 => None,
        1 => alternatives.pop(),
        _ => Some(TraceCondition::And { and: alternatives }),
    };

    Ok(read)
}

/// A group as the trace reads take it; `None` when a part of it is one they
/// cannot read. The text of one group of parts that all hold is one phrase,
/// as the free text is.
fn condition_of(expression: &FieldExpression) -> Option<TraceCondition> {
    let (items, and) = match expression {
        FieldExpression::Part(text) => {
            return match read_part(text) {
                TracePart::Clause(clause) => Some(TraceCondition::Clause(clause)),
                TracePart::Text(text) => Some(TraceCondition::Text { text }),
                TracePart::Rejected => None,
            };
        }
        FieldExpression::And(items) => (items, true),
        FieldExpression::Or(items) => (items, false),
    };

    let mut read = Vec::with_capacity(items.len());
    let mut texts = Vec::new();

    for item in items {
        match condition_of(item)? {
            TraceCondition::Text { text } if and => texts.push(text),
            condition => read.push(condition),
        }
    }

    if !texts.is_empty() {
        read.push(TraceCondition::Text {
            text: texts.join(" "),
        });
    }

    if read.len() == 1 {
        return read.pop();
    }

    Some(if and {
        TraceCondition::And { and: read }
    } else {
        TraceCondition::Or { or: read }
    })
}

/// A group as it reads, for an error that names it.
fn written(expression: &FieldExpression) -> String {
    match expression {
        FieldExpression::Part(text) => text.clone(),
        FieldExpression::And(items) => {
            let parts: Vec<String> = items.iter().map(written).collect();
            format!("({})", parts.join(" AND "))
        }
        FieldExpression::Or(items) => {
            let parts: Vec<String> = items.iter().map(written).collect();
            format!("({})", parts.join(" OR "))
        }
    }
}

/// Reads one part: a comparison, a list of values, or text to find, a quoted
/// phrase as the words it holds. A part that opens a bracket it does not close
/// as a group, or compares with nothing, is rejected.
fn read_part(part: &str) -> TracePart {
    if part.starts_with('(') {
        return TracePart::Rejected;
    }

    let Some(clause) = clause_of(part) else {
        return match quoted_text(part) {
            None => TracePart::Text(part.to_string()),
            Some(phrase) if !phrase.trim().is_empty() => TracePart::Text(phrase),
            Some(_) => TracePart::Rejected,
        };
    };

    let empty = if clause.op == "in" {
        clause.values.is_empty() || clause.values.len() > MAX_TRACE_IN_VALUES
    } else {
        clause.value.is_empty()
    };

    if empty {
        TracePart::Rejected
    } else {
        TracePart::Clause(clause)
    }
}

/// One comparison read from a part: a field and a value, or a field and a
/// list of values; `None` for anything else.
fn clause_of(part: &str) -> Option<TraceClause> {
    // Alternatives are no one comparison, though the first of them reads like one.
    if has_top_level_or(part) {
        return None;
    }

    let trimmed = part.trim();

    if let Some(found) = IN_LIST.captures(trimmed) {
        let values = split_list(&found[2])
            .iter()
            .map(|value| unquote(value))
            .filter(|value| !value.is_empty())
            .collect();

        return Some(TraceClause {
            field: found[1].to_string(),
            op: "in".to_string(),
            value: String::new(),
            values,
        });
    }

    let found = COMPARISON.captures(trimmed)?;
    let op = match found[2].to_ascii_lowercase().as_str() {
        "eq" => "=".to_string(),
        "ne" => "!=".to_string(),
        "like" => "contains".to_string(),
        other => other.to_string(),
    };

    Some(TraceClause {
        field: found[1].to_string(),
        op,
        value: unquote(&found[3]),
        values: Vec::new(),
    })
}

/// The items of a list, split at the commas outside quotes, each trimmed.
fn split_list(input: &str) -> Vec<String> {
    let characters: Vec<char> = input.chars().collect();
    let mut items = Vec::new();
    let mut start = 0;
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for (index, character) in characters.iter().copied().enumerate() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if let Some(open) = quote {
            if character == open {
                quote = None;
            }
        } else if character == '\'' || character == '"' {
            quote = Some(character);
        } else if character == ',' {
            items.push(characters[start..index].iter().collect::<String>());
            start = index + 1;
        }
    }

    items.push(characters[start..].iter().collect());
    items
        .into_iter()
        .map(|item| item.trim().to_string())
        .collect()
}

/// A value as a field query writes it, bare or quoted, read back as it is: a
/// double-quoted value as a JSON string, a single-quoted one with its escaped
/// quotes and backslashes undone.
fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    let characters: Vec<char> = trimmed.chars().collect();

    if characters.len() >= 2 {
        let first = characters[0];
        let last = characters[characters.len() - 1];

        if first == '"' && last == '"' {
            return serde_json::from_str::<String>(trimmed)
                .unwrap_or_else(|_| characters[1..characters.len() - 1].iter().collect());
        }

        if first == '\'' && last == '\'' {
            return unescape_single(&characters[1..characters.len() - 1]);
        }
    }

    trimmed.to_string()
}

fn unescape_single(characters: &[char]) -> String {
    let mut read = String::with_capacity(characters.len());
    let mut index = 0;

    while index < characters.len() {
        let character = characters[index];

        if character == '\\'
            && let Some(next @ ('\'' | '\\')) = characters.get(index + 1).copied()
        {
            read.push(next);
            index += 2;
            continue;
        }

        read.push(character);
        index += 1;
    }

    read
}
