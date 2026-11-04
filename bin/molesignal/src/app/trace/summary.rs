// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Trace 原始 Span 与列表摘要的独立写入投影。

use serde_json::Value;

use crate::{
    domain::{intake::RawEvent, stream::Schema},
    shared::{
        tail_sampling::DecidedTrace,
        time::TimestampMicros,
        trace::summary::{
            TRACE_SUMMARY_DURATION_NS_FIELD, TRACE_SUMMARY_ERROR_COUNT_FIELD,
            TRACE_SUMMARY_MARKER_FIELD, TRACE_SUMMARY_SPAN_COUNT_FIELD,
            TRACE_SUMMARY_START_NS_FIELD,
        },
    },
};

/// 流的 schema 里有摘要的全部字段：写过摘要的流才有东西可列。只经原生接入写入的流没有。
pub(crate) fn carries_summaries(schema: &Schema) -> bool {
    [
        TRACE_SUMMARY_MARKER_FIELD,
        TRACE_SUMMARY_START_NS_FIELD,
        TRACE_SUMMARY_DURATION_NS_FIELD,
        TRACE_SUMMARY_SPAN_COUNT_FIELD,
        TRACE_SUMMARY_ERROR_COUNT_FIELD,
    ]
    .iter()
    .all(|required| schema.fields.iter().any(|field| field.name == *required))
}

pub(super) fn span_events(trace: &DecidedTrace) -> Vec<RawEvent> {
    trace
        .spans
        .iter()
        .cloned()
        .map(|span| span.into_raw_event())
        .collect()
}

/// 已决 Trace 只在第一次写它时有摘要：late export 只补原始 Span，不生成第二行摘要。
pub(super) fn summarized(trace: &DecidedTrace) -> bool {
    !trace.spans.iter().any(|span| span.late)
}

/// 一条 Trace 写下的 Span 的摘要行：根 Span 那一行，加上整条 Trace 的起止、Span 数和出错数。按写下
/// 的行算，实时流水线丢掉、改掉的 Span 也就照它们写下的样子计数；一行都没写下时没有摘要。
pub(super) fn summary_event(spans: &[RawEvent]) -> Option<RawEvent> {
    let carrier = summary_carrier(spans)?;
    let start_ns = spans.iter().filter_map(|span| nanos(span, START)).min()?;

    let end_ns = spans
        .iter()
        .filter_map(|span| nanos(span, END))
        .max()
        .unwrap_or(start_ns);

    let mut event = carrier.clone();
    event.timestamp = TimestampMicros(i64::try_from(start_ns / 1_000).unwrap_or(i64::MAX));

    event
        .fields
        .insert(TRACE_SUMMARY_MARKER_FIELD.into(), Value::String("1".into()));

    let start_ns_i64 = i64::try_from(start_ns).unwrap_or(i64::MAX);
    let duration_ns_i64 = i64::try_from(end_ns.saturating_sub(start_ns)).unwrap_or(i64::MAX);

    event.fields.insert(
        TRACE_SUMMARY_START_NS_FIELD.into(),
        Value::from(start_ns_i64),
    );

    event.fields.insert(
        TRACE_SUMMARY_DURATION_NS_FIELD.into(),
        Value::from(duration_ns_i64),
    );

    event.fields.insert(
        TRACE_SUMMARY_SPAN_COUNT_FIELD.into(),
        Value::from(i64::try_from(spans.len()).unwrap_or(i64::MAX)),
    );

    let errors = spans
        .iter()
        .filter(|span| text(span, "status_code").eq_ignore_ascii_case("ERROR"))
        .count();

    event.fields.insert(
        TRACE_SUMMARY_ERROR_COUNT_FIELD.into(),
        Value::from(i64::try_from(errors).unwrap_or(i64::MAX)),
    );

    Some(event)
}

const START: &str = "start_time_unix_nano";
const END: &str = "end_time_unix_nano";

fn nanos(span: &RawEvent, field: &str) -> Option<u64> {
    span.fields.get(field).and_then(Value::as_u64)
}

fn text<'a>(span: &'a RawEvent, field: &str) -> &'a str {
    span.fields.get(field).and_then(Value::as_str).unwrap_or("")
}

/// 带摘要的那一行：最早开始的根 Span；没有根 Span 时最早开始的 Span。
fn summary_carrier(spans: &[RawEvent]) -> Option<&RawEvent> {
    let earliest = |left: &&RawEvent, right: &&RawEvent| {
        let start = |span: &RawEvent| nanos(span, START).unwrap_or(u64::MAX);

        start(left)
            .cmp(&start(right))
            .then_with(|| text(left, "span_id").cmp(text(right, "span_id")))
    };

    spans
        .iter()
        .filter(|span| text(span, "parent_span_id").is_empty())
        .min_by(earliest)
        .or_else(|| spans.iter().min_by(earliest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::{
        tail_sampling::DecidedTrace,
        trace::normalization::{CanonicalSpan, SamplingReason},
    };

    #[test]
    fn writes_spans_without_summary_marker_and_one_independent_summary() {
        let mut root = CanonicalSpan::new(
            "trace-1".into(),
            "root".into(),
            "request".into(),
            1,
            10,
            100,
        );
        root.status_code = "ERROR".into();
        let mut child =
            CanonicalSpan::new("trace-1".into(), "child".into(), "db".into(), 2, 20, 80);
        child.parent_span_id = Some("root".into());
        let trace = DecidedTrace {
            org_id: "org".into(),
            stream: Some("default".into()),
            trace_id: "trace-1".into(),
            policy_version: 1,
            kept: true,
            reason: SamplingReason::Rule,
            spans: vec![child, root],
        };

        assert!(
            span_events(&trace)
                .iter()
                .all(|row| !row.fields.contains_key(TRACE_SUMMARY_MARKER_FIELD))
        );
        let summary = summary_event(&span_events(&trace)).unwrap();
        assert_eq!(summary.fields[TRACE_SUMMARY_START_NS_FIELD], 10);
        assert_eq!(summary.fields[TRACE_SUMMARY_DURATION_NS_FIELD], 90);
        assert_eq!(summary.fields[TRACE_SUMMARY_SPAN_COUNT_FIELD], 2);
        assert_eq!(summary.fields[TRACE_SUMMARY_ERROR_COUNT_FIELD], 1);
    }

    #[test]
    fn late_export_has_no_second_summary() {
        let mut span = CanonicalSpan::new(
            "trace-1".into(),
            "late".into(),
            "late span".into(),
            1,
            110,
            120,
        );
        span.late = true;
        let trace = DecidedTrace {
            org_id: "org".into(),
            stream: Some("default".into()),
            trace_id: "trace-1".into(),
            policy_version: 1,
            kept: true,
            reason: SamplingReason::Rule,
            spans: vec![span],
        };
        assert_eq!(span_events(&trace).len(), 1);
        assert!(!summarized(&trace));
    }

    #[test]
    fn a_summary_counts_the_spans_as_they_were_written() {
        let row = |span: &str, parent: &str, start: u64, end: u64, status: &str| RawEvent {
            timestamp: TimestampMicros(0),
            fields: serde_json::json!({
                "trace_id": "trace-1",
                "span_id": span,
                "parent_span_id": parent,
                "start_time_unix_nano": start,
                "end_time_unix_nano": end,
                "status_code": status,
            })
            .as_object()
            .unwrap()
            .clone(),
        };

        // A pipeline dropped the root's other child and marked the timeout as no error.
        let written = vec![
            row("db", "root", 20, 80, "OK"),
            row("root", "", 10, 100, "OK"),
        ];
        let summary = summary_event(&written).unwrap();

        assert_eq!(summary.fields["span_id"], "root");
        assert_eq!(summary.fields[TRACE_SUMMARY_SPAN_COUNT_FIELD], 2);
        assert_eq!(summary.fields[TRACE_SUMMARY_ERROR_COUNT_FIELD], 0);
        assert_eq!(summary.fields[TRACE_SUMMARY_DURATION_NS_FIELD], 90);
        assert!(summary_event(&[]).is_none());
    }
}
