// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Slash commands in Mole Agent conversations.
//!
//! Conversation input starting with `/name arguments` is a command. Control commands only change
//! input state (mode, time, model, etc.) and run on the frontend without being sent to the model;
//! task commands go to the backend for one run. The backend parses and validates them here, records
//! the original text and command on the user message, and sends the expanded question to the model.
//! Unknown commands return an error without being sent to the model.

mod builtin;
mod parse;

pub use builtin::{Argument, BUILTIN_COMMANDS, BuiltinCommand, Suggest, find_builtin};
pub use parse::{Input, Invocation, parse, text_for_model};
use serde::{Deserialize, Serialize};

/// Who executes the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    /// Changes input state on the frontend without being sent to the backend.
    Control,
    /// Sends to the backend for one run.
    Task,
}

/// Where the command comes from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandSource {
    #[default]
    Builtin,
    Skill,
    /// A prompt provided by an MCP server: `/<server>:<prompt>`.
    Mcp,
}

/// The command recorded on a user message for replay during regeneration and attribution during
/// rating.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandRecord {
    /// The spelling used at invocation; a Skill may include `@team`.
    pub name: String,
    pub args: String,
    pub source: CommandSource,
    /// The text passed to the model: the expanded question.
    pub model_text: String,
    /// The incident investigated by `/rca`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incident_id: Option<String>,
    /// The invoked Skill and its version and content hash at the time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_hash: Option<String>,
    /// The invoked MCP prompt: its server and name on that server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_server_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_prompt: Option<String>,
}
