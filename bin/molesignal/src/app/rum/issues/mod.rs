// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The errors of an application as issues people can act on, read for the pages and for the tools.
//!
//! An issue is what the errors that are alike are one thing of. What it is and what is being done
//! about it live in Postgres (`rum_issues`, kept as errors are written); how many sessions and
//! people it touched in a range and how it went over the range are counted from the errors
//! themselves. A list joins the two.

pub mod read;
pub mod select;

use serde::Serialize;

use self::{
    read::{Sample, Stack},
    select::{Holder, IssueView, Numbers, Sort, StatusFilter, Wanted},
};
use super::{
    reader::{Reader, within_budget},
    selection::Selection,
};
use crate::{
    domain::rum::{RumIssue, RumIssueRepository, event::RumStream},
    infra::rum::query::{Filter, Scope, Unavailable},
    shared::{Error, Result},
};

/// How many issues a list shows unless it asks for fewer or more.
pub const DEFAULT_LIMIT: usize = 50;
/// The most issues a list shows; the breakdowns beside it name them in one filter.
pub const MAX_LIMIT: usize = 100;

/// How far back the latest errors of an issue are looked for.
const SAMPLE_WINDOW_MICROS: i64 = 14 * 24 * 60 * 60 * 1_000_000;

/// Longest id an issue has.
const MAX_ID_LEN: usize = 64;

fn filter(field: &str, op: &str, value: &str) -> Filter {
    Filter {
        field: field.into(),
        op: op.into(),
        value: value.into(),
        quoted: false,
    }
}

/// What a request left in a parameter, without the spaces around it; none when it is empty.
pub fn present(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// The order of a list: the one asked for, or the one its sort reads in.
pub fn descending(order: Option<&str>, sort: Sort) -> Result<bool> {
    match present(order) {
        None => Ok(sort.default_descending()),
        Some(order) if order.eq_ignore_ascii_case("asc") => Ok(false),
        Some(order) if order.eq_ignore_ascii_case("desc") => Ok(true),
        Some(_) => Err(Error::invalid("order must be asc or desc")),
    }
}

/// An id that is a short word, as the ids of issues are.
pub fn checked_id(id: &str) -> Result<&str> {
    let id = id.trim();

    if id.is_empty()
        || id.len() > MAX_ID_LEN
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character))
    {
        return Err(Error::not_found("RUM issue"));
    }

    Ok(id)
}

/// What a list of issues is asked: the application, the range, what the errors counted have to
/// meet and which issues are kept and in which order.
#[derive(Debug)]
pub struct Query {
    pub application: String,
    pub env: Option<String>,
    /// What sent the errors: `browser`, `flutter`…
    pub source: Option<String>,
    /// More conditions the errors counted have to meet, as the explorer says them.
    pub filters: Vec<Filter>,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub status: StatusFilter,
    pub assignee: Holder,
    pub team: Holder,
    pub search: String,
    /// Only the issues that are new in the range or went back in it.
    pub fresh: bool,
    pub sort: Sort,
    pub descending: bool,
    pub limit: usize,
}

#[derive(Debug, Serialize)]
pub struct Listing {
    pub issues: Vec<IssueView>,
    /// How many issues the filters leave, before the list is cut to `limit`.
    pub total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<Unavailable>,
}

impl Listing {
    fn without(unavailable: Option<Unavailable>) -> Self {
        Self {
            issues: Vec::new(),
            total: 0,
            unavailable,
        }
    }
}

/// The issues with errors in the range of an application, with their numbers and a trend each.
pub async fn list(
    reader: &Reader<'_>,
    issues: &dyn RumIssueRepository,
    query: &Query,
) -> Result<Listing> {
    // Errors from before the issues were kept have no issue to count them in.
    let mut filters = vec![
        filter("application.id", "=", &query.application),
        filter("error.issue_id", "!=", ""),
    ];

    if let Some(env) = present(query.env.as_deref()) {
        filters.push(filter("env", "=", env));
    }

    if let Some(source) = present(query.source.as_deref()) {
        filters.push(filter("source", "=", source));
    }

    filters.extend(query.filters.iter().cloned());

    let selection = Selection::new(Some("errors"), query.from, query.to, filters, Vec::new())?;

    let wanted = Wanted {
        status: query.status,
        assignee: query.assignee.clone(),
        team: query.team.clone(),
        search: query.search.clone(),
        fresh_since: query.fresh.then_some(selection.span.from),
        sort: query.sort,
        descending: query.descending,
    };

    let limit = query.limit.clamp(1, MAX_LIMIT);
    let conditions = selection.conditions()?;

    let scope = Scope {
        stream: RumStream::Errors,
        from: selection.span.from,
        to: selection.span.to,
        conditions: &conditions,
    };

    let counting = read::counting(query.sort)?;

    let found = within_budget(async {
        let Some(seen) = read::count_issues(reader, selection.span, &scope, &counting).await?
        else {
            return Ok(None);
        };

        let ids = seen.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
        let kept = issues.get_many(reader.org_id(), &ids).await?;

        let mut rows = select::select(seen, kept, &wanted);
        let total = rows.len();

        rows.truncate(limit);

        let shown = rows
            .iter()
            .map(|row| row.issue.id.clone())
            .collect::<Vec<_>>();

        let mut trends = read::trends(reader, selection.span, &scope, &shown).await?;

        for row in &mut rows {
            row.trend = trends
                .remove(&row.issue.id)
                .unwrap_or_else(|| read::flat(selection.span));
        }

        Ok(Some((rows, total)))
    })
    .await?;

    Ok(match found {
        Ok(Some((issues, total))) => Listing {
            issues,
            total,
            unavailable: None,
        },
        Ok(None) => Listing::without(None),
        Err(reason) => Listing::without(Some(reason)),
    })
}

#[derive(Debug, Serialize)]
pub struct Detail {
    #[serde(flatten)]
    pub issue: RumIssue,
    /// What the errors of the issue come to in the range asked for.
    #[serde(flatten)]
    pub numbers: Numbers,
    /// The latest errors, newest first.
    pub samples: Vec<Sample>,
    pub stack: Option<Stack>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<Unavailable>,
}

/// One issue with its numbers in the range, its latest sessions and the stack of the latest error
/// that has one.
pub async fn detail(
    reader: &Reader<'_>,
    issues: &dyn RumIssueRepository,
    id: &str,
    from: Option<i64>,
    to: Option<i64>,
) -> Result<Detail> {
    let id = checked_id(id)?;

    let issue = issues
        .get(reader.org_id(), id)
        .await?
        .ok_or_else(|| Error::not_found("RUM issue"))?;

    let own = |from, to| {
        Selection::new(
            Some("errors"),
            from,
            to,
            vec![
                filter("application.id", "=", &issue.application_id),
                filter("error.issue_id", "=", &issue.id),
            ],
            Vec::new(),
        )
    };

    // The numbers are of the range the reader is looking at; the latest errors are of the weeks
    // before the issue was last seen, whatever the range.
    let counted = own(from, to)?;

    let sample_to = issue.last_seen_at.0.saturating_add(60_000_000);
    let sampled = own(Some(sample_to - SAMPLE_WINDOW_MICROS), Some(sample_to))?;

    let counted_conditions = counted.conditions()?;
    let sampled_conditions = sampled.conditions()?;

    let scope = Scope {
        stream: RumStream::Errors,
        from: counted.span.from,
        to: counted.span.to,
        conditions: &counted_conditions,
    };

    let found = within_budget(async {
        let numbers = read::count_one(reader, counted.span, &scope).await?;
        let (samples, stack) = read::samples(reader, sampled.span, &sampled_conditions).await?;

        Ok((numbers, samples, stack))
    })
    .await?;

    let (numbers, samples, stack, unavailable) = match found {
        Ok((numbers, samples, stack)) => (numbers, samples, stack, None),
        Err(reason) => (Numbers::default(), Vec::new(), None, Some(reason)),
    };

    Ok(Detail {
        issue,
        numbers,
        samples,
        stack,
        unavailable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_a_short_word() {
        assert_eq!(
            checked_id(" 3f9c2a71e0b4c8d1 ").unwrap(),
            "3f9c2a71e0b4c8d1"
        );
        assert!(checked_id("").is_err());
        assert!(checked_id("../x").is_err());
        assert!(checked_id(&"a".repeat(MAX_ID_LEN + 1)).is_err());
    }

    #[test]
    fn the_order_is_the_default_of_the_sort_unless_it_is_named() {
        assert!(descending(None, Sort::Sessions).unwrap());
        assert!(!descending(None, Sort::Status).unwrap());
        assert!(!descending(Some("ASC"), Sort::Sessions).unwrap());
        assert!(descending(Some("desc"), Sort::Status).unwrap());
        assert!(descending(Some("up"), Sort::Users).is_err());
    }

    #[test]
    fn a_parameter_without_words_is_none() {
        assert_eq!(present(Some("  web ")), Some("web"));
        assert_eq!(present(Some("  ")), None);
        assert_eq!(present(None), None);
    }
}
