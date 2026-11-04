// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::time::Instant;

use super::{
    NotifyEngine,
    calls::merge_window,
    follow_ups::{is_user_step, step_delays},
    model::{
        NotifyDeliveryPlanStep, NotifyDispatch, NotifyEventOutcome, NotifyPolicyOutcome,
        NotifyPolicyPreview, NotifyRecipientOutcome, NotifyRecipientPlan, ResolvedRoute,
    },
    pipelines::PolicyCopy,
    template::{PolicyMessages, policy_template_id},
    webhook::webhook_event,
};
use crate::{
    app::notify::{
        config::{mask_target, truncate_error},
        policy_matches,
    },
    domain::notify::{
        connector::{NotifyChannel, NotifyTargetType},
        delivery::{DeliveryCompletion, DeliveryStatus, NotifyDelivery},
        policy::{NotifyEvent, NotifyPolicy},
        recipient::NotifyRecipient,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

impl NotifyEngine {
    pub(super) async fn execute_dispatch(
        &self,
        dispatch: NotifyDispatch,
    ) -> Result<NotifyEventOutcome> {
        validate_event(&dispatch.event)?;

        let policies = self
            .policies
            .list_enabled_for_event(&dispatch.event.organization_id, &dispatch.event.event_type)
            .await?;

        let mut outcomes = Vec::new();

        for policy in policies {
            match policy_matches(&policy, &dispatch.event) {
                Ok(true) => {
                    outcomes.push(
                        self.execute_policy(&policy, &dispatch.event, &dispatch.message, None)
                            .await,
                    );
                }
                Ok(false) => {}
                Err(error) => outcomes.push(NotifyPolicyOutcome {
                    policy_id: policy.id,
                    recipients: Vec::new(),
                    error: Some(truncate_error(&error.to_string())),
                    dropped_by_pipeline: false,
                }),
            }
        }

        Ok(NotifyEventOutcome {
            event_id: dispatch.event.id,
            policies: outcomes,
        })
    }

    pub(super) async fn execute_policy(
        &self,
        policy: &NotifyPolicy,
        event: &NotifyEvent,
        message: &crate::domain::notify::connector::NotifyMessage,
        stage_override: Option<crate::domain::notify::delivery::DeliveryStage>,
    ) -> NotifyPolicyOutcome {
        // 模板和收件人都从流水线留下的副本出发。
        let copy = match self.policy_copy(policy, event, message).await {
            PolicyCopy::AsIs => None,
            PolicyCopy::Changed(copy) => Some(copy),
            PolicyCopy::Dropped(reason) => {
                self.record_dropped(policy, event, stage_override, &reason)
                    .await;

                return NotifyPolicyOutcome {
                    policy_id: policy.id.clone(),
                    recipients: Vec::new(),
                    error: None,
                    dropped_by_pipeline: true,
                };
            }
        };

        let (event, message) = copy
            .as_deref()
            .map_or((event, message), |(event, message)| (event, message));

        let messages = match self.policy_messages(policy, event, message).await {
            Ok(messages) => messages,
            Err(error) => {
                return NotifyPolicyOutcome {
                    policy_id: policy.id.clone(),
                    recipients: Vec::new(),
                    error: Some(truncate_error(&error.to_string())),
                    dropped_by_pipeline: false,
                };
            }
        };

        let recipients = match self.resolve_recipients(policy, event).await {
            Ok(recipients) => recipients,
            Err(error) => {
                return NotifyPolicyOutcome {
                    policy_id: policy.id.clone(),
                    recipients: Vec::new(),
                    error: Some(truncate_error(&error.to_string())),
                    dropped_by_pipeline: false,
                };
            }
        };

        let mut outcomes = Vec::with_capacity(recipients.len());

        for recipient in recipients {
            outcomes.push(
                self.deliver_recipient(policy, event, recipient, &messages, stage_override)
                    .await,
            );
        }

        NotifyPolicyOutcome {
            policy_id: policy.id.clone(),
            recipients: outcomes,
            error: None,
            dropped_by_pipeline: false,
        }
    }

    pub async fn preview_policy(
        &self,
        organization_id: &Id,
        policy_id: &Id,
        event: NotifyEvent,
    ) -> Result<NotifyPolicyPreview> {
        if event.organization_id != *organization_id {
            return Err(Error::forbidden(
                "notify preview event organization does not match tenant context",
            ));
        }

        validate_event(&event)?;
        let policy = self.policies.get(organization_id, policy_id).await?;
        self.preview_policy_record(policy, event).await
    }

    pub(super) async fn preview_policy_record(
        &self,
        policy: NotifyPolicy,
        event: NotifyEvent,
    ) -> Result<NotifyPolicyPreview> {
        let matched = policy_matches(&policy, &event)?;

        if !matched {
            return Ok(NotifyPolicyPreview {
                policy_id: policy.id,
                matched: false,
                recipients: Vec::new(),
            });
        }

        let recipients = self.resolve_recipients(&policy, &event).await?;
        let mut plans = Vec::with_capacity(recipients.len());

        for recipient in recipients {
            let routes = self.resolve_routes(&policy, &event, &recipient).await?;

            plans.push(NotifyRecipientPlan {
                user_id: recipient.user_id,
                team_id: recipient.team_id,
                resolved_by: policy.recipient_resolver.clone(),
                delivery_plan: routes
                    .into_iter()
                    .map(|route| {
                        let template_id = self
                            .connector_registry
                            .get(&route.connector.connector_type)
                            .ok()
                            .and_then(|adapter| policy_template_id(&policy, adapter.channel()))
                            .cloned();

                        NotifyDeliveryPlanStep {
                            stage: route.stage,
                            connector_id: route.connector.id,
                            connector_name: route.connector.name,
                            endpoint_id: route.endpoint_id,
                            target_type: route.target.target_type,
                            target_value_masked: mask_target(&route.target.value),
                            template_id,
                        }
                    })
                    .collect(),
            });
        }

        Ok(NotifyPolicyPreview {
            policy_id: policy.id,
            matched: true,
            recipients: plans,
        })
    }

    async fn resolve_recipients(
        &self,
        policy: &NotifyPolicy,
        event: &NotifyEvent,
    ) -> Result<Vec<NotifyRecipient>> {
        self.resolver_registry
            .get(&policy.recipient_resolver)?
            .resolve(event, &policy.resolver_config)
            .await
    }

    async fn deliver_recipient(
        &self,
        policy: &NotifyPolicy,
        event: &NotifyEvent,
        recipient: NotifyRecipient,
        messages: &PolicyMessages,
        stage_override: Option<crate::domain::notify::delivery::DeliveryStage>,
    ) -> NotifyRecipientOutcome {
        let routes = match self.resolve_routes(policy, event, &recipient).await {
            Ok(routes) => routes,
            Err(error) => {
                return NotifyRecipientOutcome {
                    user_id: recipient.user_id,
                    team_id: recipient.team_id,
                    delivered: false,
                    attempts: Vec::new(),
                    error: Some(truncate_error(&error.to_string())),
                };
            }
        };

        let mut attempts = Vec::new();
        let multi_connector = policy.delivery_mode
            == crate::domain::notify::policy::NotifyDeliveryMode::MultiConnector;
        let mut user_route_succeeded = false;
        let delays = step_delays(policy, event, stage_override.is_some(), &routes);
        let started_at = TimestampMicros::now();

        for mut route in routes {
            let primary_route =
                route.stage == crate::domain::notify::delivery::DeliveryStage::UserPrimary;
            let user_step = is_user_step(&route);
            let delay_secs = route.delay_secs;

            if multi_connector && user_route_succeeded && !primary_route {
                break;
            }

            if let Some(stage) = stage_override {
                route.stage = stage;
            }

            match self
                .attempt_route(policy, event, &recipient, route, messages)
                .await
            {
                Ok((delivered, delivery)) => {
                    attempts.push(delivery);

                    if delivered {
                        if multi_connector && primary_route {
                            user_route_succeeded = true;
                            continue;
                        }

                        // 送到了这个人：接收顺序里还有等得更久的步骤时，排好下一步。
                        if user_step {
                            self.schedule_follow_up(
                                policy, event, &recipient, &delays, delay_secs, started_at,
                            )
                            .await;
                        }

                        return NotifyRecipientOutcome {
                            user_id: recipient.user_id,
                            team_id: recipient.team_id,
                            delivered: true,
                            attempts,
                            error: None,
                        };
                    }
                }
                Err(error) => {
                    return NotifyRecipientOutcome {
                        user_id: recipient.user_id,
                        team_id: recipient.team_id,
                        delivered: false,
                        attempts,
                        error: Some(truncate_error(&error.to_string())),
                    };
                }
            }
        }

        NotifyRecipientOutcome {
            user_id: recipient.user_id,
            team_id: recipient.team_id,
            delivered: user_route_succeeded,
            attempts,
            error: (!user_route_succeeded).then(|| "no notify route succeeded".into()),
        }
    }

    pub(super) async fn attempt_route(
        &self,
        policy: &NotifyPolicy,
        event: &NotifyEvent,
        recipient: &NotifyRecipient,
        route: ResolvedRoute,
        messages: &PolicyMessages,
    ) -> Result<(bool, NotifyDelivery)> {
        let sent_at = TimestampMicros::now();
        let idempotency_key = idempotency_key(policy, event, recipient, &route);

        let claim = self
            .deliveries
            .claim(NotifyDelivery {
                id: Id::new(),
                organization_id: event.organization_id.clone(),
                event_id: event.id.clone(),
                policy_id: Some(policy.id.clone()),
                recipient_user_id: Some(recipient.user_id.clone()),
                connector_id: Some(route.connector.id.clone()),
                endpoint_id: route.endpoint_id.clone(),
                target_type: route.target.target_type.as_str().into(),
                target_value_masked: Some(mask_target(&route.target.value)),
                stage: route.stage,
                attempt: 1,
                status: DeliveryStatus::Sending,
                error_code: None,
                error_message: None,
                latency_ms: None,
                sent_at: Some(sent_at),
                delivered_at: None,
                acknowledged_at: None,
                escalated_at: None,
                idempotency_key,
                created_at: sent_at,
            })
            .await?;

        if !claim.acquired {
            let delivered = matches!(
                claim.delivery.status,
                DeliveryStatus::Sending
                    | DeliveryStatus::Success
                    | DeliveryStatus::Acknowledged
                    | DeliveryStatus::Queued
            );

            return Ok((delivered, claim.delivery));
        }

        let adapter = self
            .connector_registry
            .get(&route.connector.connector_type)?;

        let started = Instant::now();

        // 按连接器的渠道套用策略为它选的模板。
        let message = adapter
            .validate_config(&route.connector.config)
            .and_then(|_| adapter.validate_target(&route.target))
            .and_then(|_| messages.for_channel(adapter.channel(), event));

        // 这条线路（号码、一个人的飞书、一个群）在合并窗口里发过：排在线路上，窗口结束时合成一条。
        if let Ok(message) = &message
            && let Some(window_secs) = merge_window(adapter.as_ref(), policy, event)
            && let Some(queued) = self
                .queue_if_line_busy(
                    &route.connector,
                    &route.target,
                    window_secs,
                    message,
                    &claim.delivery,
                )
                .await
        {
            return Ok((true, queued));
        }

        let result = match message {
            Ok(message) => {
                let message = self
                    .with_alert_buttons(policy, event, recipient, &route, &message)
                    .await;

                // Webhook 的请求体带上写给机器读的事件，请求头带上这次投递的 ID。
                if adapter.channel() == NotifyChannel::Webhook {
                    let mut message = message.into_owned();
                    message.event = Some(webhook_event(event));

                    message
                        .metadata
                        .insert("delivery_id".into(), claim.delivery.id.to_string());

                    adapter
                        .send_as(&route.connector, &route.target, &message)
                        .await
                } else if matches!(adapter.channel(), NotifyChannel::Sms | NotifyChannel::Phone) {
                    // 收得到回复的短信、电话：记一条回复引用，带上代码与提示。
                    let direct = route.target.target_type == NotifyTargetType::DirectUser;

                    let message = self
                        .with_reply(
                            &route.connector,
                            adapter.as_ref(),
                            &route.target,
                            direct.then_some(&recipient.user_id),
                            replyable_alerts(event),
                            message.into_owned(),
                        )
                        .await;

                    adapter
                        .send_as(&route.connector, &route.target, &message)
                        .await
                } else {
                    adapter
                        .send_as(&route.connector, &route.target, &message)
                        .await
                }
            }
            Err(error) => Err(error),
        };

        let finished_at = TimestampMicros::now();

        let (delivered, completion) = match result {
            Ok(result) if result.delivered => (
                true,
                DeliveryCompletion {
                    status: DeliveryStatus::Success,
                    error_code: None,
                    error_message: None,
                    latency_ms: Some(to_i32_millis(result.latency_ms)),
                    delivered_at: Some(finished_at),
                },
            ),
            Ok(result) => (
                false,
                DeliveryCompletion {
                    status: DeliveryStatus::Failed,
                    error_code: Some("provider_not_delivered".into()),
                    error_message: Some("notify provider did not confirm delivery".into()),
                    latency_ms: Some(to_i32_millis(result.latency_ms)),
                    delivered_at: None,
                },
            ),
            Err(error) => (
                false,
                DeliveryCompletion {
                    status: DeliveryStatus::Failed,
                    error_code: Some("connector_send_failed".into()),
                    error_message: Some(truncate_error(&error.to_string())),
                    latency_ms: Some(to_i32_millis(
                        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                    )),
                    delivered_at: None,
                },
            ),
        };

        let delivery = self
            .deliveries
            .complete(&event.organization_id, &claim.delivery.id, completion)
            .await?;

        Ok((delivered, delivery))
    }
}

pub(super) fn validate_event(event: &NotifyEvent) -> Result<()> {
    if event.id.trim().is_empty() || event.id.len() > 255 {
        return Err(Error::invalid(
            "notify event id must contain between 1 and 255 bytes",
        ));
    }

    if event.event_type.trim().is_empty() || event.event_type.len() > 128 {
        return Err(Error::invalid(
            "notify event type must contain between 1 and 128 bytes",
        ));
    }

    if !event.attributes.is_object() {
        return Err(Error::invalid("notify event attributes must be an object"));
    }

    Ok(())
}

fn idempotency_key(
    policy: &NotifyPolicy,
    event: &NotifyEvent,
    recipient: &NotifyRecipient,
    route: &ResolvedRoute,
) -> String {
    let mut hasher = blake3::Hasher::new();

    let recipient_scope = if route.target.target_type == NotifyTargetType::DirectUser {
        recipient.user_id.as_str()
    } else {
        ""
    };

    for part in [
        event.organization_id.as_str(),
        &event.id,
        policy.id.as_str(),
        recipient_scope,
        route.stage.as_str(),
        route.connector.id.as_str(),
        route
            .endpoint_id
            .as_ref()
            .map_or("", crate::shared::ids::Id::as_str),
        route.target.target_type.as_str(),
        &route.target.value,
    ] {
        hasher.update(&(part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }

    format!("notify:{}", hasher.finalize().to_hex())
}

fn to_i32_millis(value: u64) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// 回复能处理的告警：还要人接手的通知（触发、升级）里的那一个。
fn replyable_alerts(event: &NotifyEvent) -> Vec<Id> {
    let wanted = matches!(
        event.event_type.as_str(),
        "alert.triggered" | "alert.escalated"
    );

    event
        .attributes
        .get("alert_id")
        .and_then(serde_json::Value::as_str)
        .filter(|id| wanted && !id.is_empty())
        .map(|id| vec![Id::from_string(id)])
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::notify::{
        connector::{
            ConnectorCapabilities, ConnectorStatus, NotifyConnector, NotifyTarget, NotifyTargetType,
        },
        delivery::DeliveryStage,
        policy::{NotifyDeliveryConfig, NotifyDeliveryMode, NotifyFallbackConfig},
        preference::NotifyCategory,
    };

    #[test]
    fn idempotency_changes_with_target_and_tenant() {
        let org = Id::from_string("org-a");
        let policy = NotifyPolicy {
            id: Id::from_string("policy-a"),
            organization_id: org.clone(),
            name: "policy".into(),
            builtin_key: None,
            event_type: "alert.triggered".into(),
            category: NotifyCategory::Alert,
            matchers: serde_json::json!({}),
            recipient_resolver: "fixed_users".into(),
            resolver_config: serde_json::json!({}),
            delivery_mode: NotifyDeliveryMode::PreferUser,
            delivery_config: NotifyDeliveryConfig::default(),
            template_id: None,
            channel_templates: Default::default(),
            fallback_config: NotifyFallbackConfig::default(),
            ack_timeout_seconds: None,
            escalation_config: None,
            merge_window_secs: None,
            time: None,
            enabled: true,
            priority: 100,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        };
        let connector = NotifyConnector {
            id: Id::from_string("connector-a"),
            organization_id: org.clone(),
            name: "mail".into(),
            connector_type: "fake".into(),
            config: serde_json::json!({}),
            capabilities: ConnectorCapabilities::default(),
            enabled: true,
            status: ConnectorStatus::Connected,
            last_tested_at: None,
            last_test_status: None,
            last_test_error: None,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        };
        let event = NotifyEvent {
            id: "event-a".into(),
            event_type: "alert.triggered".into(),
            organization_id: org,
            occurred_at: TimestampMicros(1),
            attributes: serde_json::json!({}),
        };
        let recipient = NotifyRecipient {
            user_id: Id::from_string("user-a"),
            team_id: None,
        };
        let route = |value: &str| ResolvedRoute {
            stage: DeliveryStage::OrganizationFallback,
            connector: connector.clone(),
            endpoint_id: None,
            target: NotifyTarget {
                target_type: NotifyTargetType::FixedAddress,
                value: value.into(),
                metadata: Default::default(),
            },
            delay_secs: 0,
        };
        assert_ne!(
            idempotency_key(&policy, &event, &recipient, &route("a@example.com")),
            idempotency_key(&policy, &event, &recipient, &route("b@example.com"))
        );
        let another_recipient = NotifyRecipient {
            user_id: Id::from_string("user-b"),
            team_id: None,
        };
        assert_eq!(
            idempotency_key(&policy, &event, &recipient, &route("a@example.com")),
            idempotency_key(&policy, &event, &another_recipient, &route("a@example.com"))
        );

        let direct_route = |user_id: &str| ResolvedRoute {
            stage: DeliveryStage::UserPrimary,
            connector: connector.clone(),
            endpoint_id: Some(Id::from_string(format!("endpoint-{user_id}"))),
            target: NotifyTarget {
                target_type: NotifyTargetType::DirectUser,
                value: format!("{user_id}@example.com"),
                metadata: Default::default(),
            },
            delay_secs: 0,
        };
        assert_ne!(
            idempotency_key(&policy, &event, &recipient, &direct_route("user-a")),
            idempotency_key(&policy, &event, &another_recipient, &direct_route("user-b"))
        );
    }
}
