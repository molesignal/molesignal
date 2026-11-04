// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::{NotifyEngine, model::ResolvedRoute};
use crate::{
    app::notify::quiet_hours_active,
    domain::{
        alerting::{importance::StepGroup, urgency::Urgency},
        notify::{
            connector::{ConnectorStatus, NotifyTarget, NotifyTargetType},
            delivery::DeliveryStage,
            policy::{NotifyEvent, NotifyPolicy},
            recipient::NotifyRecipient,
            routing::NotifyDefaultRoute,
        },
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

    pub(super) async fn user_routes(
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

        // 高、低紧急度各用自己那组步骤；低紧急度的没设时只用高紧急度的第一步。
        let steps: Vec<_> = preference
            .steps_for(event_steps(event))
            .into_iter()
            .cloned()
            .collect();
        preference.steps = steps;

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
                delay_secs: step.delay_secs,
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
                delay_secs: 0,
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
                delay_secs: 0,
            });
        }

        Ok(routes)
    }
}

/// 免打扰时段里：紧急度那一档说一律不打扰的就不打扰，说看开关的看个人的「免打扰时仍接收高紧急度」
/// （内置的低是前一种，高是后一种）。没有紧急度的通知（排班、换班等）按高紧急度算。
fn preference_is_quiet(
    preference: &crate::domain::notify::preference::UserNotifyPreference,
    event: &NotifyEvent,
) -> bool {
    let bypass = preference.allow_high_urgency_bypass && quiet_follows_switch(event);

    !bypass
        && preference
            .quiet_hours
            .as_ref()
            .is_some_and(|quiet_hours| quiet_hours_active(quiet_hours, TimestampMicros::now()))
}

/// 通知的紧急度：告警通知带着 `urgency`；别的通知没有，按高算。
fn event_urgency(event: &NotifyEvent) -> Urgency {
    event
        .attributes
        .get("urgency")
        .and_then(serde_json::Value::as_str)
        .and_then(Urgency::from_label)
        .unwrap_or_default()
}

fn attribute<'a>(event: &'a NotifyEvent, key: &str) -> Option<&'a str> {
    event
        .attributes
        .get(key)
        .and_then(serde_json::Value::as_str)
}

/// 用个人的哪组步骤：告警通知按组织的档位补上了 `urgency_steps`；没有时内置的低用低的一组，其余用
/// 高的一组。
fn event_steps(event: &NotifyEvent) -> StepGroup {
    attribute(event, "urgency_steps")
        .and_then(StepGroup::from_label)
        .unwrap_or_else(|| {
            if event_urgency(event).is_low() {
                StepGroup::Low
            } else {
                StepGroup::High
            }
        })
}

/// 免打扰时看不看个人的开关：告警通知按组织的档位补上了 `urgency_quiet`；没有时内置的低不看。
fn quiet_follows_switch(event: &NotifyEvent) -> bool {
    match attribute(event, "urgency_quiet") {
        Some(quiet) => quiet == "switch",
        None => !event_urgency(event).is_low(),
    }
}

fn event_team_id(attributes: &serde_json::Value) -> Option<Id> {
    attributes
        .get("team_id")
        .or_else(|| attributes.get("teamId"))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(Id::from_string)
}
