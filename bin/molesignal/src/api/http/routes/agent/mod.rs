// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/api/v1/agent/*` 路由聚合。

use axum::Router;

use crate::api::AppState;

pub(super) mod budget;
pub(crate) mod builtin_execution;
pub mod chat;
pub mod commands;
pub mod control;
mod currencies;
pub mod dashboard_drafts;
pub mod feedback;
pub mod inbound_mcp;
pub mod investigate;
pub mod knowledge;
pub mod mcp;
pub mod memory;
pub mod model_providers;
pub mod network_policy;
pub mod prompts;
pub mod references;
pub mod runs;
pub mod skills;
pub(crate) mod team_resources;
pub mod telemetry;
pub mod tool_dispatcher;
pub mod tools_control;
pub mod toolsets;
pub mod usage;
pub(crate) mod viewer;

pub fn routes(state: AppState) -> Router<AppState> {
    Router::new()
        .merge(chat::routes())
        .merge(commands::routes())
        .merge(control::routes())
        .merge(currencies::routes())
        .merge(dashboard_drafts::routes())
        .merge(feedback::routes())
        .merge(inbound_mcp::routes(state))
        .merge(investigate::routes())
        .merge(knowledge::routes())
        .merge(memory::routes())
        .merge(model_providers::routes())
        .merge(network_policy::routes())
        .merge(mcp::routes())
        .merge(prompts::routes())
        .merge(references::routes())
        .merge(runs::routes())
        .merge(skills::routes())
        .merge(telemetry::routes())
        .merge(tools_control::routes())
        .merge(toolsets::routes())
        .merge(usage::routes())
}
