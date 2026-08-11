// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashSet, sync::Arc};

use serde_json::Value;

mod testing;

use super::{
    ConnectorRegistry,
    config::{
        merge_masked_config, normalize_object, normalize_optional_text, validate_endpoint_identity,
        validate_name,
    },
    quiet::validate_quiet_hours,
};
use crate::{
    domain::notify::{
        connector::{ConnectorStatus, NotifyConnector},
        delivery::{DeliveryFilter, NotifyDelivery},
        endpoint::UserNotifyEndpoint,
        preference::{NotifyCategory, UserNotifyPreference, UserNotifyPreferenceStep},
        repositories::{
            NotifyConnectorRepository, NotifyDeliveryRepository, NotifyRouteReferenceRepository,
            UserNotifyEndpointRepository, UserNotifyPreferenceRepository,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone)]
pub struct CreateNotifyConnector {
    pub name: String,
    pub connector_type: String,
    pub config: Value,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct UpdateNotifyConnector {
    pub name: String,
    /// `None` 保留全部现有配置；对象中的敏感字段值 `"***"` 也会保留原值。
    pub config: Option<Value>,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct CreateUserNotifyEndpoint {
    pub connector_id: Id,
    pub external_identity: String,
    pub display_name: Option<String>,
    pub metadata: Value,
    pub verified: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct UpdateUserNotifyEndpoint {
    pub connector_id: Id,
    pub external_identity: String,
    pub display_name: Option<String>,
    pub metadata: Value,
    pub verified: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct ConnectorTestOutcome {
    pub sent: bool,
    pub tested_at: TimestampMicros,
    pub elapsed_ms: u64,
    pub provider_message_id: Option<String>,
    pub error: Option<String>,
}

pub struct NotifyService {
    connectors: Arc<dyn NotifyConnectorRepository>,
    endpoints: Arc<dyn UserNotifyEndpointRepository>,
    preferences: Arc<dyn UserNotifyPreferenceRepository>,
    deliveries: Arc<dyn NotifyDeliveryRepository>,
    route_references: Arc<dyn NotifyRouteReferenceRepository>,
    registry: Arc<ConnectorRegistry>,
}

impl NotifyService {
    pub fn new(
        connectors: Arc<dyn NotifyConnectorRepository>,
        endpoints: Arc<dyn UserNotifyEndpointRepository>,
        preferences: Arc<dyn UserNotifyPreferenceRepository>,
        deliveries: Arc<dyn NotifyDeliveryRepository>,
        route_references: Arc<dyn NotifyRouteReferenceRepository>,
        registry: Arc<ConnectorRegistry>,
    ) -> Self {
        Self {
            connectors,
            endpoints,
            preferences,
            deliveries,
            route_references,
            registry,
        }
    }

    pub fn supported_connector_types(
        &self,
    ) -> Vec<(
        &'static str,
        crate::domain::notify::connector::ConnectorCapabilities,
    )> {
        self.registry.supported_types()
    }

    pub async fn create_connector(
        &self,
        organization_id: &Id,
        input: CreateNotifyConnector,
    ) -> Result<NotifyConnector> {
        validate_name(&input.name)?;
        let adapter = self.registry.get(&input.connector_type)?;
        adapter.validate_config(&input.config)?;
        let now = TimestampMicros::now();
        self.connectors
            .create(NotifyConnector {
                id: Id::new(),
                organization_id: organization_id.clone(),
                name: input.name,
                connector_type: input.connector_type,
                config: input.config,
                capabilities: adapter.capabilities(),
                enabled: input.enabled,
                status: ConnectorStatus::Unknown,
                last_tested_at: None,
                last_test_status: None,
                last_test_error: None,
                created_at: now,
                updated_at: now,
            })
            .await
    }

    pub async fn get_connector(&self, organization_id: &Id, id: &Id) -> Result<NotifyConnector> {
        self.connectors.get(organization_id, id).await
    }

    pub async fn list_connectors(&self, organization_id: &Id) -> Result<Vec<NotifyConnector>> {
        self.connectors.list(organization_id).await
    }

    pub async fn update_connector(
        &self,
        organization_id: &Id,
        id: &Id,
        input: UpdateNotifyConnector,
    ) -> Result<NotifyConnector> {
        validate_name(&input.name)?;
        let mut connector = self.connectors.get(organization_id, id).await?;
        connector.config = match input.config {
            Some(config) => merge_masked_config(&connector.config, &config),
            None => connector.config,
        };
        let adapter = self.registry.get(&connector.connector_type)?;
        adapter.validate_config(&connector.config)?;
        connector.name = input.name;
        connector.enabled = input.enabled;
        connector.capabilities = adapter.capabilities();
        connector.updated_at = TimestampMicros::now();
        self.connectors.update(connector).await
    }

    pub async fn delete_connector(&self, organization_id: &Id, id: &Id) -> Result<()> {
        let endpoint_references = self
            .endpoints
            .count_for_connector(organization_id, id)
            .await?;
        let route_references = self
            .route_references
            .count_for_connector(organization_id, id)
            .await?;
        if endpoint_references > 0 || route_references > 0 {
            return Err(Error::conflict(
                "notify connector is referenced by user endpoints, policies, or default routes",
            ));
        }
        self.connectors.delete(organization_id, id).await
    }

    pub async fn create_endpoint(
        &self,
        organization_id: &Id,
        user_id: &Id,
        input: CreateUserNotifyEndpoint,
    ) -> Result<UserNotifyEndpoint> {
        let connector = self
            .connectors
            .get(organization_id, &input.connector_id)
            .await?;
        validate_endpoint_identity(
            self.registry.as_ref(),
            &connector.connector_type,
            &input.external_identity,
        )?;
        let now = TimestampMicros::now();
        self.endpoints
            .create(UserNotifyEndpoint {
                id: Id::new(),
                organization_id: organization_id.clone(),
                user_id: user_id.clone(),
                connector_id: connector.id,
                provider_type: connector.connector_type,
                external_identity: input.external_identity.trim().to_string(),
                display_name: normalize_optional_text(input.display_name),
                metadata: normalize_object(input.metadata, "endpoint metadata")?,
                verified: input.verified,
                enabled: input.enabled,
                created_at: now,
                updated_at: now,
            })
            .await
    }

    pub async fn get_endpoint(
        &self,
        organization_id: &Id,
        user_id: &Id,
        id: &Id,
    ) -> Result<UserNotifyEndpoint> {
        self.endpoints.get(organization_id, user_id, id).await
    }

    pub async fn list_endpoints(
        &self,
        organization_id: &Id,
        user_id: &Id,
    ) -> Result<Vec<UserNotifyEndpoint>> {
        self.endpoints.list(organization_id, user_id).await
    }

    pub async fn list_organization_endpoints(
        &self,
        organization_id: &Id,
    ) -> Result<Vec<UserNotifyEndpoint>> {
        self.endpoints.list_for_organization(organization_id).await
    }

    pub async fn update_endpoint(
        &self,
        organization_id: &Id,
        user_id: &Id,
        id: &Id,
        input: UpdateUserNotifyEndpoint,
    ) -> Result<UserNotifyEndpoint> {
        let mut endpoint = self.endpoints.get(organization_id, user_id, id).await?;
        let connector = self
            .connectors
            .get(organization_id, &input.connector_id)
            .await?;
        validate_endpoint_identity(
            self.registry.as_ref(),
            &connector.connector_type,
            &input.external_identity,
        )?;
        endpoint.connector_id = connector.id;
        endpoint.provider_type = connector.connector_type;
        endpoint.external_identity = input.external_identity.trim().to_string();
        endpoint.display_name = normalize_optional_text(input.display_name);
        endpoint.metadata = normalize_object(input.metadata, "endpoint metadata")?;
        endpoint.verified = input.verified;
        endpoint.enabled = input.enabled;
        endpoint.updated_at = TimestampMicros::now();
        self.endpoints.update(endpoint).await
    }

    pub async fn delete_endpoint(&self, organization_id: &Id, user_id: &Id, id: &Id) -> Result<()> {
        if self
            .preferences
            .list(organization_id, user_id)
            .await?
            .iter()
            .any(|preference| preference.steps.iter().any(|step| step.endpoint_id == *id))
        {
            return Err(Error::conflict(
                "user notify endpoint is referenced by a notify preference",
            ));
        }
        self.endpoints.delete(organization_id, user_id, id).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_preference(
        &self,
        organization_id: &Id,
        user_id: &Id,
        category: NotifyCategory,
        enabled: bool,
        endpoint_ids: Vec<Id>,
        quiet_hours: Option<Value>,
        allow_critical_bypass: bool,
    ) -> Result<UserNotifyPreference> {
        if endpoint_ids.len() > 20 {
            return Err(Error::invalid(
                "notify preference supports at most 20 endpoints",
            ));
        }
        let mut seen = HashSet::new();
        for endpoint_id in &endpoint_ids {
            if !seen.insert(endpoint_id.0.clone()) {
                return Err(Error::invalid(
                    "notify preference endpoint_ids must be unique",
                ));
            }
            self.endpoints
                .get(organization_id, user_id, endpoint_id)
                .await?;
        }
        let quiet_hours = quiet_hours
            .map(|value| -> Result<Value> {
                let value = normalize_object(value, "quiet_hours")?;
                validate_quiet_hours(&value)?;
                Ok(value)
            })
            .transpose()?;
        let existing = self
            .preferences
            .get(organization_id, user_id, category)
            .await?;
        let now = TimestampMicros::now();
        let (id, created_at) = existing
            .map(|value| (value.id, value.created_at))
            .unwrap_or_else(|| (Id::new(), now));
        let steps = endpoint_ids
            .into_iter()
            .enumerate()
            .map(|(index, endpoint_id)| UserNotifyPreferenceStep {
                id: Id::new(),
                preference_id: id.clone(),
                endpoint_id,
                step_order: i32::try_from(index + 1).unwrap_or(i32::MAX),
                created_at: now,
            })
            .collect();
        self.preferences
            .upsert(UserNotifyPreference {
                id,
                organization_id: organization_id.clone(),
                user_id: user_id.clone(),
                category,
                enabled,
                quiet_hours,
                allow_critical_bypass,
                steps,
                created_at,
                updated_at: now,
            })
            .await
    }

    pub async fn list_preferences(
        &self,
        organization_id: &Id,
        user_id: &Id,
    ) -> Result<Vec<UserNotifyPreference>> {
        self.preferences.list(organization_id, user_id).await
    }

    pub async fn list_organization_preferences(
        &self,
        organization_id: &Id,
    ) -> Result<Vec<UserNotifyPreference>> {
        self.preferences
            .list_for_organization(organization_id)
            .await
    }

    pub async fn record_delivery_once(&self, delivery: NotifyDelivery) -> Result<NotifyDelivery> {
        self.deliveries.record_once(delivery).await
    }

    pub async fn list_deliveries(
        &self,
        organization_id: &Id,
        filter: &DeliveryFilter,
    ) -> Result<Vec<NotifyDelivery>> {
        self.deliveries.list(organization_id, filter).await
    }

    pub async fn get_delivery(&self, organization_id: &Id, id: &Id) -> Result<NotifyDelivery> {
        self.deliveries.get(organization_id, id).await
    }
}
