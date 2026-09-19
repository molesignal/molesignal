// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;

use crate::{domain::alerting::incident::Incident, shared::Result};

#[async_trait]
pub trait AlertIncidentLifecycleSink: Send + Sync {
    async fn on_incident_changed(&self, incident: &Incident) -> Result<()>;
}
