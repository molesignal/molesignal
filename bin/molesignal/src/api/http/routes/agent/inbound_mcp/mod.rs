// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Authenticated Inbound MCP Streamable HTTP adapter and management API.

use std::sync::Arc;

use axum::{Extension, Router};

use crate::api::AppState;

mod admission;
mod handler;
mod http;
mod prompts;
mod resources;
mod runtime;
mod settings;
mod tasks;
mod tools;

pub fn routes(state: AppState) -> Router<AppState> {
    let runtime = Arc::new(runtime::InboundMcpAdapterRuntime::new(
        state.agent.inbound_mcp_request_state_key,
    ));
    Router::new()
        .merge(settings::routes())
        .merge(http::routes(state, runtime.clone()))
        .layer(Extension(runtime))
}
