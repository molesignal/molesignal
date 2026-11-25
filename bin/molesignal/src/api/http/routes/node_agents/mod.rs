// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/node-agents`: managed Node Agents of the organization — enrollment tokens, the inventory,
//! drain, revocation, configuration, tasks and the task quota. Agents themselves talk to the
//! `NodeControl` gRPC endpoint, never to these routes.

mod nodes;
mod tasks;

use axum::Router;

use crate::api::AppState;

pub(super) fn routes() -> Router<AppState> {
    Router::new().merge(nodes::routes()).merge(tasks::routes())
}
