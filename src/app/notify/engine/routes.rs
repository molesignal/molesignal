// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::{NotifyEngine, model::ResolvedRoute};
use crate::{
    app::notify::quiet_hours_active,
    domain::notify::{
        connector::{ConnectorStatus, NotifyTarget, NotifyTargetType},
        delivery::DeliveryStage,
        policy::{NotifyEvent, NotifyPolicy},
        recipient::NotifyRecipient,
        routing::NotifyDefaultRoute,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

impl NotifyEngine {
    pub(super) async fn resolve_routes(
        &self,
        policy: &NotifyPolicy,
        event: &NotifyEvent,
        recipient: &NotifyRecipient,
    ) -> Result<Vec<ResolvedRoute>> {
        let mut routes = match policy.delivery_mode {
            crate::domain::notify::policy::NotifyDeliveryMode::PreferUser => {
                self.user_routes(policy, event, recipient).await?
            }
            crate::domain::notify::policy::NotifyDeliveryMode::ForceConnector
            | crate::domain::notify::policy::NotifyDeliveryMode::MultiConnector => {
                self.configured_user_routes(policy, event, recipient)
                    .await?
            }
        };
        if policy.fallback_config.use_team_defaults
            && let Some(team_id) = recipient
                .team_id
                .clone()
                .or_else(|| event_team_id(&event.attributes))
            && let Some(defaults) = self
                .team_defaults
                .get(&event.organization_id, &team_id, policy.category)
                .await?
            && defaults.enabled
        {
            routes.extend(
                self.default_routes(
                    &event.organization_id,
                    defaults.routes,
                    DeliveryStage::TeamFallback,
                )
                .await?,
            );
        }
        if policy.fallback_config.use_organization_defaults
            && let Some(defaults) = self
                .organization_defaults
                .get(&event.organization_id, policy.category)
                .await?
            && defaults.enabled
        {
            routes.extend(
                self.default_routes(
                    &event.organization_id,
                    defaults.routes,
                    DeliveryStage::OrganizationFallback,
                )
                .await?,
            );
        }
        Ok(routes)
    }

    async fn user_routes(
        &self,
        policy: &NotifyPolicy,
        event: &NotifyEvent,
        recipient: &NotifyRecipient,
    ) -> Result<Vec<ResolvedRoute>> {
        let Some(mut preference) = self
            .preferences
            .get(&event.organization_id, &recipient.user_id, policy.category)
            .await?
        else {
            return Ok(Vec::new());
        };
        if !preference.enabled || preference_is_quiet(&preference, event) {
            return Ok(Vec::new());
        }
        preference.steps.sort_by_key(|step| step.step_order);
        let max_steps = if policy.fallback_config.use_user_fallbacks {
            usize::MAX
        } else {
            1
        };
        let mut routes = Vec::new();
        for (index, step) in preference.steps.into_iter().take(max_steps).enumerate() {
            let endpoint = self
                .endpoints
                .get(
                    &event.organization_id,
                    &recipient.user_id,
                    &step.endpoint_id,
                )
                .await?;
            if !endpoint.enabled || !endpoint.verified {
                continue;
            }
            let connector = self
                .connectors
                .get(&event.organization_id, &endpoint.connector_id)
                .await?;
            if !connector.enabled || connector.status == ConnectorStatus::Error {
                continue;
            }
            let target = NotifyTarget {
                target_type: NotifyTargetType::DirectUser,
                value: endpoint.external_identity,
                metadata: Default::default(),
            };
            self.connector_registry
                .get(&connector.connector_type)?
                .validate_target(&target)?;
            routes.push(ResolvedRoute {
                stage: if index == 0 {
                    DeliveryStage::UserPrimary
                } else {
                    DeliveryStage::UserFallback
                },
                connector,
                endpoint_id: Some(endpoint.id),
                target,
            });
        }
        Ok(routes)
    }

    async fn configured_user_routes(
        &self,
        policy: &NotifyPolicy,
        event: &NotifyEvent,
        recipient: &NotifyRecipient,
    ) -> Result<Vec<ResolvedRoute>> {
        if let Some(preference) = self
            .preferences
            .get(&event.organization_id, &recipient.user_id, policy.category)
            .await?
            && (!preference.enabled || preference_is_quiet(&preference, event))
        {
            return Ok(Vec::new());
        }
        let endpoints = self
            .endpoints
            .list(&event.organization_id, &recipient.user_id)
            .await?;
        let mut routes = Vec::new();
        let mut selected_endpoint_ids = std::collections::HashSet::new();
        for connector_id in &policy.delivery_config.connector_ids {
            let Some(endpoint) = endpoints.iter().find(|endpoint| {
                endpoint.connector_id == *connector_id
                    && endpoint.enabled
                    && endpoint.verified
                    && selected_endpoint_ids.insert(endpoint.id.clone())
            }) else {
                continue;
            };
            let connector = self
                .connectors
                .get(&event.organization_id, connector_id)
                .await?;
            if !connector.enabled || connector.status == ConnectorStatus::Error {
                continue;
            }
            let target = NotifyTarget {
                target_type: NotifyTargetType::DirectUser,
                value: endpoint.external_identity.clone(),
                metadata: Default::default(),
            };
            self.connector_registry
                .get(&connector.connector_type)?
                .validate_target(&target)?;
            routes.push(ResolvedRoute {
                stage: DeliveryStage::UserPrimary,
                connector,
                endpoint_id: Some(endpoint.id.clone()),
                target,
            });
        }
        if policy.fallback_config.use_user_fallbacks {
            for mut route in self.user_routes(policy, event, recipient).await? {
                if route
                    .endpoint_id
                    .as_ref()
                    .is_some_and(|id| selected_endpoint_ids.contains(id))
                {
                    continue;
                }
                route.stage = DeliveryStage::UserFallback;
                routes.push(route);
            }
        }
        Ok(routes)
    }

    async fn default_routes(
        &self,
        organization_id: &Id,
        mut configured: Vec<NotifyDefaultRoute>,
        stage: DeliveryStage,
    ) -> Result<Vec<ResolvedRoute>> {
        configured.sort_by_key(|route| route.order);
        let mut routes = Vec::new();
        for route in configured {
            let connector = self
                .connectors
                .get(organization_id, &route.connector_id)
                .await?;
            if !connector.enabled || connector.status == ConnectorStatus::Error {
                continue;
            }
            let target = NotifyTarget {
                target_type: route.target_type,
                value: route.target,
                metadata: Default::default(),
            };
            self.connector_registry
                .get(&connector.connector_type)?
                .validate_target(&target)?;
            routes.push(ResolvedRoute {
                stage,
                connector,
                endpoint_id: None,
                target,
            });
        }
        Ok(routes)
    }
}

fn preference_is_quiet(
    preference: &crate::domain::notify::preference::UserNotifyPreference,
    event: &NotifyEvent,
) -> bool {
    let critical_bypass = preference.allow_critical_bypass
        && event
            .attributes
            .get("severity")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|severity| severity.eq_ignore_ascii_case("critical"));
    !critical_bypass
        && preference
            .quiet_hours
            .as_ref()
            .is_some_and(|quiet_hours| quiet_hours_active(quiet_hours, TimestampMicros::now()))
}

fn event_team_id(attributes: &serde_json::Value) -> Option<Id> {
    attributes
        .get("team_id")
        .or_else(|| attributes.get("teamId"))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(Id::from_string)
}
