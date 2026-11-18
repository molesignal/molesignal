// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Network-independent rules for external MCP servers: which headers and environment variables are
//! sensitive, server prefixes within an organization, importing other clients' configurations, and
//! filling in prompt arguments. Server and tool records are in [`crate::tool_control`]; connection,
//! synchronization, and execution are in the server's `app::mcp`.

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
