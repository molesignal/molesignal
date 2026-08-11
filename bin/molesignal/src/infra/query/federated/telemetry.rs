// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::OnceLock;

static AUTH_ERRORS: OnceLock<prometheus::IntCounterVec> = OnceLock::new();

pub(super) fn auth_errors() -> &'static prometheus::IntCounterVec {
    AUTH_ERRORS.get_or_init(|| {
        crate::shared::metrics::register_int_counter_vec(
            "federated_search_auth_errors_total",
            "cross-cluster federated search auth failures by cluster",
            &["cluster"],
        )
    })
}
