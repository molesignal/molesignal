// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod escalation;
mod events;
mod execution;
mod management;
mod model;
mod routes;
mod template;
#[cfg(test)]
mod tests;

use std::sync::Arc;

pub use model::{
    NotifyDefaultInput, NotifyDeliveryPlanStep, NotifyDispatch, NotifyEventOutcome,
    NotifyPolicyInput, NotifyPolicyOutcome, NotifyPolicyPreview, NotifyRecipientOutcome,
    NotifyRecipientPlan,
};
pub use template::validate_notify_template_body;

use super::{ConnectorRegistry, RecipientResolverRegistry};
use crate::domain::{
    iam::TeamRepository,
    notify::repositories::{
        NotifyConnectorRepository, NotifyDeliveryRepository, NotifyEventRepository,
        NotifyPolicyRepository, NotifyTemplateRepository, OrganizationNotifyDefaultRepository,
        TeamNotifyDefaultRepository, UserNotifyEndpointRepository, UserNotifyPreferenceRepository,
    },
};

pub struct NotifyEngineDependencies {
    pub connectors: Arc<dyn NotifyConnectorRepository>,
    pub endpoints: Arc<dyn UserNotifyEndpointRepository>,
    pub preferences: Arc<dyn UserNotifyPreferenceRepository>,
    pub deliveries: Arc<dyn NotifyDeliveryRepository>,
    pub events: Arc<dyn NotifyEventRepository>,
    pub policies: Arc<dyn NotifyPolicyRepository>,
    pub team_defaults: Arc<dyn TeamNotifyDefaultRepository>,
    pub organization_defaults: Arc<dyn OrganizationNotifyDefaultRepository>,
    pub teams: Arc<dyn TeamRepository>,
    pub templates: Arc<dyn NotifyTemplateRepository>,
    pub connector_registry: Arc<ConnectorRegistry>,
    pub resolver_registry: Arc<RecipientResolverRegistry>,
}

pub struct NotifyEngine {
    connectors: Arc<dyn NotifyConnectorRepository>,
    endpoints: Arc<dyn UserNotifyEndpointRepository>,
    preferences: Arc<dyn UserNotifyPreferenceRepository>,
    deliveries: Arc<dyn NotifyDeliveryRepository>,
    events: Arc<dyn NotifyEventRepository>,
    policies: Arc<dyn NotifyPolicyRepository>,
    team_defaults: Arc<dyn TeamNotifyDefaultRepository>,
    organization_defaults: Arc<dyn OrganizationNotifyDefaultRepository>,
    teams: Arc<dyn TeamRepository>,
    templates: Arc<dyn NotifyTemplateRepository>,
    connector_registry: Arc<ConnectorRegistry>,
    resolver_registry: Arc<RecipientResolverRegistry>,
}

impl NotifyEngine {
    pub fn new(dependencies: NotifyEngineDependencies) -> Self {
        Self {
            connectors: dependencies.connectors,
            endpoints: dependencies.endpoints,
            preferences: dependencies.preferences,
            deliveries: dependencies.deliveries,
            events: dependencies.events,
            policies: dependencies.policies,
            team_defaults: dependencies.team_defaults,
            organization_defaults: dependencies.organization_defaults,
            teams: dependencies.teams,
            templates: dependencies.templates,
            connector_registry: dependencies.connector_registry,
            resolver_registry: dependencies.resolver_registry,
        }
    }

    pub fn supported_recipient_resolvers(&self) -> Vec<&'static str> {
        self.resolver_registry.supported_types()
    }
}
