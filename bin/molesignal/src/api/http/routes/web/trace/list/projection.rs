// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Trace-list projection availability check.

use domain::{iam::IamContext, stream::StreamType};

use crate::{api::AppState, app::trace::summary::carries_summaries};

pub(super) async fn available(state: &AppState, ctx: &IamContext, stream: &str) -> bool {
    state
        .telemetry
        .streams
        .get(&ctx.org_id, stream, StreamType::TRACES)
        .await
        .is_ok_and(|definition| carries_summaries(&definition.schema))
}
