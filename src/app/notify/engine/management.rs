// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use super::{
    NotifyEngine,
    model::{NotifyDefaultInput, NotifyPolicyInput, now},
};
use crate::{
    app::notify::{config::validate_name, validate_matchers},
    domain::notify::{
        connector::{NotifyTarget, NotifyTargetType},
        policy::{NotifyDeliveryMode, NotifyEvent, NotifyPolicy},
        preference::NotifyCategory,
        routing::{OrganizationNotifyDefault, TeamNotifyDefault},
    },
    shared::{Error, Result, ids::Id},
};

impl NotifyEngine {
    pub async fn preview_policy_input(
        &self,
        organization_id: &Id,
        input: NotifyPolicyInput,
        event: NotifyEvent,
    ) -> Result<crate::app::notify::NotifyPolicyPreview> {
        if event.organization_id != *organization_id {
            return Err(Error::forbidden(
                "notify preview event organization does not match tenant context",
            ));
        }
        super::execution::validate_event(&event)?;
        let input = self.validate_policy_input(organization_id, input).await?;
        let now = now();
        self.preview_policy_record(
            policy_from_input(
                Id::from_string("notify-policy-preview"),
                organization_id.clone(),
                input,
                now,
                now,
            ),
            event,
        )
        .await
    }

    pub async fn create_policy(
        &self,
        organization_id: &Id,
        input: NotifyPolicyInput,
    ) -> Result<NotifyPolicy> {
        let input = self.validate_policy_input(organization_id, input).await?;
        let now = now();
        self.policies
            .create(policy_from_input(
                Id::new(),
                organization_id.clone(),
                input,
                now,
                now,
            ))
            .await
    }

    pub async fn update_policy(
        &self,
        organization_id: &Id,
        id: &Id,
        input: NotifyPolicyInput,
    ) -> Result<NotifyPolicy> {
        let input = self.validate_policy_input(organization_id, input).await?;
        let existing = self.policies.get(organization_id, id).await?;
        self.policies
            .update(policy_from_input(
                existing.id,
                organization_id.clone(),
                input,
                existing.created_at,
                now(),
            ))
            .await
    }

    pub async fn get_policy(&self, organization_id: &Id, id: &Id) -> Result<NotifyPolicy> {
        self.policies.get(organization_id, id).await
    }

    pub async fn list_policies(&self, organization_id: &Id) -> Result<Vec<NotifyPolicy>> {
        self.policies.list(organization_id).await
    }

    pub async fn delete_policy(&self, organization_id: &Id, id: &Id) -> Result<()> {
        self.policies.delete(organization_id, id).await
    }

    pub async fn get_team_default(
        &self,
        organization_id: &Id,
        team_id: &Id,
        category: NotifyCategory,
    ) -> Result<Option<TeamNotifyDefault>> {
        self.ensure_team(organization_id, team_id).await?;
        self.team_defaults
            .get(organization_id, team_id, category)
            .await
    }

    pub async fn list_team_defaults(
        &self,
        organization_id: &Id,
        team_id: &Id,
    ) -> Result<Vec<TeamNotifyDefault>> {
        self.ensure_team(organization_id, team_id).await?;
        self.team_defaults.list(organization_id, team_id).await
    }

    pub async fn upsert_team_default(
        &self,
        organization_id: &Id,
        team_id: &Id,
        category: NotifyCategory,
        input: NotifyDefaultInput,
    ) -> Result<TeamNotifyDefault> {
        self.ensure_team(organization_id, team_id).await?;
        let routes = self
            .validate_default_routes(organization_id, input.routes)
            .await?;
        let existing = self
            .team_defaults
            .get(organization_id, team_id, category)
            .await?;
        let now = now();
        let (id, created_at) = existing
            .map(|value| (value.id, value.created_at))
            .unwrap_or_else(|| (Id::new(), now));
        self.team_defaults
            .upsert(TeamNotifyDefault {
                id,
                organization_id: organization_id.clone(),
                team_id: team_id.clone(),
                category,
                routes,
                enabled: input.enabled,
                created_at,
                updated_at: now,
            })
            .await
    }

    pub async fn delete_team_default(
        &self,
        organization_id: &Id,
        team_id: &Id,
        category: NotifyCategory,
    ) -> Result<()> {
        self.ensure_team(organization_id, team_id).await?;
        self.team_defaults
            .delete(organization_id, team_id, category)
            .await
    }

    pub async fn get_organization_default(
        &self,
        organization_id: &Id,
        category: NotifyCategory,
    ) -> Result<Option<OrganizationNotifyDefault>> {
        self.organization_defaults
            .get(organization_id, category)
            .await
    }

    pub async fn list_organization_defaults(
        &self,
        organization_id: &Id,
    ) -> Result<Vec<OrganizationNotifyDefault>> {
        self.organization_defaults.list(organization_id).await
    }

    pub async fn upsert_organization_default(
        &self,
        organization_id: &Id,
        category: NotifyCategory,
        input: NotifyDefaultInput,
    ) -> Result<OrganizationNotifyDefault> {
        let routes = self
            .validate_default_routes(organization_id, input.routes)
            .await?;
        let existing = self
            .organization_defaults
            .get(organization_id, category)
            .await?;
        let now = now();
        let (id, created_at) = existing
            .map(|value| (value.id, value.created_at))
            .unwrap_or_else(|| (Id::new(), now));
        self.organization_defaults
            .upsert(OrganizationNotifyDefault {
                id,
                organization_id: organization_id.clone(),
                category,
                routes,
                enabled: input.enabled,
                created_at,
                updated_at: now,
            })
            .await
    }

    pub async fn delete_organization_default(
        &self,
        organization_id: &Id,
        category: NotifyCategory,
    ) -> Result<()> {
        self.organization_defaults
            .delete(organization_id, category)
            .await
    }

    async fn validate_policy_input(
        &self,
        organization_id: &Id,
        mut input: NotifyPolicyInput,
    ) -> Result<NotifyPolicyInput> {
        validate_name(&input.name)?;
        input.name = input.name.trim().to_string();
        input.event_type = input.event_type.trim().to_string();
        if input.event_type.is_empty()
            || input.event_type.len() > 128
            || !input.event_type.chars().all(|value| {
                value.is_ascii_lowercase()
                    || value.is_ascii_digit()
                    || matches!(value, '.' | '_' | '-')
            })
        {
            return Err(Error::invalid(
                "notify policy event_type must use lowercase letters, digits, dot, underscore, or dash",
            ));
        }
        validate_matchers(&input.matchers)?;
        if !input.resolver_config.is_object() {
            return Err(Error::invalid(
                "notify policy resolver_config must be a JSON object",
            ));
        }
        input.recipient_resolver = input.recipient_resolver.trim().to_string();
        self.resolver_registry
            .get(&input.recipient_resolver)?
            .validate_config(&input.resolver_config)?;
        self.validate_delivery_selection(
            organization_id,
            input.delivery_mode,
            &input.delivery_config,
        )
        .await?;
        if let Some(template_id) = &input.template_id {
            let template = self.templates.get(organization_id, template_id).await?;
            super::template::validate_template_for_notify(&template)?;
        }
        self.validate_escalation_input(organization_id, &input)
            .await?;
        if input
            .ack_timeout_seconds
            .is_some_and(|seconds| seconds <= 0)
        {
            return Err(Error::invalid(
                "notify policy ack_timeout_seconds must be positive",
            ));
        }
        if !(0..=10_000).contains(&input.priority) {
            return Err(Error::invalid(
                "notify policy priority must be between 0 and 10000",
            ));
        }
        if input
            .escalation_config
            .as_ref()
            .is_some_and(|value| !value.is_object())
        {
            return Err(Error::invalid(
                "notify policy escalation_config must be a JSON object",
            ));
        }
        Ok(input)
    }

    pub(super) async fn validate_delivery_selection(
        &self,
        organization_id: &Id,
        delivery_mode: NotifyDeliveryMode,
        delivery_config: &crate::domain::notify::policy::NotifyDeliveryConfig,
    ) -> Result<()> {
        let connector_ids = &delivery_config.connector_ids;
        if connector_ids.len() > 10 {
            return Err(Error::invalid(
                "notify policy delivery_config supports at most 10 connectors",
            ));
        }
        if connector_ids.iter().collect::<HashSet<_>>().len() != connector_ids.len() {
            return Err(Error::invalid(
                "notify policy delivery_config connector_ids must be unique",
            ));
        }
        match delivery_mode {
            NotifyDeliveryMode::PreferUser if !connector_ids.is_empty() => {
                return Err(Error::invalid(
                    "prefer_user delivery mode cannot configure connector_ids",
                ));
            }
            NotifyDeliveryMode::ForceConnector if connector_ids.len() != 1 => {
                return Err(Error::invalid(
                    "force_connector delivery mode requires exactly one connector_id",
                ));
            }
            NotifyDeliveryMode::MultiConnector if connector_ids.len() < 2 => {
                return Err(Error::invalid(
                    "multi_connector delivery mode requires at least two connector_ids",
                ));
            }
            _ => {}
        }
        for connector_id in connector_ids {
            let connector = self.connectors.get(organization_id, connector_id).await?;
            let capabilities = self
                .connector_registry
                .capabilities(&connector.connector_type)?;
            if !capabilities.direct_user {
                return Err(Error::invalid(format!(
                    "notify connector {} cannot deliver to user endpoints",
                    connector.id
                )));
            }
        }
        Ok(())
    }

    async fn ensure_team(&self, organization_id: &Id, team_id: &Id) -> Result<()> {
        if self
            .teams
            .list(organization_id)
            .await?
            .into_iter()
            .any(|team| team.id == *team_id)
        {
            Ok(())
        } else {
            Err(Error::not_found("team"))
        }
    }

    async fn validate_default_routes(
        &self,
        organization_id: &Id,
        mut routes: Vec<crate::domain::notify::routing::NotifyDefaultRoute>,
    ) -> Result<Vec<crate::domain::notify::routing::NotifyDefaultRoute>> {
        if routes.is_empty() {
            return Err(Error::invalid("notify default requires at least one route"));
        }
        if routes.len() > 50 {
            return Err(Error::invalid("notify default supports at most 50 routes"));
        }
        routes.sort_by_key(|route| route.order);
        let mut connectors_and_targets = HashSet::new();
        for (index, route) in routes.iter_mut().enumerate() {
            let expected_order = i32::try_from(index + 1).unwrap_or(i32::MAX);
            if route.order != expected_order {
                return Err(Error::invalid(
                    "notify default route order must be contiguous and start at 1",
                ));
            }
            if matches!(
                route.target_type,
                NotifyTargetType::DirectUser | NotifyTargetType::Webhook
            ) {
                return Err(Error::invalid(
                    "notify default routes require fixed_address or fixed_group targets",
                ));
            }
            route.target = route.target.trim().to_string();
            let key = (
                route.connector_id.0.clone(),
                route.target_type,
                route.target.clone(),
            );
            if !connectors_and_targets.insert(key) {
                return Err(Error::invalid("notify default routes must be unique"));
            }
            let connector = self
                .connectors
                .get(organization_id, &route.connector_id)
                .await?;
            self.connector_registry
                .get(&connector.connector_type)?
                .validate_target(&NotifyTarget {
                    target_type: route.target_type,
                    value: route.target.clone(),
                    metadata: Default::default(),
                })?;
        }
        Ok(routes)
    }
}

fn policy_from_input(
    id: Id,
    organization_id: Id,
    input: NotifyPolicyInput,
    created_at: crate::shared::time::TimestampMicros,
    updated_at: crate::shared::time::TimestampMicros,
) -> NotifyPolicy {
    NotifyPolicy {
        id,
        organization_id,
        name: input.name,
        event_type: input.event_type,
        category: input.category,
        matchers: input.matchers,
        recipient_resolver: input.recipient_resolver,
        resolver_config: input.resolver_config,
        delivery_mode: input.delivery_mode,
        delivery_config: input.delivery_config,
        template_id: input.template_id,
        fallback_config: input.fallback_config,
        ack_timeout_seconds: input.ack_timeout_seconds,
        escalation_config: input.escalation_config,
        enabled: input.enabled,
        priority: input.priority,
        created_at,
        updated_at,
    }
}
