// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 按组织统计的小时摄入量。
use crate::{api::AppState, shared::ids::Id};

pub(crate) fn record_intake_usage(state: &AppState, org_id: &Id, bytes: u64, now_micros: i64) {
    // 首页运营视图需要按时间窗区分「原始摄入量」与「压缩后落盘量」。小时桶只做
    // best-effort 观测，不参与配额判定。
    if bytes > 0 {
        let usage = state.platform.usage.clone();
        let usage_org_id = org_id.clone();
        crate::shared::trace_context::spawn_with_current_trace_context(async move {
            if let Err(e) = usage
                .add_hourly_intake_bytes(&usage_org_id, now_micros, bytes as i64)
                .await
            {
                tracing::warn!(
                    org_id = %usage_org_id.0,
                    error = %e,
                    "failed to record hourly intake usage"
                );
            }
        });
    }
}
