// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::cmp::Ordering;

use super::{ApmQueryContext, DependencySummary, ErrorSummary, ServiceSummary, TransactionSummary};
use crate::{
    domain::apm::{QueryCursor, SortDirection},
    shared::{
        Error, Result,
        cursor::{CursorDirection, trim_cursor_page},
    },
};

pub(super) trait ApmSortable {
    fn compare(&self, other: &Self, field: &str) -> Ordering;
    fn compare_cursor_value(&self, value: &str, field: &str) -> Result<Ordering>;
    fn sort_value(&self, field: &str) -> String;
    fn tie_breaker(&self) -> String;
}

pub(super) fn paginate<T>(
    mut items: Vec<T>,
    context: &ApmQueryContext,
) -> Result<(Vec<T>, Option<String>, Option<String>)>
where
    T: ApmSortable,
{
    items.sort_by(|left, right| {
        let ordering = left
            .compare(right, &context.sort)
            .then_with(|| left.tie_breaker().cmp(&right.tie_breaker()));
        match context.direction {
            SortDirection::Asc => ordering,
            SortDirection::Desc => ordering.reverse(),
        }
    });
    let page_direction = context.cursor.as_ref().map(|cursor| cursor.page_direction);
    let fetch_limit = context.limit.saturating_add(1);
    let candidates = match context.cursor.as_ref() {
        None => items.into_iter().take(fetch_limit).collect(),
        Some(cursor) => seek_candidates(items, context, cursor, fetch_limit)?,
    };
    let page = trim_cursor_page(candidates, context.limit, page_direction);
    let previous_cursor = if page.has_previous {
        page.items
            .first()
            .map(|item| {
                context.encode_cursor(
                    CursorDirection::Before,
                    item.sort_value(&context.sort),
                    item.tie_breaker(),
                )
            })
            .transpose()?
    } else {
        None
    };
    let next_cursor = if page.has_next {
        page.items
            .last()
            .map(|item| {
                context.encode_cursor(
                    CursorDirection::After,
                    item.sort_value(&context.sort),
                    item.tie_breaker(),
                )
            })
            .transpose()?
    } else {
        None
    };
    Ok((page.items, previous_cursor, next_cursor))
}

fn seek_candidates<T>(
    items: Vec<T>,
    context: &ApmQueryContext,
    cursor: &QueryCursor,
    fetch_limit: usize,
) -> Result<Vec<T>>
where
    T: ApmSortable,
{
    let mut matching = Vec::new();
    for item in items {
        let raw_ordering = item
            .compare_cursor_value(&cursor.sort_value, &context.sort)?
            .then_with(|| item.tie_breaker().cmp(&cursor.tie_breaker));
        let canonical_ordering = match context.direction {
            SortDirection::Asc => raw_ordering,
            SortDirection::Desc => raw_ordering.reverse(),
        };
        let include = match cursor.page_direction {
            CursorDirection::After => canonical_ordering == Ordering::Greater,
            CursorDirection::Before => canonical_ordering == Ordering::Less,
        };
        if include {
            matching.push(item);
        }
    }
    if cursor.page_direction == CursorDirection::Before {
        matching.reverse();
    }
    matching.truncate(fetch_limit);
    Ok(matching)
}

impl ApmSortable for ServiceSummary {
    fn compare(&self, other: &Self, field: &str) -> Ordering {
        match field {
            "name" => self.service.stable_key().cmp(&other.service.stable_key()),
            "error_rate" => self.red.error_rate.total_cmp(&other.red.error_rate),
            "p95" => self
                .red
                .p95_micros
                .unwrap_or_default()
                .cmp(&other.red.p95_micros.unwrap_or_default()),
            _ => self.red.request_count.cmp(&other.red.request_count),
        }
    }

    fn compare_cursor_value(&self, value: &str, field: &str) -> Result<Ordering> {
        match field {
            "name" => Ok(self.service.stable_key().as_str().cmp(value)),
            "error_rate" => compare_u64(self.red.error_rate.to_bits(), value),
            "p95" => compare_u64(self.red.p95_micros.unwrap_or_default(), value),
            _ => compare_u64(self.red.request_count, value),
        }
    }

    fn sort_value(&self, field: &str) -> String {
        match field {
            "name" => self.service.stable_key(),
            "error_rate" => self.red.error_rate.to_bits().to_string(),
            "p95" => self.red.p95_micros.unwrap_or_default().to_string(),
            _ => self.red.request_count.to_string(),
        }
    }

    fn tie_breaker(&self) -> String {
        self.service.stable_key()
    }
}

impl ApmSortable for TransactionSummary {
    fn compare(&self, other: &Self, field: &str) -> Ordering {
        compare_red(
            &self.red,
            &other.red,
            self.total_time_micros,
            other.total_time_micros,
            field,
        )
    }

    fn compare_cursor_value(&self, value: &str, field: &str) -> Result<Ordering> {
        compare_red_cursor(&self.red, self.total_time_micros, value, field)
    }

    fn sort_value(&self, field: &str) -> String {
        red_sort_value(&self.red, self.total_time_micros, field)
    }

    fn tie_breaker(&self) -> String {
        format!(
            "{}\u{0}{}\u{0}{}",
            self.service.stable_key(),
            self.version.as_deref().unwrap_or_default(),
            self.transaction.name
        )
    }
}

impl ApmSortable for DependencySummary {
    fn compare(&self, other: &Self, field: &str) -> Ordering {
        compare_red(
            &self.red,
            &other.red,
            self.total_time_micros,
            other.total_time_micros,
            field,
        )
    }

    fn compare_cursor_value(&self, value: &str, field: &str) -> Result<Ordering> {
        compare_red_cursor(&self.red, self.total_time_micros, value, field)
    }

    fn sort_value(&self, field: &str) -> String {
        red_sort_value(&self.red, self.total_time_micros, field)
    }

    fn tie_breaker(&self) -> String {
        format!(
            "{}\u{0}{}\u{0}{}",
            self.service.stable_key(),
            self.version.as_deref().unwrap_or_default(),
            self.dependency.target
        )
    }
}

impl ApmSortable for ErrorSummary {
    fn compare(&self, other: &Self, field: &str) -> Ordering {
        match field {
            "last_seen" => self.last_seen_at.cmp(&other.last_seen_at),
            "error_rate" => self.red.error_rate.total_cmp(&other.red.error_rate),
            _ => self.occurrence_count.cmp(&other.occurrence_count),
        }
    }

    fn compare_cursor_value(&self, value: &str, field: &str) -> Result<Ordering> {
        match field {
            "last_seen" => compare_i64(self.last_seen_at.0, value),
            "error_rate" => compare_u64(self.red.error_rate.to_bits(), value),
            _ => compare_u64(self.occurrence_count, value),
        }
    }

    fn sort_value(&self, field: &str) -> String {
        match field {
            "last_seen" => self.last_seen_at.0.to_string(),
            "error_rate" => self.red.error_rate.to_bits().to_string(),
            _ => self.occurrence_count.to_string(),
        }
    }

    fn tie_breaker(&self) -> String {
        self.error.fingerprint.clone()
    }
}

fn compare_red(
    left: &super::RedSummary,
    right: &super::RedSummary,
    left_total: u64,
    right_total: u64,
    field: &str,
) -> Ordering {
    match field {
        "error_rate" => left.error_rate.total_cmp(&right.error_rate),
        "p95" => left
            .p95_micros
            .unwrap_or_default()
            .cmp(&right.p95_micros.unwrap_or_default()),
        "total_time" => left_total.cmp(&right_total),
        _ => left.request_count.cmp(&right.request_count),
    }
}

fn red_sort_value(red: &super::RedSummary, total: u64, field: &str) -> String {
    match field {
        "error_rate" => red.error_rate.to_bits().to_string(),
        "p95" => red.p95_micros.unwrap_or_default().to_string(),
        "total_time" => total.to_string(),
        _ => red.request_count.to_string(),
    }
}

fn compare_red_cursor(
    red: &super::RedSummary,
    total: u64,
    value: &str,
    field: &str,
) -> Result<Ordering> {
    match field {
        "error_rate" => compare_u64(red.error_rate.to_bits(), value),
        "p95" => compare_u64(red.p95_micros.unwrap_or_default(), value),
        "total_time" => compare_u64(total, value),
        _ => compare_u64(red.request_count, value),
    }
}

fn compare_u64(actual: u64, cursor: &str) -> Result<Ordering> {
    cursor
        .parse::<u64>()
        .map(|cursor| actual.cmp(&cursor))
        .map_err(|_| Error::invalid("invalid APM numeric cursor value"))
}

fn compare_i64(actual: i64, cursor: &str) -> Result<Ordering> {
    cursor
        .parse::<i64>()
        .map(|cursor| actual.cmp(&cursor))
        .map_err(|_| Error::invalid("invalid APM numeric cursor value"))
}
