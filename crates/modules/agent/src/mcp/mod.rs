// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 外部 MCP 服务器上与网络无关的规则：哪些请求头和环境变量算敏感、服务器在组织里的前缀、
//! 导入其他客户端的配置、prompt 的参数怎么填。服务器与工具的记录在 [`crate::tool_control`]，
//! 连接、同步与执行在服务端的 `app::mcp`。

mod handle;
mod import;
mod prompts;
mod values;

pub use handle::{handle_base, prompt_command, tool_registry_name, unique_handle};
pub use import::{ImportedServer, parse_client_config};
pub use prompts::{McpPrompt, McpPromptArgument, bind_arguments, prompt_text};
pub use values::{
    NamedValue, NamedValueInput, ValueKind, find_reserved_overlap, last4, looks_secret,
    normalize_values,
};
