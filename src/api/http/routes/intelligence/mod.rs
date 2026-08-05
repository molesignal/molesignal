// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/api/v1/intelligence/*` 路由聚合。

use axum::Router;

use crate::api::AppState;

pub mod chat;
pub mod control;
pub mod dashboard_drafts;
pub mod mcp;
pub mod model_providers;
pub mod prompts;
pub mod telemetry;
pub mod tool_dispatcher;
pub mod tools_control;
pub mod toolsets;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(chat::routes())
        .merge(control::routes())
        .merge(dashboard_drafts::routes())
        .merge(model_providers::routes())
        .merge(mcp::routes())
        .merge(prompts::routes())
        .merge(telemetry::routes())
        .merge(tools_control::routes())
        .merge(toolsets::routes())
}
