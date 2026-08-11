// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cancellation-routing lifetime for one federated query.

use std::sync::Arc;

use crate::infra::query::federation_cancel::FederationCancelRegistry;

pub(super) struct DispatchGuard {
    registry: Arc<FederationCancelRegistry>,
    federation_id: String,
}

impl DispatchGuard {
    pub(super) fn new(registry: Arc<FederationCancelRegistry>, federation_id: String) -> Self {
        Self {
            registry,
            federation_id,
        }
    }
}

impl Drop for DispatchGuard {
    fn drop(&mut self) {
        self.registry.clear_dispatch(&self.federation_id);
    }
}
