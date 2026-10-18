// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 延迟发现：`tool_search` 在组织允许的内置与 MCP 工具里按名称、标签与说明打分查找，
//! `tools_call` 调用找到的工具。只有常用工具直接交给模型，其余都经这两个工具。

use std::cmp::Reverse;

use serde::Deserialize;
use serde_json::{Value, json};

use super::RealToolDispatcher;
use crate::{
    agent::tools::{
        ToolAuthContext, ToolCall, ToolDispatcher, ToolResult, builtin_tools, is_meta_tool,
    },
    shared::{Error, Result},
};

impl RealToolDispatcher {
    pub(super) fn tool_search(&self, arguments: Value) -> Result<ToolResult> {
        let args: ToolSearchArgs = serde_json::from_value(arguments)
            .map_err(|error| Error::invalid(format!("invalid tool arguments: {error}")))?;
        let query = args.query.unwrap_or_default().trim().to_ascii_lowercase();
        let domain = args.domain.as_deref().map(str::to_ascii_lowercase);

        let mut entries = builtin_tools()
            .into_iter()
            .filter(|tool| {
                tool.exposure.mole_agent
                    && !is_meta_tool(&tool.name)
                    && self.resolution.builtin_enabled(&tool.name)
            })
            .filter(|tool| domain.as_ref().is_none_or(|domain| tool.domain.eq_ignore_ascii_case(domain)))
            .filter_map(|tool| {
                let score = search_score(
                    &query,
                    &tool.name,
                    &tool.display_name,
                    &tool.description,
                    &tool.domain,
                    &tool.tags,
                )?;

                Some((score, json!({
                    "name": tool.name, "display_name": tool.display_name,
                    "description": tool.description,
                    "domain": tool.domain, "category": tool.category, "tags": tool.tags,
                    "required_permissions": tool.required_permissions,
                    "permission_mode": tool.permission_mode,
                    "execution_mode": self.resolution.execution_mode_for_builtin(&tool.name),
                    "input_schema": args.include_schema.unwrap_or(true).then_some(tool.input_schema),
                    "output_schema": args.include_schema.unwrap_or(true).then_some(tool.output_schema),
                })))
            })
            .collect::<Vec<_>>();

        entries.extend(
            self.resolution
                .mcp_tools
                .values()
                .filter(|tool| self.resolution.mcp_tool(&tool.name).is_some())
                .filter(|_| domain.as_ref().is_none_or(|domain| domain == "mcp"))
                .filter_map(|tool| {
                    let score = search_score(
                        &query,
                        &tool.name,
                        &tool.display_name,
                        &tool.description,
                        "mcp",
                        &tool.tags,
                    )?;

                    Some((score, json!({
                        "name": tool.name, "display_name": tool.display_name,
                        "description": tool.description, "tags": tool.tags,
                        "risk": tool.risk, "source": "mcp",
                        "execution_mode": self.resolution.execution_mode_for_mcp(tool),
                        "input_schema": args.include_schema.unwrap_or(true).then_some(tool.input_schema.clone()),
                        "output_schema": args.include_schema.unwrap_or(true).then_some(tool.output_schema.clone()),
                    })))
                }),
        );

        entries.sort_by_key(|(score, value)| {
            (
                Reverse(*score),
                value["name"].as_str().unwrap_or_default().to_string(),
            )
        });

        let limit = args.limit.unwrap_or(10).clamp(1, 50);
        entries.truncate(limit);

        Ok(ToolResult::json(json!({
            "query": query, "tools": entries.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
        })))
    }

    pub(super) async fn tools_call(
        &self,
        ctx: &ToolAuthContext,
        arguments: Value,
    ) -> Result<ToolResult> {
        let args: ToolsCallArgs = serde_json::from_value(arguments)
            .map_err(|error| Error::invalid(format!("invalid tool arguments: {error}")))?;

        if is_meta_tool(&args.name) {
            return Ok(ToolResult::error("meta tools cannot call themselves"));
        }

        Box::pin(self.dispatch(
            ctx,
            ToolCall {
                name: args.name,
                arguments: args.arguments,
            },
        ))
        .await
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolSearchArgs {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    include_schema: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolsCallArgs {
    name: String,
    arguments: Value,
}

fn search_score(
    query: &str,
    name: &str,
    display_name: &str,
    description: &str,
    domain: &str,
    tags: &[String],
) -> Option<u16> {
    if query.is_empty() {
        return Some(1);
    }

    let name = name.to_ascii_lowercase();

    if name == query {
        return Some(1_000);
    }

    let display_name = display_name.to_ascii_lowercase();
    let description = description.to_ascii_lowercase();
    let domain = domain.to_ascii_lowercase();

    query.split_whitespace().try_fold(0u16, |score, token| {
        let token_score = u16::from(name.contains(token)) * 300
            + u16::from(display_name.contains(token)) * 160
            + u16::from(domain.contains(token)) * 120
            + u16::from(
                tags.iter()
                    .any(|tag| tag.to_ascii_lowercase().contains(token)),
            ) * 100
            + u16::from(description.contains(token)) * 40;

        (token_score > 0).then_some(score.saturating_add(token_score))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_prefers_exact_name_and_matches_tags() {
        assert_eq!(
            search_score("get_trace", "get_trace", "", "", "", &[]),
            Some(1_000)
        );
        assert!(search_score("traces", "x", "", "", "", &["Traces".into()]).is_some());
        assert_eq!(search_score("missing", "x", "", "", "", &[]), None);
    }
}
