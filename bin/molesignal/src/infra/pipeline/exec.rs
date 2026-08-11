// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 多步处理执行原语：对一批事件按顺序应用内联 VRL 脚本。
//!
//! 这是「真实执行」的可复用核心（解析 `function_steps` → 串行 VRL）。端到端编排
//! （读源 stream → apply_steps → 写目标 stream + connector egress）的 backfill 路径已接通：
//! 见 bootstrap `workers::pipeline_exec`（读源由 `QueryService` 完成，变换+写目标+egress 复用本模块）。
//! cron 自动触发的 [`super::scheduled::ScheduledPipelineRunner`] 仍是 stub（尚未 spawn）。
//! 本模块只负责确定性的纯计算部分，可单测。

use serde_json::Value;

use crate::{
    domain::stream::StreamType,
    infra::runtime::vrl::runtime::VrlRuntime,
    shared::{Error, Result},
};

/// 一个处理步骤：显示名 + 内联 VRL 脚本。
#[derive(Debug, Clone)]
pub struct TransformStep {
    pub name: String,
    pub script: String,
}

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
        "metrics" => StreamType::Metrics,
        "traces" => StreamType::Traces,
        _ => StreamType::Logs,
    }
}

/// 同一个 scheduled pipeline 的输入与输出共享 `signal_type`，因此同名意味着读写同一
/// 物理数据流。拒绝这种配置，避免传输形成自写回路或在目标身份不明确时继续写入。
pub fn validate_pipeline_streams(
    source_stream: &str,
    target_stream: &str,
    stream_type: StreamType,
) -> Result<()> {
    let source_stream = source_stream.trim();
    let target_stream = target_stream.trim();
    if source_stream.is_empty() || target_stream.is_empty() {
        return Err(Error::invalid(
            "pipeline source_stream and target_stream cannot be empty",
        ));
    }
    if source_stream == target_stream {
        return Err(Error::conflict(format!(
            "pipeline source and target stream `{source_stream}` cannot be the same for type `{}`",
            stream_type.as_str()
        )));
    }
    Ok(())
}

/// 从 scheduled pipeline 的 `function_steps` JSON 解析处理步骤。
/// 兼容三种形态：`{ steps: [{transform_name, script}] }`、裸数组、旧单对象 `{transform_name, script}`。
pub fn parse_steps(function_steps: &Value) -> Vec<TransformStep> {
    let raw = function_steps
        .get("steps")
        .and_then(Value::as_array)
        .or_else(|| function_steps.as_array());
    if let Some(arr) = raw {
        let steps: Vec<TransformStep> = arr.iter().filter_map(step_from).collect();
        if !steps.is_empty() {
            return steps;
        }
    }
    if function_steps.get("script").is_some()
        && let Some(step) = step_from(function_steps)
    {
        return vec![step];
    }
    Vec::new()
}

fn step_from(v: &Value) -> Option<TransformStep> {
    let script = v.get("script")?.as_str()?.to_string();
    let name = v
        .get("transform_name")
        .or_else(|| v.get("function_name"))
        .or_else(|| v.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("transform")
        .to_string();
    Some(TransformStep { name, script })
}

/// 解析 sink connector 引用（egress 目标 connector id 列表）。
pub fn parse_sink_connectors(function_steps: &Value) -> Vec<String> {
    function_steps
        .get("sink_connectors")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// 对每条事件依次应用所有步骤的 VRL 脚本（原地突变）。某步编译失败 → 整批失败；
/// 单条事件某步运行失败 → 记录错误并丢弃该事件后续处理。返回 `(transformed, errors)`。
pub fn apply_steps(
    runtime: &VrlRuntime,
    steps: &[TransformStep],
    events: Vec<Value>,
) -> (Vec<Value>, Vec<String>) {
    let mut errors = Vec::new();
    let mut programs = Vec::with_capacity(steps.len());
    for step in steps {
        match runtime.compile(&step.script) {
            Ok(program) => programs.push(program),
            Err(e) => errors.push(format!("step `{}` compile: {e}", step.name)),
        }
    }
    if !errors.is_empty() {
        return (Vec::new(), errors);
    }

    let mut out = Vec::with_capacity(events.len());
    for mut event in events {
        let mut ok = true;
        for (program, step) in programs.iter().zip(steps) {
            if let Err(e) = runtime.run(program, &mut event) {
                errors.push(format!("step `{}` run: {e}", step.name));
                ok = false;
                break;
            }
        }
        if ok {
            out.push(event);
        }
    }
    (out, errors)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parse_steps_new_array_shape() {
        let fs = json!({
            "steps": [
                { "transform_name": "a", "script": ".x = 1" },
                { "transform_name": "b", "script": ".y = 2" },
            ]
        });
        let steps = parse_steps(&fs);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].name, "a");
        assert_eq!(steps[1].script, ".y = 2");
    }

    #[test]
    fn parse_steps_legacy_single_object() {
        let fs = json!({ "transform_name": "legacy", "script": ".z = 3" });
        let steps = parse_steps(&fs);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].name, "legacy");
    }

    #[test]
    fn parse_sink_connectors_reads_ids() {
        let fs = json!({ "sink_connectors": ["c1", "c2"] });
        assert_eq!(parse_sink_connectors(&fs), vec!["c1", "c2"]);
        assert!(parse_sink_connectors(&json!({})).is_empty());
    }

    #[test]
    fn parse_signal_type_supports_workbench_shape_and_legacy_logs() {
        assert_eq!(
            parse_signal_type(&json!({ "signal_type": "metrics" })),
            StreamType::Metrics
        );
        assert_eq!(
            parse_signal_type(&json!({ "signal_type": "traces" })),
            StreamType::Traces
        );
        assert_eq!(parse_signal_type(&json!([])), StreamType::Logs);
        assert_eq!(
            parse_signal_type(&json!({ "signal_type": "unknown" })),
            StreamType::Logs
        );
    }

    #[test]
    fn pipeline_rejects_same_source_and_target_for_one_signal_type() {
        let err = validate_pipeline_streams("app_logs", "app_logs", StreamType::Logs)
            .expect_err("same stream must be rejected");
        assert_eq!(err.http_status_code(), 409);
        assert!(err.to_string().contains("app_logs"));
        assert!(err.to_string().contains("logs"));

        assert!(
            validate_pipeline_streams("app_logs", "app_logs_enriched", StreamType::Logs).is_ok()
        );
    }

    #[test]
    fn apply_steps_runs_chain_in_order() {
        let rt = VrlRuntime::new();
        let steps = vec![
            TransformStep {
                name: "set-env".into(),
                script: r#".env = "prod""#.into(),
            },
            TransformStep {
                name: "upcase".into(),
                script: r#".env = upcase(string!(.env))"#.into(),
            },
        ];
        let (out, errors) = apply_steps(&rt, &steps, vec![json!({ "msg": "hi" })]);
        assert!(errors.is_empty());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["env"], "PROD");
        assert_eq!(out[0]["msg"], "hi");
    }

    #[test]
    fn apply_steps_reports_compile_error() {
        let rt = VrlRuntime::new();
        let steps = vec![TransformStep {
            name: "bad".into(),
            script: "this ((".into(),
        }];
        let (out, errors) = apply_steps(&rt, &steps, vec![json!({})]);
        assert!(out.is_empty());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("bad"));
    }
}
