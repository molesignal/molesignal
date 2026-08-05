// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Trace-list projection availability check.

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::stream::StreamType,
    shared::trace::summary::{
        TRACE_SUMMARY_DURATION_NS_FIELD, TRACE_SUMMARY_ERROR_COUNT_FIELD,
        TRACE_SUMMARY_MARKER_FIELD, TRACE_SUMMARY_SPAN_COUNT_FIELD, TRACE_SUMMARY_START_NS_FIELD,
    },
};

pub(super) async fn available(state: &AppState, ctx: &IamContext, stream: &str) -> bool {
    let Ok(definition) = state
        .telemetry
        .streams
        .get(&ctx.org_id, stream, StreamType::Traces)
        .await
    else {
        return false;
    };
    [
        TRACE_SUMMARY_MARKER_FIELD,
        TRACE_SUMMARY_START_NS_FIELD,
        TRACE_SUMMARY_DURATION_NS_FIELD,
        TRACE_SUMMARY_SPAN_COUNT_FIELD,
        TRACE_SUMMARY_ERROR_COUNT_FIELD,
    ]
    .iter()
    .all(|required| {
        definition
            .schema
            .fields
            .iter()
            .any(|field| field.name == *required)
    })
}
