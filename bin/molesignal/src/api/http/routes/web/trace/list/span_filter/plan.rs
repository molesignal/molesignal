// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a Span-filter statement asks of a trace: the table of the traces
//! whose Spans it read, with what their Spans answered, and what the summary
//! joined to it has to pass.
//!
//! A comparison on a field of a Span holds for a trace when one of its Spans
//! passes it, and with `!=` when none of them has the value. A part of the
//! query made of such comparisons alone is asked of a trace's Spans together,
//! in the `HAVING` of the table they are grouped into. A part made of the
//! trace's own fields and free text is asked of its summary. A part that mixes
//! the two, `service.name = 'checkout' OR span_count > 5`, is asked of the
//! joined row: each Span comparison in it is worked out for every trace as a
//! column of 1 or 0, which the part reads beside the summary's fields.

use super::{RAW_TABLE, TraceFilter, TraceListContext, filter_predicate, qualified, sql_literal};
use crate::{
    api::http::routes::web::trace::filter::TraceCondition,
    shared::{Error, Result},
};

/// The column of the `matching_trace_ids` table that says whether a trace has
/// a Span that passes one comparison: this prefix and its place in the query.
const SPAN_ANSWER: &str = "__molesignal_span_";

/// The fields of a summary that free text is looked for in.
const TEXT_FIELDS: [&str; 5] = ["trace_id", "span_id", "service.name", "name", "status_code"];

pub(super) struct Plan {
    /// The body of the `matching_trace_ids` table: a row for each trace with a
    /// Span in the range, with the answers of its Spans.
    pub(super) matching: String,
    /// What the summary `s` of a trace, joined to its row `m` of that table,
    /// has to pass.
    pub(super) predicates: Vec<String>,
}

pub(super) fn plan(context: &TraceListContext) -> Result<Plan> {
    let query = context.query();
    let mut having = Vec::new();
    let mut predicates = Vec::new();
    let mut answers = Vec::new();

    for part in query.conjuncts() {
        let filters = part.filters();

        let spans = filters
            .iter()
            .filter(|filter| filter.is_span_filter())
            .count();

        if spans > 0 && spans == filters.len() && !part.has_text() {
            having.push(asked_of_spans(part)?);
        } else {
            predicates.push(asked_of_row(part, &mut answers)?);
        }
    }

    if having.is_empty() && answers.is_empty() {
        return Err(Error::internal(
            "trace Span-filter query requires at least one Span filter",
        ));
    }

    let columns = answers
        .iter()
        .enumerate()
        .map(|(index, question)| {
            format!(
                ",\n       MAX(CASE WHEN {question} THEN 1 ELSE 0 END) AS \"{SPAN_ANSWER}{index}\""
            )
        })
        .collect::<String>();

    let having = if having.is_empty() {
        String::new()
    } else {
        format!("\nHAVING {}", having.join(" AND "))
    };

    Ok(Plan {
        matching: format!(
            "SELECT \"trace_id\"{columns}\n\
             FROM \"{RAW_TABLE}\" AS r\n\
             WHERE \"trace_id\" IS NOT NULL\n\
             GROUP BY \"trace_id\"{having}"
        ),
        predicates,
    })
}

/// What a Span is asked for a comparison on one of its fields, and the answer
/// a trace needs from its Spans: one that says yes (1), or for a value left
/// out none that has it (0). A Span without the field compares to nothing, so
/// it never has the value.
fn span_question(filter: &TraceFilter) -> Result<(String, u8)> {
    if filter.excludes() {
        return Ok((filter_predicate("r", filter, &filter.field, "=")?, 0));
    }

    Ok((filter_predicate("r", filter, &filter.field, &filter.op)?, 1))
}

/// A part made of Span comparisons alone, asked of a trace's Spans together.
fn asked_of_spans(part: &TraceCondition) -> Result<String> {
    match part {
        TraceCondition::All { and } => joined(and, " AND ", asked_of_spans),
        TraceCondition::Any { or } => joined(or, " OR ", asked_of_spans),
        TraceCondition::Compare(filter) => {
            let (question, answer) = span_question(filter)?;

            Ok(format!(
                "MAX(CASE WHEN {question} THEN 1 ELSE 0 END) = {answer}"
            ))
        }
        TraceCondition::Text { .. } => Err(Error::internal(
            "free text is looked for in a trace summary",
        )),
    }
}

/// A part asked of the joined row: a Span comparison by the column its
/// answer is kept in, which `answers` gains; anything else of the summary.
fn asked_of_row(part: &TraceCondition, answers: &mut Vec<String>) -> Result<String> {
    match part {
        TraceCondition::All { and } => joined(and, " AND ", |item| asked_of_row(item, answers)),
        TraceCondition::Any { or } => joined(or, " OR ", |item| asked_of_row(item, answers)),
        TraceCondition::Text { text } => Ok(text_predicate(text)),
        TraceCondition::Compare(filter) if filter.is_span_filter() => {
            let (question, answer) = span_question(filter)?;
            let column = qualified("m", &format!("{SPAN_ANSWER}{}", answers.len()));
            answers.push(question);

            Ok(format!("{column} = {answer}"))
        }
        TraceCondition::Compare(filter) => {
            let column = filter
                .summary_column()
                .ok_or_else(|| Error::internal("invalid trace summary filter"))?;

            filter_predicate("s", filter, column, &filter.op)
        }
    }
}

fn joined(
    items: &[TraceCondition],
    separator: &str,
    mut ask: impl FnMut(&TraceCondition) -> Result<String>,
) -> Result<String> {
    let asked = items.iter().map(&mut ask).collect::<Result<Vec<_>>>()?;

    Ok(format!("({})", asked.join(separator)))
}

/// Free text, looked for in the fields of the summary `s`, whatever the case.
fn text_predicate(text: &str) -> String {
    let value = sql_literal(&format!("%{}%", text.to_ascii_lowercase()));

    let fields = TEXT_FIELDS
        .into_iter()
        .map(|field| {
            format!(
                "LOWER(COALESCE(CAST({} AS VARCHAR), '')) LIKE {value}",
                qualified("s", field)
            )
        })
        .collect::<Vec<_>>();

    format!("({})", fields.join(" OR "))
}
