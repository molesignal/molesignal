// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[agent]`：Mole Agent 的实例级默认值。

use serde::{Deserialize, Serialize};

/// `[agent]`：Mole Agent。组织管理员可以在 Mole Agent 设置里为本组织另设预算。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSettings {
    /// 每个组织每天可用的模型 token（输入加输出，按 UTC 自然日计）；0 表示不限。
    #[serde(default)]
    pub daily_token_budget: u64,
    /// 用到上限的这个百分比时开始提醒；小于 1 按 1、大于 100 按 100 处理。
    #[serde(default = "default_budget_warning_percent")]
    pub budget_warning_percent: u8,
    /// 实例级 Skill 目录：每个含 `SKILL.md` 的子目录是一个 Skill，所有组织可见、只读，组织
    /// 可以停用。Kubernetes 用 ConfigMap、PVC 或 git-sync 挂载。
    #[serde(default)]
    pub skill_paths: Vec<String>,
    /// 每个节点同时执行的回答（Run）数；等提问人回答或确认的不占名额。0 按 1 处理。
    #[serde(default = "default_max_concurrent_runs")]
    pub max_concurrent_runs: usize,
    /// 模型能读的上下文（token）。一轮回答交给模型的历史超过它的 80% 时，先把更早的对话压缩
    /// 成摘要。
    #[serde(default = "default_max_context_tokens")]
    pub max_context_tokens: u64,
    /// 组织级 stdio MCP 服务器可以在服务端起的程序，与服务器命令行的第一个词原样比较。空（默认）
    /// 表示服务端不起任何 stdio 服务器。
    #[serde(default)]
    pub mcp_stdio_commands: Vec<String>,
}

fn default_budget_warning_percent() -> u8 {
    80
}

fn default_max_concurrent_runs() -> usize {
    8
}

fn default_max_context_tokens() -> u64 {
    128_000
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            daily_token_budget: 0,
            budget_warning_percent: default_budget_warning_percent(),
            skill_paths: Vec::new(),
            max_concurrent_runs: default_max_concurrent_runs(),
            max_context_tokens: default_max_context_tokens(),
            mcp_stdio_commands: Vec::new(),
        }
    }
}
