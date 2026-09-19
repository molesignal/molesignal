// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Resource loaders used by alerting route permission attributes.

use async_trait::async_trait;

use crate::{
    api::{AppState, http::middleware::ProtectedResource},
    domain::alerting::{escalation::EscalationPolicy, incident::Incident, rule::AlertRule},
    shared::{Result, ids::Id},
};

#[async_trait]
impl ProtectedResource for AlertRule {
    type Id = Id;

    async fn load(state: &AppState, id: Self::Id) -> Result<Self> {
        state.alerting.service.get_rule(&id).await
    }

    fn organization_id(&self) -> &Id {
        &self.org_id
    }

    fn resource_type(&self) -> &str {
        "alert"
    }

    fn resource_id(&self) -> &str {
        self.id.as_str()
    }
}

#[async_trait]
impl ProtectedResource for Incident {
    type Id = Id;

    async fn load(state: &AppState, id: Self::Id) -> Result<Self> {
        state.alerting.service.get_incident(&id).await
    }

    fn organization_id(&self) -> &Id {
        &self.org_id
    }

    fn resource_type(&self) -> &str {
        "alert"
    }

    fn resource_id(&self) -> &str {
        self.id.as_str()
    }
}

#[async_trait]
impl ProtectedResource for EscalationPolicy {
    type Id = Id;

    async fn load(state: &AppState, id: Self::Id) -> Result<Self> {
        state.alerting.service.get_policy(&id).await
    }

    fn organization_id(&self) -> &Id {
        &self.org_id
    }

    fn resource_type(&self) -> &str {
        "alert"
    }

    fn resource_id(&self) -> &str {
        self.id.as_str()
    }
}
