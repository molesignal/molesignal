// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Which calls only read, so the Agent loop may run them together with the calls next to them in
//! the same model reply.

use serde_json::Value;
use tool_runtime::{
    ToolAccess, ToolExecutionMode,
    catalog::{BuiltinToolKind, is_meta_tool, is_session_tool, is_skill_tool, is_workspace_tool},
};

use super::RealToolDispatcher;

impl RealToolDispatcher {
    /// A builtin tool that only reads, is on for this chat and runs without asking anyone, or
    /// `tools_call` passing on such a tool. Skill, session, knowledge and memory tools follow the
    /// chat, and MCP tools reach systems outside: they always run alone.
    pub(super) fn reads_only(&self, name: &str, arguments: &Value) -> bool {
        if name == BuiltinToolKind::ToolsCall.name() {
            return self.resolution.builtin_enabled(name)
                && arguments["name"]
                    .as_str()
                    .is_some_and(|inner| self.reads_only_builtin(inner));
        }

        self.reads_only_builtin(name)
    }

    fn reads_only_builtin(&self, name: &str) -> bool {
        let Some(kind) = BuiltinToolKind::from_name(name) else {
            return false;
        };

        if is_meta_tool(name) || is_skill_tool(name) || is_session_tool(name) {
            return false;
        }

        if is_workspace_tool(name) {
            return false;
        }

        let spec = kind.spec();

        spec.access == ToolAccess::ReadOnly
            && spec.exposure.mole_agent
            && self.resolution.builtin_enabled(name)
            && self.resolution.execution_mode_for_builtin(name) == ToolExecutionMode::Automatic
    }
}
