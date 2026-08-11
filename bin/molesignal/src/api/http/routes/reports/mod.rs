// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Report template and scheduled-delivery routes.

use axum::Router;

use crate::api::AppState;

pub(super) mod scheduled;
pub(super) mod templates;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(templates::routes())
        .merge(scheduled::routes())
}
