// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/api/v1/agent/*` 路由聚合。

use axum::Router;

use crate::api::AppState;

pub(crate) mod builtin_execution;
pub mod chat;
pub mod control;
pub mod dashboard_drafts;
pub mod inbound_mcp;
pub mod mcp;
pub mod model_providers;
pub mod prompts;
pub mod telemetry;
pub mod tool_dispatcher;
pub mod tools_control;
pub mod toolsets;

pub fn routes(state: AppState) -> Router<AppState> {
    Router::new()
        .merge(chat::routes())
        .merge(control::routes())
        .merge(dashboard_drafts::routes())
        .merge(inbound_mcp::routes(state))
        .merge(model_providers::routes())
        .merge(mcp::routes())
        .merge(prompts::routes())
        .merge(telemetry::routes())
        .merge(tools_control::routes())
        .merge(toolsets::routes())
}
