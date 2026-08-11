// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use crate::domain::apm::{ApmSpanFact, ErrorSample, TraceExemplar};

pub(super) fn dimension_is_overflow(fact: &ApmSpanFact) -> bool {
    fact.transaction
        .as_ref()
        .is_some_and(|value| value.name == crate::domain::apm::OTHER_DIMENSION)
        || fact
            .dependency
            .as_ref()
            .is_some_and(|value| value.target == crate::domain::apm::OTHER_DIMENSION)
        || fact.error.as_ref().is_some_and(|value| value.overflow)
}

pub(super) fn admit_exemplar(values: &mut Vec<TraceExemplar>, value: TraceExemplar, limit: usize) {
    if values
        .iter()
        .any(|current| current.trace_id == value.trace_id && current.span_id == value.span_id)
    {
        return;
    }
    values.push(value);
    values.sort_by(|left, right| {
        right
            .duration_micros
            .cmp(&left.duration_micros)
            .then_with(|| left.trace_id.cmp(&right.trace_id))
    });
    values.truncate(limit);
}

pub(super) fn admit_error_sample(values: &mut Vec<ErrorSample>, value: ErrorSample, limit: usize) {
    if let Some(existing) = values
        .iter_mut()
        .find(|current| current.trace_id == value.trace_id && current.span_id == value.span_id)
    {
        if value.event_time >= existing.event_time {
            *existing = value;
        }
        return;
    }
    values.push(value);
    values.sort_by_key(|sample| std::cmp::Reverse(sample.event_time));
    values.truncate(limit);
}
