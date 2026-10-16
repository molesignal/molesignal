// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;

use crate::{domain::alerting::incident::Incident, shared::Result};

#[async_trait]
pub trait AlertIncidentLifecycleSink: Send + Sync {
    async fn on_incident_changed(&self, incident: &Incident) -> Result<()>;
}

/// Hands every incident change to several sinks; each sees it whatever the
/// others did, and the first failure is reported.
pub struct AlertIncidentLifecycleSinks {
    sinks: Vec<std::sync::Arc<dyn AlertIncidentLifecycleSink>>,
}

impl AlertIncidentLifecycleSinks {
    pub fn new(sinks: Vec<std::sync::Arc<dyn AlertIncidentLifecycleSink>>) -> Self {
        Self { sinks }
    }
}

#[async_trait]
impl AlertIncidentLifecycleSink for AlertIncidentLifecycleSinks {
    async fn on_incident_changed(&self, incident: &Incident) -> Result<()> {
        let mut first_error = None;

        for sink in &self.sinks {
            if let Err(error) = sink.on_incident_changed(incident).await {
                first_error.get_or_insert(error);
            }
        }

        first_error.map_or(Ok(()), Err)
    }
}
