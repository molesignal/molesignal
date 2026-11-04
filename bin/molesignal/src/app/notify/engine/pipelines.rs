// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 通知前处理：策略发送一条告警通知之前，经过为这条策略启用的告警流水线
//! （[`crate::app::alerting::event_pipelines::notify`]）。改动落在事件和消息的副本上，只影响这条策略
//! 发出的通知，接收人照原来的算；丢弃就是这条策略不发，投递记录里写原因。

use std::sync::Arc;

use serde_json::Value;

use super::{NotifyEngine, context::complete_importance};
use crate::{
    app::{
        alerting::event_pipelines::{
            AlertPipelines,
            notify::{BeforeNotify, NotifyFacts},
        },
        notify::{config::truncate_error, events::retold_by_pipeline},
    },
    domain::{
        alerting::{incident::Severity, priority::Priority, urgency::Urgency},
        notify::{
            connector::NotifyMessage,
            delivery::{DeliveryCompletion, DeliveryStage, DeliveryStatus, NotifyDelivery},
            policy::{NotifyEvent, NotifyPolicy},
        },
        pipeline::alert::AlertFields,
    },
    shared::{ids::Id, time::TimestampMicros},
};

/// A delivery record's `error_code` when a pipeline kept a policy from sending.
const DROPPED_BY_PIPELINE: &str = "dropped_by_pipeline";

/// What a policy sends about an event, once its pipelines have seen it.
pub(super) enum PolicyCopy {
    /// No pipeline changed anything: the event and the message as they are.
    AsIs,
    Changed(Box<(NotifyEvent, NotifyMessage)>),
    /// A step dropped it, and why.
    Dropped(String),
}

impl NotifyEngine {
    /// 策略发送告警通知之前经过这些告警流水线。
    pub fn with_alert_pipelines(mut self, pipelines: Arc<AlertPipelines>) -> Self {
        self.alert_pipelines = Some(pipelines);
        self
    }

    /// What `policy` sends about this event. Only alert notifications go through pipelines.
    pub(super) async fn policy_copy(
        &self,
        policy: &NotifyPolicy,
        event: &NotifyEvent,
        message: &NotifyMessage,
    ) -> PolicyCopy {
        let (Some(service), Some(event_name)) = (
            &self.alert_pipelines,
            event.event_type.strip_prefix("alert."),
        ) else {
            return PolicyCopy::AsIs;
        };

        let pipelines = service.for_policy(&event.organization_id, &policy.id).await;

        if pipelines.is_empty() {
            return PolicyCopy::AsIs;
        }

        let Some(before) = alert_fields_of(&event.attributes) else {
            return PolicyCopy::AsIs;
        };

        let rule = self.event_rule(event).await;

        let rule_id = event
            .attributes
            .get("rule_id")
            .and_then(Value::as_str)
            .unwrap_or_default();

        let facts = NotifyFacts {
            policy_id: &policy.id,
            policy_name: &policy.name,
            rule_id,
            // An event that no rule opened goes by its title.
            rule_name: rule
                .as_ref()
                .map_or(before.summary.as_str(), |rule| rule.name.as_str()),
            event_name,
            at: event.occurred_at,
        };

        let after = match service
            .before_notify(&pipelines, before.clone(), facts)
            .await
        {
            BeforeNotify::Kept(after) => *after,
            BeforeNotify::Dropped(reason) => return PolicyCopy::Dropped(reason),
        };

        if after == before {
            return PolicyCopy::AsIs;
        }

        let mut event = event.clone();
        let mut message = message.clone();

        retold_by_pipeline(&mut event, &mut message, &before, &after);

        // The names of the levels follow them.
        if before.urgency != after.urgency || before.priority != after.priority {
            if let Some(attributes) = event.attributes.as_object_mut() {
                attributes.remove("urgency_name");
                attributes.remove("priority_name");
            }

            let levels = crate::app::alerting::importance::levels_of(
                self.importance.as_ref(),
                &event.organization_id,
            )
            .await;

            complete_importance(&mut event.attributes, &levels);
        }

        PolicyCopy::Changed(Box::new((event, message)))
    }

    /// Records that this policy did not send: a skipped delivery with the reason, once per event and
    /// policy. It names neither a person nor a connector, since nobody was tried.
    pub(super) async fn record_dropped(
        &self,
        policy: &NotifyPolicy,
        event: &NotifyEvent,
        stage: Option<DeliveryStage>,
        reason: &str,
    ) {
        let now = TimestampMicros::now();
        let organization_id = &event.organization_id;

        let claim = self
            .deliveries
            .claim(NotifyDelivery {
                id: Id::new(),
                organization_id: organization_id.clone(),
                event_id: event.id.clone(),
                policy_id: Some(policy.id.clone()),
                recipient_user_id: None,
                connector_id: None,
                endpoint_id: None,
                target_type: "policy".into(),
                target_value_masked: None,
                stage: stage.unwrap_or(DeliveryStage::UserPrimary),
                attempt: 1,
                status: DeliveryStatus::Sending,
                error_code: None,
                error_message: None,
                latency_ms: None,
                sent_at: Some(now),
                delivered_at: None,
                acknowledged_at: None,
                escalated_at: None,
                idempotency_key: dropped_key(policy, event),
                created_at: now,
            })
            .await;

        let completed = match claim {
            Ok(claim) if claim.acquired => {
                self.deliveries
                    .complete(
                        organization_id,
                        &claim.delivery.id,
                        DeliveryCompletion {
                            status: DeliveryStatus::Skipped,
                            error_code: Some(DROPPED_BY_PIPELINE.into()),
                            error_message: Some(truncate_error(reason)),
                            latency_ms: None,
                            delivered_at: None,
                        },
                    )
                    .await
            }
            Ok(_) => return,
            Err(error) => Err(error),
        };

        if let Err(error) = completed {
            tracing::warn!(policy_id = %policy.id, event_id = %event.id, %error, "a dropped notification was not recorded");
        }
    }
}

/// One record per event and policy: a retry of the event finds it there.
fn dropped_key(policy: &NotifyPolicy, event: &NotifyEvent) -> String {
    let mut hasher = blake3::Hasher::new();

    for part in [
        event.organization_id.as_str(),
        &event.id,
        policy.id.as_str(),
        DROPPED_BY_PIPELINE,
    ] {
        hasher.update(&(part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }

    format!("notify:{}", hasher.finalize().to_hex())
}

/// The fields of the alert a notification is about, as its event holds them. `None` when it holds no
/// title or severity it could be read by.
fn alert_fields_of(attributes: &Value) -> Option<AlertFields> {
    let text = |key: &str| attributes.get(key).and_then(Value::as_str);

    let texts = |key: &str| {
        attributes
            .get(key)
            .and_then(Value::as_object)
            .map(|fields| {
                fields
                    .iter()
                    .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned())))
                    .collect()
            })
            .unwrap_or_default()
    };

    Some(AlertFields {
        summary: text("summary")?.to_owned(),
        severity: Severity::from_label(text("severity")?)?,
        priority: text("priority").and_then(Priority::from_label),
        urgency: text("urgency").and_then(Urgency::from_label),
        labels: texts("labels"),
        annotations: texts("annotations"),
        affected_services: attributes
            .get("affected_services")
            .and_then(Value::as_array)
            .map(|names| {
                names
                    .iter()
                    .filter_map(|name| name.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
    })
}
