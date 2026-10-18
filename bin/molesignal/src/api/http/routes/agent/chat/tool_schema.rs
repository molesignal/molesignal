// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 交给模型的工具清单，按 provider 期望的格式。

use serde_json::{Value, json};

use super::super::toolsets;
use crate::agent::{chat::Provider, tools::builtin_tools};

/// 把 [`builtin_tools`] 转成 provider 期望的 tools schema。
/// OpenAI / OpenAI-compatible：`{type:function, function:{name,description,parameters}}`；
/// Anthropic：`{name, description, input_schema}`。
///
/// `extra` 是这一轮另外要直接交给模型的工具（Skill 的 `allowed-tools`、Skill 的三个工具）：
/// 只收组织策略允许、当前可用的，被停用或没登记的名称照样不出现。
pub(super) fn tools_schema_for(
    provider: Provider,
    resolution: &toolsets::ToolsetResolution,
    extra: &[String],
) -> Value {
    // 常用与控制工具固定暴露；其余内置/MCP 工具通过 tool_search → tools_call 延迟发现。
    let mut arr: Vec<Value> = builtin_tools()
        .into_iter()
        .filter(|tool| {
            tool.exposure.mole_agent
                && (tool.exposure.pinned || extra.contains(&tool.name))
                && resolution.builtin_enabled(&tool.name)
        })
        .map(|t| tool_schema_entry(provider, &t.name, &t.description, &t.input_schema))
        .collect();

    for name in extra {
        if let Some(tool) = resolution.mcp_tool(name) {
            arr.push(tool_schema_entry(
                provider,
                &tool.name,
                &tool.description,
                &tool.input_schema,
            ));
        }
    }

    arr.sort_by(|left, right| tool_schema_name(left).cmp(tool_schema_name(right)));
    arr.dedup_by(|left, right| tool_schema_name(left) == tool_schema_name(right));
    Value::Array(arr)
}

fn tool_schema_name(value: &Value) -> &str {
    value
        .get("name")
        .or_else(|| value.pointer("/function/name"))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// 单个工具 schema 条目（按 provider 期望格式）。
fn tool_schema_entry(
    provider: Provider,
    name: &str,
    description: &str,
    input_schema: &Value,
) -> Value {
    match provider {
        Provider::Anthropic => json!({
            "name": name,
            "description": description,
            "input_schema": input_schema,
        }),
        Provider::OpenAi | Provider::OpenAiCompatible => json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description,
                "parameters": input_schema,
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_tools_schema_has_function_wrapper() {
        let schema = tools_schema_for(
            Provider::OpenAi,
            &toolsets::ToolsetResolution::default(),
            &[],
        );
        let arr = schema.as_array().unwrap();
        assert!(!arr.is_empty());
        assert_eq!(arr[0]["type"], "function");
        assert!(arr[0]["function"]["name"].is_string());
    }

    #[test]
    fn a_skill_shows_the_tools_it_names_but_never_a_disabled_one() {
        let resolution = toolsets::ToolsetResolution {
            builtin_allowed: Some(
                ["load_skill", "list_streams"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
            ),
            ..toolsets::ToolsetResolution::default()
        };

        let names = |extra: &[String]| {
            tools_schema_for(Provider::Anthropic, &resolution, extra)
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| tool_schema_name(entry).to_string())
                .collect::<Vec<_>>()
        };

        let extra = ["load_skill", "query_logs", "no_such_tool"].map(String::from);
        let shown = names(&extra);

        assert!(shown.contains(&"load_skill".to_string()));
        // query_logs is not allowed by the organization's toolset: a skill cannot open it.
        assert!(!shown.contains(&"query_logs".to_string()));
        assert!(!shown.contains(&"no_such_tool".to_string()));
        assert!(!names(&[]).contains(&"load_skill".to_string()));
    }

    #[test]
    fn anthropic_tools_schema_uses_input_schema() {
        let schema = tools_schema_for(
            Provider::Anthropic,
            &toolsets::ToolsetResolution::default(),
            &[],
        );
        let arr = schema.as_array().unwrap();
        assert!(arr[0]["input_schema"].is_object());
        assert!(arr[0]["name"].is_string());
    }
}
