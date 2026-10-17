// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 多步处理执行原语：对一批事件按顺序应用处理步骤（见 [`processing`]）。
//!
//! 这是「真实执行」的可复用核心。端到端编排（读源 stream → 处理 → 写目标 stream + connector
//! egress + 死信）在 bootstrap `workers::pipeline_exec`；读什么、写到哪、失败怎么办由
//! [`crate::domain::pipeline::scheduled::ScheduledConfig`] 决定。本模块只负责确定性的纯计算部分，可单测。

use serde_json::Value;

use crate::domain::stream::StreamType;

/// 从工作台写入的 `function_steps.signal_type` 还原数据流类型。
///
/// 旧版流水线只保存步骤数组，没有 signal_type；这些流水线历史上均为日志处理，
/// 因此缺失或无法识别时保持 Logs 兼容。
pub fn parse_signal_type(function_steps: &Value) -> StreamType {
    match function_steps
        .get("signal_type")
        .and_then(Value::as_str)
        .unwrap_or("logs")
    {
        "metrics" => StreamType::METRICS,
        "traces" => StreamType::TRACES,
        _ => StreamType::LOGS,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parse_signal_type_supports_workbench_shape_and_legacy_logs() {
        assert_eq!(
            parse_signal_type(&json!({ "signal_type": "metrics" })),
            StreamType::METRICS
        );
        assert_eq!(
            parse_signal_type(&json!({ "signal_type": "traces" })),
            StreamType::TRACES
        );
        assert_eq!(parse_signal_type(&json!([])), StreamType::LOGS);
        assert_eq!(
            parse_signal_type(&json!({ "signal_type": "unknown" })),
            StreamType::LOGS
        );
    }
}

pub mod processing;
