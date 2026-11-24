// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What every reading of Spans asks the same way: which traces stream to read, and how a
//! statement tells a request a service took in, a Span that failed and how long one took. The
//! Traces page, the evidence of an incident and the agent's tools read Spans with these.

use domain::stream::{FieldDef, FieldType, Schema, StreamDefinition};

use super::summary::carries_summaries;
use crate::infra::query::escape_sql_ident;

/// The fields every trace query reads: a stream without one of them would fail to plan.
const REQUIRED_FIELDS: [&str; 7] = [
    "trace_id",
    "span_id",
    "service.name",
    "name",
    "start_time_unix_nano",
    "end_time_unix_nano",
    "status_code",
];

/// The traces stream to read of an organization's streams.
///
/// A stream that carries the standard OTEL column contract comes first: an older or sample
/// stream may use `span.name`, or omit explicit start and end columns, and would make every
/// trace query fail to plan. Among those, a stream with summaries comes first: the trace list
/// reads nothing else, so a stream written only through native intake must not hide one. Then
/// `default`, then by name.
pub fn choose_traces_stream(mut traces: Vec<StreamDefinition>) -> Option<StreamDefinition> {
    traces.sort_by(|left, right| {
        rank(right)
            .cmp(&rank(left))
            .then_with(|| left.name.cmp(&right.name))
    });

    traces.into_iter().next()
}

fn rank(stream: &StreamDefinition) -> (bool, bool, bool) {
    let canonical = REQUIRED_FIELDS
        .iter()
        .all(|required| declared(&stream.schema, required).is_some());

    (
        canonical,
        carries_summaries(&stream.schema),
        stream.name == "default",
    )
}

/// Where a service took a request in: a Span of the server or consumer kind, or the root of a
/// trace that is not itself a call out. A client or producer Span without a parent is a call
/// made outside any request, not one taken in. `None` for a stream that tells neither kind nor
/// parent: every Span is then counted.
pub fn entry_clause(schema: &Schema) -> Option<String> {
    let taking = kind_in(schema, [2, 5], ["SERVER", "CONSUMER"]);
    let making = kind_in(schema, [3, 4], ["CLIENT", "PRODUCER"]);

    let root = declared(schema, "parent_span_id")
        .is_some()
        .then(|| "(\"parent_span_id\" IS NULL OR \"parent_span_id\" = '')".to_string());

    match (taking, making, root) {
        (Some(taking), Some(making), Some(root)) => Some(format!(
            "({taking} OR ({root} AND (\"kind\" IS NULL OR NOT {making})))"
        )),
        (Some(taking), _, None) => Some(taking),
        (None, _, Some(root)) => Some(root),
        _ => None,
    }
}

/// 1 for a Span that failed, 0 for one that did not. `None` for a stream that keeps no status:
/// its failures cannot be told.
pub fn failed_expression(schema: &Schema) -> Option<String> {
    declared(schema, "status_code").map(|_| {
        "CASE WHEN UPPER(CAST(\"status_code\" AS VARCHAR)) = 'ERROR' THEN 1 ELSE 0 END".to_string()
    })
}

/// How long a Span took, in whole nanoseconds.
pub fn duration_expression(schema: &Schema) -> String {
    if declared(schema, "duration_ns").is_some() {
        quoted("duration_ns")
    } else {
        "(\"end_time_unix_nano\" - \"start_time_unix_nano\")".to_string()
    }
}

/// Whether a Span is of one of two kinds, as the stream keeps kinds: the numbers OTLP gives
/// them, or text, which may hold those numbers or the kinds' names. `None` for a stream that
/// keeps no kind.
fn kind_in(schema: &Schema, numbers: [i64; 2], names: [&str; 2]) -> Option<String> {
    match declared(schema, "kind").map(|field| field.data_type) {
        Some(FieldType::Int64) => Some(format!("\"kind\" IN ({}, {})", numbers[0], numbers[1])),
        Some(FieldType::Utf8) => {
            let listed = numbers
                .iter()
                .map(i64::to_string)
                .chain(
                    names
                        .iter()
                        .flat_map(|name| [name.to_string(), format!("SPAN_KIND_{name}")]),
                )
                .map(|text| format!("'{}'", text.replace('\'', "''")))
                .collect::<Vec<_>>();

            Some(format!("UPPER(\"kind\") IN ({})", listed.join(", ")))
        }
        _ => None,
    }
}

pub fn declared<'a>(schema: &'a Schema, name: &str) -> Option<&'a FieldDef> {
    schema.fields.iter().find(|field| field.name == name)
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", escape_sql_ident(name))
}
