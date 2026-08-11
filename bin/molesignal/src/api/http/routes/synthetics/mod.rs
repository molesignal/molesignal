// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Organization-scoped Synthetics management API.

mod agents;
mod locations;
mod monitors;
mod results;
mod secrets;

use axum::Router;

use crate::api::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(agents::routes())
        .merge(locations::routes())
        .merge(monitors::routes())
        .merge(results::routes())
        .merge(secrets::routes())
}
