// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 多步处理执行原语：对一批事件按顺序应用处理步骤（见 [`processing`]）。
//!
//! 这是「真实执行」的可复用核心。端到端编排（读源 stream → 处理 → 写目标 stream + connector
//! egress + 死信）在 bootstrap `workers::pipeline_exec`；读什么、写到哪、失败怎么办由
//! [`domain::pipeline::scheduled::ScheduledConfig`] 决定。本模块只负责确定性的纯计算部分，可单测。

use domain::stream::StreamType;
use serde_json::Value;

/// 从工作台写入的 `function_steps.signal_type` 还原数据流类型。A pipeline that names no
/// known signal type processes logs.
pub fn parse_signal_type(function_steps: &Value) -> StreamType {
    match function_steps
        .get("signalType")
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
    fn parse_signal_type_reads_the_workbench_shape() {
        assert_eq!(
            parse_signal_type(&json!({ "signalType": "metrics" })),
            StreamType::METRICS
        );
        assert_eq!(
            parse_signal_type(&json!({ "signalType": "traces" })),
            StreamType::TRACES
        );
        assert_eq!(
            parse_signal_type(&json!({ "signalType": "unknown" })),
            StreamType::LOGS
        );
    }
}

pub mod processing;
