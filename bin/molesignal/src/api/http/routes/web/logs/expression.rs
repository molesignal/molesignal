// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a log read matches, from its lists or from the field query a client
//! sends as text: the text is read as the Logs page reads it, into the same
//! filters, words to find and condition tree.

use query_language::field_query::{LogClause, read_log_query};
use serde_json::Value;

use super::{LogCondition, LogFilter, normalize_filters, normalize_free_text, read_condition};
use crate::shared::{Error, Result};

/// The longest field query a read takes, in bytes.
const MAX_EXPRESSION_LENGTH: usize = 8 * 1024;

/// The filters, words to find and condition tree a log read matches, checked
/// and normalized. `expression` stands for the other three: a request sends
/// either the text or the lists, never both.
pub(super) fn read_match(
    expression: Option<String>,
    filters: Vec<LogFilter>,
    free_text: Vec<String>,
    condition: Option<Value>,
) -> Result<(Vec<LogFilter>, Vec<String>, Option<LogCondition>)> {
    let (filters, free_text, condition) = match expression.filter(|text| !text.trim().is_empty()) {
        None => (filters, free_text, condition),
        Some(_) if !filters.is_empty() || !free_text.is_empty() || condition.is_some() => {
            return Err(Error::invalid(
                "`expression` stands for `filters`, `free_text` and `where`; send one or the other",
            ));
        }
        Some(text) => read_expression(&text)?,
    };

    let filters = normalize_filters(filters)?;
    let free_text = normalize_free_text(free_text)?;
    let condition = read_condition(condition, &filters, &free_text)?;

    Ok((filters, free_text, condition))
}

fn read_expression(text: &str) -> Result<(Vec<LogFilter>, Vec<String>, Option<Value>)> {
    if text.len() > MAX_EXPRESSION_LENGTH {
        return Err(Error::invalid(format!(
            "the field query is longer than {MAX_EXPRESSION_LENGTH} bytes"
        )));
    }

    let read = read_log_query(text)?;

    let condition = read
        .condition
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| Error::internal(format!("encode log condition: {error}")))?;

    Ok((
        read.filters.into_iter().map(LogFilter::from).collect(),
        read.free_text,
        condition,
    ))
}

impl From<LogClause> for LogFilter {
    fn from(clause: LogClause) -> Self {
        Self {
            field: clause.field,
            op: clause.op,
            value: clause.value,
            quoted: clause.quoted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_query_reads_into_the_lists_and_the_tree() {
        let (filters, free_text, condition) = read_match(
            Some("level = 'error' AND timeout AND (service.name = 'api' OR slow)".into()),
            Vec::new(),
            Vec::new(),
            None,
        )
        .expect("read");

        assert_eq!(filters.len(), 1);
        assert_eq!(
            (filters[0].field.as_str(), filters[0].value.as_str()),
            ("level", "error")
        );
        assert_eq!(free_text, vec!["timeout".to_string()]);
        assert!(condition.is_some());
    }

    #[test]
    fn the_text_and_the_lists_do_not_come_together() {
        let filter = LogFilter {
            field: "level".into(),
            op: "=".into(),
            value: "error".into(),
            quoted: true,
        };

        assert!(read_match(Some("timeout".into()), vec![filter], Vec::new(), None).is_err());
        assert!(read_match(Some("  ".into()), Vec::new(), vec!["x".into()], None).is_ok());
    }
}
