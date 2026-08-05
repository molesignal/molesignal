// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Transport-neutral primitives for stable keyset pagination.
//!
//! Each signal owns its ordered fields and cursor payload. This module keeps
//! page direction and lexicographic seek generation identical across HTTP
//! lists, PostgreSQL-backed projections, and query-engine-backed streams.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorDirection {
    #[default]
    After,
    Before,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorSortDirection {
    Asc,
    Desc,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CursorValue {
    Integer(i64),
    Float(f64),
    Text(String),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CursorPage<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub previous_cursor: Option<String>,
    pub has_more: bool,
}

impl<T> CursorPage<T> {
    pub fn empty() -> Self {
        Self {
            items: Vec::new(),
            next_cursor: None,
            previous_cursor: None,
            has_more: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TrimmedCursorPage<T> {
    pub items: Vec<T>,
    pub has_previous: bool,
    pub has_next: bool,
}

/// Consume a `page_size + 1` result. Before-cursor queries are executed with
/// reversed ordering, so their retained rows are restored to canonical order.
pub fn trim_cursor_page<T>(
    mut items: Vec<T>,
    page_size: usize,
    direction: Option<CursorDirection>,
) -> TrimmedCursorPage<T> {
    let has_extra = items.len() > page_size;
    items.truncate(page_size);
    if direction == Some(CursorDirection::Before) {
        items.reverse();
    }

    let has_previous = match direction {
        None => false,
        Some(CursorDirection::After) => !items.is_empty(),
        Some(CursorDirection::Before) => has_extra,
    };
    let has_next = match direction {
        None | Some(CursorDirection::After) => has_extra,
        Some(CursorDirection::Before) => !items.is_empty(),
    };
    TrimmedCursorPage {
        items,
        has_previous,
        has_next,
    }
}

/// Build the strict seek predicate for a compound ordering. Equality terms
/// are carried forward until the final unique tie-breaker, preventing rows
/// with equal primary sort values from being repeated or skipped.
pub fn lexicographic_seek(
    fields: &[(&str, CursorValue, CursorSortDirection)],
    page_direction: CursorDirection,
    quote_text: impl Fn(&str) -> String,
) -> String {
    let mut alternatives = Vec::with_capacity(fields.len());
    for index in 0..fields.len() {
        let mut terms = Vec::with_capacity(index + 1);
        for (name, value, _) in &fields[..index] {
            terms.push(format!("{name} = {}", value.sql(&quote_text)));
        }
        let (name, value, sort_direction) = &fields[index];
        terms.push(format!(
            "{name} {} {}",
            comparator(*sort_direction, page_direction),
            value.sql(&quote_text),
        ));
        alternatives.push(format!("({})", terms.join(" AND ")));
    }
    format!("({})", alternatives.join(" OR "))
}

fn comparator(
    sort_direction: CursorSortDirection,
    page_direction: CursorDirection,
) -> &'static str {
    match (sort_direction, page_direction) {
        (CursorSortDirection::Asc, CursorDirection::After)
        | (CursorSortDirection::Desc, CursorDirection::Before) => ">",
        (CursorSortDirection::Desc, CursorDirection::After)
        | (CursorSortDirection::Asc, CursorDirection::Before) => "<",
    }
}

impl CursorValue {
    fn sql(&self, quote_text: &impl Fn(&str) -> String) -> String {
        match self {
            Self::Integer(value) => value.to_string(),
            Self::Float(value) if value.is_finite() => value.to_string(),
            Self::Float(_) => "NULL".to_string(),
            Self::Text(value) => quote_text(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "''"))
    }

    #[test]
    fn compound_seek_carries_equality_to_unique_key() {
        let fields = [
            (
                "duration_ns",
                CursorValue::Integer(99),
                CursorSortDirection::Desc,
            ),
            (
                "start_ns",
                CursorValue::Integer(88),
                CursorSortDirection::Desc,
            ),
            (
                "trace_id",
                CursorValue::Text("trace-z".into()),
                CursorSortDirection::Desc,
            ),
        ];

        assert_eq!(
            lexicographic_seek(&fields, CursorDirection::After, quote),
            "((duration_ns < 99) OR (duration_ns = 99 AND start_ns < 88) OR (duration_ns = 99 AND start_ns = 88 AND trace_id < 'trace-z'))"
        );
    }

    #[test]
    fn before_inverts_each_sort_comparator() {
        let fields = [
            (
                "name",
                CursorValue::Text("api".into()),
                CursorSortDirection::Asc,
            ),
            (
                "id",
                CursorValue::Text("2".into()),
                CursorSortDirection::Desc,
            ),
        ];

        assert_eq!(
            lexicographic_seek(&fields, CursorDirection::Before, quote),
            "((name < 'api') OR (name = 'api' AND id > '2'))"
        );
    }

    #[test]
    fn trims_bidirectional_page_in_canonical_order() {
        let first = trim_cursor_page(vec![1, 2, 3], 2, None);
        assert_eq!(first.items, vec![1, 2]);
        assert!(!first.has_previous);
        assert!(first.has_next);

        let before = trim_cursor_page(vec![4, 3, 2], 2, Some(CursorDirection::Before));
        assert_eq!(before.items, vec![3, 4]);
        assert!(before.has_previous);
        assert!(before.has_next);
    }
}
