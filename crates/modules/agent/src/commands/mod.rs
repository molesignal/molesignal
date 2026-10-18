// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 对话里的斜杠命令。
//!
//! 对话输入以 `/名称 参数` 开头时是命令。control 类命令只改输入框里的状态（模式、时间、
//! 模型……），由前端执行，不发给模型；task 类命令发给后端跑一轮，后端在这里解析、校验，把原文
//! 和命令一起记在用户消息上，模型拿到的是命令展开后的问题。未知命令直接报错，不发给模型。

mod builtin;
mod parse;

pub use builtin::{Argument, BUILTIN_COMMANDS, BuiltinCommand, Suggest, find_builtin};
pub use parse::{Input, Invocation, parse, text_for_model};
use serde::{Deserialize, Serialize};

/// 命令由谁执行。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    /// 改输入框里的状态，由前端执行，不发给后端。
    Control,
    /// 发给后端跑一轮。
    Task,
}

/// 命令从哪来。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandSource {
    #[default]
    Builtin,
    Skill,
    /// MCP 服务器提供的 prompt：`/<服务器>:<prompt>`。
    Mcp,
}

/// 记在用户消息上的命令：重新生成时按它重放，打分时按它归因。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CommandRecord {
    /// 调用时的写法，Skill 可能带 `@团队`。
    pub name: String,
    pub args: String,
    pub source: CommandSource,
    /// 交给模型的文字：命令展开后的问题。
    pub model_text: String,
    /// `/rca` 调查的事件。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incident_id: Option<String>,
    /// 调用的 Skill 及它当时的版本与内容 hash。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_hash: Option<String>,
    /// 调用的 MCP prompt：服务器与服务器上的名称。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_server_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_prompt: Option<String>,
}
