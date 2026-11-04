// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Trace-list projection availability check.

use crate::{
    api::AppState,
    app::{iam::IamContext, trace::summary::carries_summaries},
    domain::stream::StreamType,
};

pub(super) async fn available(state: &AppState, ctx: &IamContext, stream: &str) -> bool {
    state
        .telemetry
        .streams
        .get(&ctx.org_id, stream, StreamType::TRACES)
        .await
        .is_ok_and(|definition| carries_summaries(&definition.schema))
}
