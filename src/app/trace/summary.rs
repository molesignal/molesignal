// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Trace 原始 Span 与列表摘要的独立写入投影。

use serde_json::Value;

use crate::{
    domain::ingestion::RawEvent,
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

pub(super) fn span_events(trace: &DecidedTrace) -> Vec<RawEvent> {
    trace
        .spans
        .iter()
        .cloned()
        .map(|span| span.into_raw_event())
        .collect()
}

/// 每个已决 Trace 生成恰好一行摘要。late export 只补原始 Span，不生成第二行摘要。
pub(super) fn summary_event(trace: &DecidedTrace) -> Option<RawEvent> {
    if trace.spans.is_empty() || trace.spans.iter().any(|span| span.late) {
        return None;
    }
    let carrier = summary_carrier(trace)?.clone();
    let start_ns = trace
        .spans
        .iter()
        .map(|span| span.start_time_unix_nano)
        .min()?;
    let end_ns = trace
        .spans
        .iter()
        .map(|span| span.end_time_unix_nano)
        .max()
        .unwrap_or(start_ns);
    let mut event = carrier.into_raw_event();
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
        Value::from(i64::try_from(trace.spans.len()).unwrap_or(i64::MAX)),
    );
    event.fields.insert(
        TRACE_SUMMARY_ERROR_COUNT_FIELD.into(),
        Value::from(
            i64::try_from(
                trace
                    .spans
                    .iter()
                    .filter(|span| span.status_code.eq_ignore_ascii_case("ERROR"))
                    .count(),
            )
            .unwrap_or(i64::MAX),
        ),
    );
    Some(event)
}

fn summary_carrier(
    trace: &DecidedTrace,
) -> Option<&crate::shared::trace::normalization::CanonicalSpan> {
    trace
        .spans
        .iter()
        .filter(|span| span.parent_span_id.as_deref().is_none_or(str::is_empty))
        .min_by(|left, right| {
            left.start_time_unix_nano
                .cmp(&right.start_time_unix_nano)
                .then_with(|| left.span_id.cmp(&right.span_id))
        })
        .or_else(|| {
            trace.spans.iter().min_by(|left, right| {
                left.start_time_unix_nano
                    .cmp(&right.start_time_unix_nano)
                    .then_with(|| left.span_id.cmp(&right.span_id))
            })
        })
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
        let summary = summary_event(&trace).unwrap();
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
        assert!(summary_event(&trace).is_none());
    }
}
