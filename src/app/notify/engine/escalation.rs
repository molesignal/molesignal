// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::Value;

use super::{NotifyEngine, model::NotifyPolicyInput};
use crate::{
    domain::notify::{
        delivery::{DeliveryStage, NotifyDelivery},
        policy::{NotifyDeliveryConfig, NotifyDeliveryMode, NotifyFallbackConfig, NotifyPolicy},
        preference::NotifyCategory,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Deserialize)]
struct EscalationConfig {
    recipient_resolver: String,
    #[serde(default)]
    resolver_config: Option<Value>,
    #[serde(default)]
    delivery_mode: NotifyDeliveryMode,
    #[serde(default)]
    delivery_config: NotifyDeliveryConfig,
    #[serde(default)]
    fallback_config: Option<NotifyFallbackConfig>,
}

impl NotifyEngine {
    pub async fn acknowledge_event(
        &self,
        organization_id: &Id,
        event_id: &str,
        acknowledged_at: TimestampMicros,
    ) -> Result<u64> {
        self.deliveries
            .acknowledge_event(organization_id, event_id, acknowledged_at)
            .await
    }

    pub async fn acknowledge_delivery(
        &self,
        organization_id: &Id,
        delivery_id: &Id,
        acknowledged_at: TimestampMicros,
    ) -> Result<NotifyDelivery> {
        let delivery = self.deliveries.get(organization_id, delivery_id).await?;
        if delivery.policy_id.is_none() {
            return Err(Error::invalid(
                "notify test deliveries cannot be acknowledged",
            ));
        }
        self.acknowledge_event(organization_id, &delivery.event_id, acknowledged_at)
            .await?;
        self.deliveries.get(organization_id, delivery_id).await
    }

    pub async fn process_due_escalations(
        &self,
        organization_id: &Id,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<u32> {
        let due = self
            .deliveries
            .list_due_ack(organization_id, now, limit)
            .await?;
        let count = u32::try_from(due.len()).unwrap_or(u32::MAX);
        for delivery in due {
            if let Err(error) = self.escalate_delivery(&delivery, now).await {
                tracing::warn!(
                    org_id = %organization_id,
                    delivery_id = %delivery.id,
                    error = %error,
                    "notify acknowledgement escalation failed"
                );
            }
        }
        Ok(count)
    }

    pub(super) async fn validate_escalation_input(
        &self,
        organization_id: &Id,
        input: &NotifyPolicyInput,
    ) -> Result<()> {
        match (&input.ack_timeout_seconds, &input.escalation_config) {
            (None, None) => return Ok(()),
            (Some(_), None) | (None, Some(_)) => {
                return Err(Error::invalid(
                    "notify policy acknowledgement timeout and escalation config must be configured together",
                ));
            }
            (Some(_), Some(_)) => {}
        }
        let config = parse_escalation(input.escalation_config.as_ref())?;
        let resolver_config = config
            .resolver_config
            .as_ref()
            .unwrap_or(&input.resolver_config);
        self.resolver_registry
            .get(config.recipient_resolver.trim())?
            .validate_config(resolver_config)?;
        self.validate_delivery_selection(
            organization_id,
            config.delivery_mode,
            &config.delivery_config,
        )
        .await
    }

    async fn escalate_delivery(
        &self,
        delivery: &NotifyDelivery,
        now: TimestampMicros,
    ) -> Result<()> {
        let Some(policy_id) = delivery.policy_id.as_ref() else {
            self.deliveries
                .mark_escalated(&delivery.organization_id, &delivery.id, now)
                .await?;
            return Ok(());
        };
        let policy = self
            .policies
            .get(&delivery.organization_id, policy_id)
            .await?;
        let config = parse_escalation(policy.escalation_config.as_ref())?;
        let event_record = match self
            .events
            .get(&delivery.organization_id, &delivery.event_id)
            .await
        {
            Ok(record) => record,
            Err(error) => {
                self.deliveries
                    .mark_escalated(&delivery.organization_id, &delivery.id, now)
                    .await?;
                return Err(error);
            }
        };
        let escalation_policy = escalation_policy(&policy, config);
        let outcome = self
            .execute_policy(
                &escalation_policy,
                &event_record.event,
                &event_record.message,
                Some(DeliveryStage::Escalation),
            )
            .await;
        if let Some(error) = outcome.error {
            return Err(Error::unavailable(error));
        }
        if outcome.recipients.is_empty()
            || outcome
                .recipients
                .iter()
                .any(|recipient| !recipient.delivered)
        {
            return Err(Error::unavailable(
                "notify escalation did not reach every resolved recipient",
            ));
        }
        self.deliveries
            .mark_escalated(&delivery.organization_id, &delivery.id, now)
            .await?;
        Ok(())
    }
}

fn parse_escalation(value: Option<&Value>) -> Result<EscalationConfig> {
    let value = value.ok_or_else(|| Error::invalid("notify escalation config is missing"))?;
    serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("invalid notify escalation config: {error}")))
}

fn escalation_policy(policy: &NotifyPolicy, config: EscalationConfig) -> NotifyPolicy {
    let resolver_config = config
        .resolver_config
        .unwrap_or_else(|| policy.resolver_config.clone());
    NotifyPolicy {
        id: policy.id.clone(),
        organization_id: policy.organization_id.clone(),
        name: format!("{} / escalation", policy.name),
        event_type: policy.event_type.clone(),
        category: NotifyCategory::Escalation,
        matchers: Value::Object(Default::default()),
        recipient_resolver: config.recipient_resolver,
        resolver_config,
        delivery_mode: config.delivery_mode,
        delivery_config: config.delivery_config,
        template_id: policy.template_id.clone(),
        fallback_config: config
            .fallback_config
            .unwrap_or_else(|| policy.fallback_config.clone()),
        ack_timeout_seconds: None,
        escalation_config: None,
        enabled: true,
        priority: policy.priority,
        created_at: policy.created_at,
        updated_at: policy.updated_at,
    }
}
