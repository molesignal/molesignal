// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IAM directory and fine-grained access-control routes.

use axum::Router;

use crate::api::AppState;

mod access;
pub(super) mod directory;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(directory::routes())
        .merge(access::routes())
}
