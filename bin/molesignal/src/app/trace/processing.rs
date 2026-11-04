// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 租户的 Span 在写入之前经过所在流的实时流水线（[`IntakeService::process_spans`]）。在尾采样之后
//! 处理，Trace 的摘要就按写下的 Span 算；平台自身遥测的 Span 不经过流水线。

use crate::{
    app::intake::IntakeService,
    domain::intake::RawEvent,
    shared::{ids::Id, self_telemetry::with_suppression},
};

/// 流水线留下的 Span。它丢掉的不写；它处理失败的也不写，发送方早已收到应答，只在日志里记下。
pub(super) async fn written_spans(
    intake: &IntakeService,
    org_id: &str,
    stream: &str,
    spans: Vec<RawEvent>,
) -> Result<Vec<RawEvent>, String> {
    let processed = with_suppression(intake.process_spans(&Id(org_id.to_owned()), stream, spans))
        .await
        .map_err(|error| error.to_string())?;

    if let Some(first) = processed.failed.first() {
        tracing::warn!(
            org_id,
            stream,
            failed = processed.failed.len(),
            reason = %first.reason,
            "the realtime pipeline failed on spans; they are not written"
        );
    }

    Ok(processed.kept)
}
