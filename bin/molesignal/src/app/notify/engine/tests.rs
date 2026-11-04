// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use serde_json::Value;

use super::*;
use crate::{
    app::notify::{
        ALERT_RESOLVED_EVENT, ALERT_TRIGGERED_EVENT, AlertButtonSource, ConnectorRegistry,
        RecipientResolverRegistry, alert_dispatch,
    },
    domain::{
        alerting::{
            incident::{Incident, IncidentStatus, Severity},
            mute::{MuteRule, MuteRuleRepository, MuteWindow},
            repositories::IncidentRepository,
            semantic_group::{LabelMatcher, MatchOp},
        },
        iam::{Team, TeamRepository},
        notify::{
            buttons::{AlertAction, AlertButtonLabels, AlertButtons},
            call::{CallLine, QueuedCall},
            connector::{
                ConnectorAdapter, ConnectorCapabilities, ConnectorDeliveryResult, ConnectorStatus,
                ConnectorTestStatus, NotifyChannel, NotifyConnector, NotifyMessage, NotifyTarget,
                NotifyTargetType,
            },
            delivery::{
                DeliveryClaim, DeliveryCompletion, DeliveryFilter, DeliveryStage, DeliveryStatus,
                NotifyDelivery,
            },
            endpoint::UserNotifyEndpoint,
            event::{NotifyEventClaim, NotifyEventRecord, NotifyEventStatus},
            follow_up::NotifyFollowUp,
            policy::{
                NotifyDeliveryConfig, NotifyDeliveryMode, NotifyEvent, NotifyFallbackConfig,
                NotifyPolicy,
            },
            preference::{NotifyCategory, UserNotifyPreference, UserNotifyPreferenceStep},
            recipient::{NotifyRecipient, RecipientResolver},
            repositories::{
                NotifyCallRepository, NotifyConnectorRepository, NotifyDeliveryRepository,
                NotifyEventRepository, NotifyFollowUpRepository, NotifyPolicyRepository,
                NotifyTemplateRepository, OrganizationNotifyDefaultRepository,
                TeamNotifyDefaultRepository, UserNotifyEndpointRepository,
                UserNotifyPreferenceRepository,
            },
            routing::{NotifyDefaultRoute, OrganizationNotifyDefault, TeamNotifyDefault},
            template::NotifyTemplate,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 内存里的通知仓储。`service` 里不经策略直接发送的测试也用它。
#[derive(Default)]
pub(crate) struct MemoryStore {
    connectors: Mutex<HashMap<String, NotifyConnector>>,
    endpoints: Mutex<HashMap<String, UserNotifyEndpoint>>,
    preferences: Mutex<HashMap<String, UserNotifyPreference>>,
    deliveries: Mutex<HashMap<String, NotifyDelivery>>,
    events: Mutex<HashMap<String, NotifyEventRecord>>,
    policies: Mutex<HashMap<String, NotifyPolicy>>,
    team_defaults: Mutex<HashMap<String, TeamNotifyDefault>>,
    organization_defaults: Mutex<HashMap<String, OrganizationNotifyDefault>>,
    teams: Mutex<HashMap<String, Team>>,
    /// 后续步骤与它的状态（pending、processing、done、cancelled）。
    follow_ups: Mutex<Vec<(NotifyFollowUp, &'static str)>>,
    /// 线路与它上次拨打的时刻，按「组织|连接器|线路」。
    call_lines: Mutex<HashMap<String, (CallLine, TimestampMicros)>>,
    call_queue: Mutex<Vec<QueuedCall>>,
    templates: Mutex<HashMap<String, NotifyTemplate>>,
    mute_rules: Mutex<Vec<MuteRule>>,
    incidents: Mutex<HashMap<String, Incident>>,
}

fn line_id(line: &CallLine) -> String {
    format!(
        "{}|{}|{}",
        line.organization_id, line.connector_id, line.key
    )
}

fn same_follow_up(left: &NotifyFollowUp, right: &NotifyFollowUp) -> bool {
    left.organization_id == right.organization_id
        && left.event_id == right.event_id
        && left.policy_id == right.policy_id
        && left.recipient_user_id == right.recipient_user_id
        && left.delay_secs == right.delay_secs
}

#[async_trait]
impl NotifyConnectorRepository for MemoryStore {
    async fn create(&self, connector: NotifyConnector) -> Result<NotifyConnector> {
        self.connectors
            .lock()
            .unwrap()
            .insert(connector.id.0.clone(), connector.clone());

        Ok(connector)
    }

    async fn update(&self, connector: NotifyConnector) -> Result<NotifyConnector> {
        NotifyConnectorRepository::create(self, connector).await
    }

    async fn get(&self, organization_id: &Id, id: &Id) -> Result<NotifyConnector> {
        self.connectors
            .lock()
            .unwrap()
            .get(id.as_str())
            .filter(|connector| connector.organization_id == *organization_id)
            .cloned()
            .ok_or_else(|| Error::not_found("notify connector"))
    }

    async fn list(&self, organization_id: &Id) -> Result<Vec<NotifyConnector>> {
        Ok(self
            .connectors
            .lock()
            .unwrap()
            .values()
            .filter(|connector| connector.organization_id == *organization_id)
            .cloned()
            .collect())
    }

    async fn record_test_result(
        &self,
        organization_id: &Id,
        id: &Id,
        tested_at: TimestampMicros,
        status: ConnectorTestStatus,
        error: Option<String>,
    ) -> Result<NotifyConnector> {
        let mut connectors = self.connectors.lock().unwrap();

        let connector = connectors
            .get_mut(id.as_str())
            .filter(|connector| connector.organization_id == *organization_id)
            .ok_or_else(|| Error::not_found("notify connector"))?;

        connector.last_tested_at = Some(tested_at);
        connector.last_test_status = Some(status);
        connector.last_test_error = error;
        Ok(connector.clone())
    }

    async fn delete(&self, organization_id: &Id, id: &Id) -> Result<()> {
        NotifyConnectorRepository::get(self, organization_id, id).await?;
        self.connectors.lock().unwrap().remove(id.as_str());
        Ok(())
    }
}

#[async_trait]
impl UserNotifyEndpointRepository for MemoryStore {
    async fn create(&self, endpoint: UserNotifyEndpoint) -> Result<UserNotifyEndpoint> {
        self.endpoints
            .lock()
            .unwrap()
            .insert(endpoint.id.0.clone(), endpoint.clone());

        Ok(endpoint)
    }

    async fn update(&self, endpoint: UserNotifyEndpoint) -> Result<UserNotifyEndpoint> {
        UserNotifyEndpointRepository::create(self, endpoint).await
    }

    async fn get(&self, organization_id: &Id, user_id: &Id, id: &Id) -> Result<UserNotifyEndpoint> {
        self.endpoints
            .lock()
            .unwrap()
            .get(id.as_str())
            .filter(|endpoint| {
                endpoint.organization_id == *organization_id && endpoint.user_id == *user_id
            })
            .cloned()
            .ok_or_else(|| Error::not_found("user notify endpoint"))
    }

    async fn list(&self, organization_id: &Id, user_id: &Id) -> Result<Vec<UserNotifyEndpoint>> {
        Ok(self
            .endpoints
            .lock()
            .unwrap()
            .values()
            .filter(|endpoint| {
                endpoint.organization_id == *organization_id && endpoint.user_id == *user_id
            })
            .cloned()
            .collect())
    }

    async fn list_for_organization(&self, organization_id: &Id) -> Result<Vec<UserNotifyEndpoint>> {
        Ok(self
            .endpoints
            .lock()
            .unwrap()
            .values()
            .filter(|endpoint| endpoint.organization_id == *organization_id)
            .cloned()
            .collect())
    }

    async fn count_for_connector(&self, organization_id: &Id, connector_id: &Id) -> Result<u64> {
        Ok(self
            .endpoints
            .lock()
            .unwrap()
            .values()
            .filter(|endpoint| {
                endpoint.organization_id == *organization_id
                    && endpoint.connector_id == *connector_id
            })
            .count() as u64)
    }

    async fn delete(&self, organization_id: &Id, user_id: &Id, id: &Id) -> Result<()> {
        UserNotifyEndpointRepository::get(self, organization_id, user_id, id).await?;
        self.endpoints.lock().unwrap().remove(id.as_str());
        Ok(())
    }
}

#[async_trait]
impl UserNotifyPreferenceRepository for MemoryStore {
    async fn get(
        &self,
        organization_id: &Id,
        user_id: &Id,
        category: NotifyCategory,
    ) -> Result<Option<UserNotifyPreference>> {
        Ok(self
            .preferences
            .lock()
            .unwrap()
            .values()
            .find(|preference| {
                preference.organization_id == *organization_id
                    && preference.user_id == *user_id
                    && preference.category == category
            })
            .cloned())
    }

    async fn list(&self, organization_id: &Id, user_id: &Id) -> Result<Vec<UserNotifyPreference>> {
        Ok(self
            .preferences
            .lock()
            .unwrap()
            .values()
            .filter(|preference| {
                preference.organization_id == *organization_id && preference.user_id == *user_id
            })
            .cloned()
            .collect())
    }

    async fn list_for_organization(
        &self,
        organization_id: &Id,
    ) -> Result<Vec<UserNotifyPreference>> {
        Ok(self
            .preferences
            .lock()
            .unwrap()
            .values()
            .filter(|preference| preference.organization_id == *organization_id)
            .cloned()
            .collect())
    }

    async fn upsert(&self, preference: UserNotifyPreference) -> Result<UserNotifyPreference> {
        self.preferences
            .lock()
            .unwrap()
            .insert(preference.id.0.clone(), preference.clone());

        Ok(preference)
    }
}

#[async_trait]
impl NotifyDeliveryRepository for MemoryStore {
    async fn record_once(&self, delivery: NotifyDelivery) -> Result<NotifyDelivery> {
        let mut deliveries = self.deliveries.lock().unwrap();

        if let Some(existing) = deliveries
            .values()
            .find(|existing| existing.idempotency_key == delivery.idempotency_key)
        {
            return Ok(existing.clone());
        }

        deliveries.insert(delivery.id.0.clone(), delivery.clone());
        Ok(delivery)
    }

    async fn claim(&self, mut delivery: NotifyDelivery) -> Result<DeliveryClaim> {
        let mut deliveries = self.deliveries.lock().unwrap();

        if let Some(existing) = deliveries
            .values_mut()
            .find(|existing| existing.idempotency_key == delivery.idempotency_key)
        {
            if matches!(
                existing.status,
                DeliveryStatus::Pending | DeliveryStatus::Failed | DeliveryStatus::Skipped
            ) {
                existing.status = DeliveryStatus::Sending;
                existing.attempt += 1;
                existing.sent_at = delivery.sent_at;
                existing.error_code = None;
                existing.error_message = None;

                return Ok(DeliveryClaim {
                    delivery: existing.clone(),
                    acquired: true,
                });
            }

            return Ok(DeliveryClaim {
                delivery: existing.clone(),
                acquired: false,
            });
        }

        delivery.status = DeliveryStatus::Sending;
        deliveries.insert(delivery.id.0.clone(), delivery.clone());

        Ok(DeliveryClaim {
            delivery,
            acquired: true,
        })
    }

    async fn complete(
        &self,
        organization_id: &Id,
        id: &Id,
        completion: DeliveryCompletion,
    ) -> Result<NotifyDelivery> {
        let mut deliveries = self.deliveries.lock().unwrap();

        let delivery = deliveries
            .get_mut(id.as_str())
            .filter(|delivery| delivery.organization_id == *organization_id)
            .ok_or_else(|| Error::not_found("notify delivery"))?;

        delivery.status = completion.status;
        delivery.error_code = completion.error_code;
        delivery.error_message = completion.error_message;
        delivery.latency_ms = completion.latency_ms;
        delivery.delivered_at = completion.delivered_at;
        Ok(delivery.clone())
    }

    async fn get(&self, organization_id: &Id, id: &Id) -> Result<NotifyDelivery> {
        self.deliveries
            .lock()
            .unwrap()
            .get(id.as_str())
            .filter(|delivery| delivery.organization_id == *organization_id)
            .cloned()
            .ok_or_else(|| Error::not_found("notify delivery"))
    }

    async fn find_by_idempotency_key(
        &self,
        organization_id: &Id,
        idempotency_key: &str,
    ) -> Result<Option<NotifyDelivery>> {
        Ok(self
            .deliveries
            .lock()
            .unwrap()
            .values()
            .find(|delivery| {
                delivery.organization_id == *organization_id
                    && delivery.idempotency_key == idempotency_key
            })
            .cloned())
    }

    async fn list(
        &self,
        organization_id: &Id,
        _filter: &DeliveryFilter,
    ) -> Result<Vec<NotifyDelivery>> {
        Ok(self
            .deliveries
            .lock()
            .unwrap()
            .values()
            .filter(|delivery| delivery.organization_id == *organization_id)
            .cloned()
            .collect())
    }

    async fn acknowledge_event(
        &self,
        organization_id: &Id,
        event_id: &str,
        acknowledged_at: TimestampMicros,
    ) -> Result<u64> {
        let mut deliveries = self.deliveries.lock().unwrap();
        let mut count = 0;

        for delivery in deliveries.values_mut().filter(|delivery| {
            delivery.organization_id == *organization_id
                && delivery.event_id == event_id
                && matches!(
                    delivery.status,
                    DeliveryStatus::Success | DeliveryStatus::Acknowledged | DeliveryStatus::Queued
                )
        }) {
            delivery.status = DeliveryStatus::Acknowledged;
            delivery.acknowledged_at = Some(acknowledged_at);
            count += 1;
        }

        Ok(count)
    }

    async fn acknowledge_event_prefix(
        &self,
        organization_id: &Id,
        event_id_prefix: &str,
        acknowledged_at: TimestampMicros,
    ) -> Result<u64> {
        let event_ids = self
            .deliveries
            .lock()
            .unwrap()
            .values()
            .filter(|delivery| delivery.event_id.starts_with(event_id_prefix))
            .map(|delivery| delivery.event_id.clone())
            .collect::<std::collections::BTreeSet<_>>();

        let mut count = 0;

        for event_id in event_ids {
            count += NotifyDeliveryRepository::acknowledge_event(
                self,
                organization_id,
                &event_id,
                acknowledged_at,
            )
            .await?;
        }

        Ok(count)
    }

    async fn list_due_ack(
        &self,
        organization_id: &Id,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<NotifyDelivery>> {
        let policies = self.policies.lock().unwrap();

        Ok(self
            .deliveries
            .lock()
            .unwrap()
            .values()
            .filter(|delivery| {
                let Some(policy_id) = &delivery.policy_id else {
                    return false;
                };

                let Some(policy) = policies.get(policy_id.as_str()) else {
                    return false;
                };

                delivery.organization_id == *organization_id
                    && delivery.status == DeliveryStatus::Success
                    && delivery.acknowledged_at.is_none()
                    && delivery.escalated_at.is_none()
                    && delivery.delivered_at.is_some_and(|delivered_at| {
                        policy.ack_timeout_seconds.is_some_and(|seconds| {
                            delivered_at
                                .0
                                .saturating_add(i64::from(seconds) * 1_000_000)
                                <= now.0
                        })
                    })
                    && policy.escalation_config.is_some()
            })
            .take(limit as usize)
            .cloned()
            .collect())
    }

    async fn mark_escalated(
        &self,
        organization_id: &Id,
        id: &Id,
        escalated_at: TimestampMicros,
    ) -> Result<NotifyDelivery> {
        let mut deliveries = self.deliveries.lock().unwrap();

        let delivery = deliveries
            .get_mut(id.as_str())
            .filter(|delivery| delivery.organization_id == *organization_id)
            .ok_or_else(|| Error::not_found("notify delivery"))?;

        delivery.escalated_at = Some(escalated_at);
        Ok(delivery.clone())
    }
}

#[async_trait]
impl NotifyFollowUpRepository for MemoryStore {
    async fn schedule(&self, follow_up: NotifyFollowUp) -> Result<()> {
        let mut follow_ups = self.follow_ups.lock().unwrap();

        if !follow_ups
            .iter()
            .any(|(existing, _)| same_follow_up(existing, &follow_up))
        {
            follow_ups.push((follow_up, "pending"));
        }

        Ok(())
    }

    async fn claim_due(
        &self,
        organization_id: &Id,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<NotifyFollowUp>> {
        let mut claimed = Vec::new();

        for (follow_up, status) in self.follow_ups.lock().unwrap().iter_mut() {
            if follow_up.organization_id == *organization_id
                && *status == "pending"
                && follow_up.due_at.0 <= now.0
                && claimed.len() < limit as usize
            {
                *status = "processing";
                claimed.push(follow_up.clone());
            }
        }

        Ok(claimed)
    }

    async fn finish(&self, follow_up: &NotifyFollowUp) -> Result<()> {
        for (existing, status) in self.follow_ups.lock().unwrap().iter_mut() {
            if same_follow_up(existing, follow_up) {
                *status = "done";
            }
        }

        Ok(())
    }

    async fn postpone(&self, follow_up: &NotifyFollowUp, due_at: TimestampMicros) -> Result<()> {
        for (existing, status) in self.follow_ups.lock().unwrap().iter_mut() {
            if same_follow_up(existing, follow_up) && *status == "processing" {
                existing.due_at = due_at;
                *status = "pending";
            }
        }

        Ok(())
    }

    async fn cancel(&self, organization_id: &Id, event_id: &str) -> Result<u64> {
        let mut count = 0;

        for (follow_up, status) in self.follow_ups.lock().unwrap().iter_mut() {
            if follow_up.organization_id == *organization_id
                && follow_up.event_id == event_id
                && *status == "pending"
            {
                *status = "cancelled";
                count += 1;
            }
        }

        Ok(count)
    }

    async fn cancel_prefix(&self, organization_id: &Id, event_id_prefix: &str) -> Result<u64> {
        let mut count = 0;

        for (follow_up, status) in self.follow_ups.lock().unwrap().iter_mut() {
            if follow_up.organization_id == *organization_id
                && follow_up.event_id.starts_with(event_id_prefix)
                && *status == "pending"
            {
                *status = "cancelled";
                count += 1;
            }
        }

        Ok(count)
    }
}

#[async_trait]
impl NotifyCallRepository for MemoryStore {
    async fn take_line(
        &self,
        line: &CallLine,
        now: TimestampMicros,
    ) -> Result<Option<TimestampMicros>> {
        let mut lines = self.call_lines.lock().unwrap();

        let Some((_, last)) = lines.get_mut(&line_id(line)) else {
            lines.insert(line_id(line), (line.clone(), now));
            return Ok(Some(TimestampMicros(0)));
        };

        if last.0 + i64::from(line.window_secs) * 1_000_000 > now.0 {
            return Ok(None);
        }

        let previous = *last;
        *last = now;
        Ok(Some(previous))
    }

    async fn release_line(
        &self,
        line: &CallLine,
        taken_at: TimestampMicros,
        previous: TimestampMicros,
    ) -> Result<()> {
        if let Some((_, last)) = self.call_lines.lock().unwrap().get_mut(&line_id(line))
            && *last == taken_at
        {
            *last = previous;
        }

        Ok(())
    }

    async fn enqueue(&self, call: QueuedCall) -> Result<()> {
        let mut queue = self.call_queue.lock().unwrap();

        if !queue
            .iter()
            .any(|queued| queued.delivery_id == call.delivery_id)
        {
            queue.push(call);
        }

        Ok(())
    }

    async fn due_lines(
        &self,
        organization_id: &Id,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<CallLine>> {
        let queue = self.call_queue.lock().unwrap();

        Ok(self
            .call_lines
            .lock()
            .unwrap()
            .values()
            .filter(|(line, last)| {
                line.organization_id == *organization_id
                    && last.0 + i64::from(line.window_secs) * 1_000_000 <= now.0
                    && queue.iter().any(|queued| {
                        queued.connector_id == line.connector_id && queued.line_key == line.key
                    })
            })
            .take(limit as usize)
            .map(|(line, _)| line.clone())
            .collect())
    }

    async fn drain(&self, line: &CallLine) -> Result<Vec<QueuedCall>> {
        let mut queue = self.call_queue.lock().unwrap();

        let (drained, kept) = queue.drain(..).partition::<Vec<_>, _>(|queued| {
            queued.organization_id == line.organization_id
                && queued.connector_id == line.connector_id
                && queued.line_key == line.key
        });

        *queue = kept;
        Ok(drained)
    }
}

#[async_trait]
impl NotifyEventRepository for MemoryStore {
    async fn enqueue(&self, record: NotifyEventRecord) -> Result<NotifyEventRecord> {
        let key = format!("{}:{}", record.event.organization_id, record.event.id);
        let mut events = self.events.lock().unwrap();
        Ok(events.entry(key).or_insert(record).clone())
    }

    async fn get(&self, organization_id: &Id, id: &str) -> Result<NotifyEventRecord> {
        self.events
            .lock()
            .unwrap()
            .get(&format!("{organization_id}:{id}"))
            .cloned()
            .ok_or_else(|| Error::not_found("notify event"))
    }

    async fn claim(
        &self,
        organization_id: &Id,
        id: &str,
        now: TimestampMicros,
    ) -> Result<NotifyEventClaim> {
        let key = format!("{organization_id}:{id}");
        let mut events = self.events.lock().unwrap();

        let record = events
            .get_mut(&key)
            .ok_or_else(|| Error::not_found("notify event"))?;

        let acquired = (record.status == NotifyEventStatus::Pending
            && record.next_attempt_at.0 <= now.0)
            || (record.status == NotifyEventStatus::Processing
                && record
                    .claimed_at
                    .is_some_and(|claimed| claimed.0 <= now.0 - 300_000_000));

        if acquired {
            record.status = NotifyEventStatus::Processing;
            record.attempt += 1;
            record.claimed_at = Some(now);
            record.updated_at = now;
        }

        Ok(NotifyEventClaim {
            record: record.clone(),
            acquired,
        })
    }

    async fn claim_retry(
        &self,
        organization_id: &Id,
        id: &str,
        now: TimestampMicros,
    ) -> Result<NotifyEventClaim> {
        let key = format!("{organization_id}:{id}");
        let mut events = self.events.lock().unwrap();

        let record = events
            .get_mut(&key)
            .ok_or_else(|| Error::not_found("notify event"))?;

        let acquired = record.status != NotifyEventStatus::Processing
            || record
                .claimed_at
                .is_some_and(|claimed| claimed.0 <= now.0 - 300_000_000);

        if acquired {
            record.status = NotifyEventStatus::Processing;
            record.attempt += 1;
            record.claimed_at = Some(now);
            record.updated_at = now;
        }

        Ok(NotifyEventClaim {
            record: record.clone(),
            acquired,
        })
    }

    async fn claim_pending(
        &self,
        organization_id: &Id,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<NotifyEventRecord>> {
        let mut events = self.events.lock().unwrap();
        let mut records = Vec::new();

        for record in events.values_mut().filter(|record| {
            record.event.organization_id == *organization_id
                && ((record.status == NotifyEventStatus::Pending
                    && record.next_attempt_at.0 <= now.0)
                    || (record.status == NotifyEventStatus::Processing
                        && record
                            .claimed_at
                            .is_some_and(|claimed| claimed.0 <= now.0 - 300_000_000)))
        }) {
            record.status = NotifyEventStatus::Processing;
            record.attempt += 1;
            record.claimed_at = Some(now);
            record.updated_at = now;
            records.push(record.clone());

            if records.len() >= limit as usize {
                break;
            }
        }

        Ok(records)
    }

    async fn finish(
        &self,
        organization_id: &Id,
        id: &str,
        status: NotifyEventStatus,
        next_attempt_at: TimestampMicros,
        error: Option<String>,
        now: TimestampMicros,
    ) -> Result<NotifyEventRecord> {
        let key = format!("{organization_id}:{id}");
        let mut events = self.events.lock().unwrap();

        let record = events
            .get_mut(&key)
            .ok_or_else(|| Error::not_found("notify event"))?;

        record.status = status;
        record.next_attempt_at = next_attempt_at;
        record.claimed_at = None;
        record.last_error = error;
        record.updated_at = now;
        Ok(record.clone())
    }

    async fn postpone(
        &self,
        organization_id: &Id,
        id: &str,
        next_attempt_at: TimestampMicros,
        note: &str,
        now: TimestampMicros,
    ) -> Result<NotifyEventRecord> {
        let key = format!("{organization_id}:{id}");
        let mut events = self.events.lock().unwrap();

        let record = events
            .get_mut(&key)
            .ok_or_else(|| Error::not_found("notify event"))?;

        record.status = NotifyEventStatus::Pending;
        record.attempt = (record.attempt - 1).max(0);
        record.next_attempt_at = next_attempt_at;
        record.claimed_at = None;
        record.last_error = Some(note.into());
        record.updated_at = now;
        Ok(record.clone())
    }
}

#[async_trait]
impl NotifyPolicyRepository for MemoryStore {
    async fn create(&self, policy: NotifyPolicy) -> Result<NotifyPolicy> {
        self.policies
            .lock()
            .unwrap()
            .insert(policy.id.0.clone(), policy.clone());

        Ok(policy)
    }

    async fn update(&self, policy: NotifyPolicy) -> Result<NotifyPolicy> {
        NotifyPolicyRepository::create(self, policy).await
    }

    async fn get(&self, organization_id: &Id, id: &Id) -> Result<NotifyPolicy> {
        self.policies
            .lock()
            .unwrap()
            .get(id.as_str())
            .filter(|policy| policy.organization_id == *organization_id)
            .cloned()
            .ok_or_else(|| Error::not_found("notify policy"))
    }

    async fn list(&self, organization_id: &Id) -> Result<Vec<NotifyPolicy>> {
        Ok(self
            .policies
            .lock()
            .unwrap()
            .values()
            .filter(|policy| policy.organization_id == *organization_id)
            .cloned()
            .collect())
    }

    async fn list_enabled_for_event(
        &self,
        organization_id: &Id,
        event_type: &str,
    ) -> Result<Vec<NotifyPolicy>> {
        Ok(self
            .policies
            .lock()
            .unwrap()
            .values()
            .filter(|policy| {
                policy.organization_id == *organization_id
                    && policy.event_type == event_type
                    && policy.enabled
            })
            .cloned()
            .collect())
    }

    async fn delete(&self, organization_id: &Id, id: &Id) -> Result<()> {
        NotifyPolicyRepository::get(self, organization_id, id).await?;
        self.policies.lock().unwrap().remove(id.as_str());
        Ok(())
    }

    /// The store keeps no built-in rows to restore from.
    async fn restore_builtin(&self, _organization_id: &Id, _id: &Id) -> Result<NotifyPolicy> {
        Err(Error::not_found("built-in notify policy"))
    }
}

#[async_trait]
impl TeamNotifyDefaultRepository for MemoryStore {
    async fn get(
        &self,
        organization_id: &Id,
        team_id: &Id,
        category: NotifyCategory,
    ) -> Result<Option<TeamNotifyDefault>> {
        Ok(self
            .team_defaults
            .lock()
            .unwrap()
            .values()
            .find(|defaults| {
                defaults.organization_id == *organization_id
                    && defaults.team_id == *team_id
                    && defaults.category == category
            })
            .cloned())
    }

    async fn list(&self, organization_id: &Id, team_id: &Id) -> Result<Vec<TeamNotifyDefault>> {
        Ok(self
            .team_defaults
            .lock()
            .unwrap()
            .values()
            .filter(|defaults| {
                defaults.organization_id == *organization_id && defaults.team_id == *team_id
            })
            .cloned()
            .collect())
    }

    async fn upsert(&self, defaults: TeamNotifyDefault) -> Result<TeamNotifyDefault> {
        self.team_defaults
            .lock()
            .unwrap()
            .insert(defaults.id.0.clone(), defaults.clone());

        Ok(defaults)
    }

    async fn delete(
        &self,
        organization_id: &Id,
        team_id: &Id,
        category: NotifyCategory,
    ) -> Result<()> {
        let id = TeamNotifyDefaultRepository::get(self, organization_id, team_id, category)
            .await?
            .ok_or_else(|| Error::not_found("team notify default"))?
            .id;

        self.team_defaults.lock().unwrap().remove(id.as_str());
        Ok(())
    }
}

#[async_trait]
impl OrganizationNotifyDefaultRepository for MemoryStore {
    async fn get(
        &self,
        organization_id: &Id,
        category: NotifyCategory,
    ) -> Result<Option<OrganizationNotifyDefault>> {
        Ok(self
            .organization_defaults
            .lock()
            .unwrap()
            .values()
            .find(|defaults| {
                defaults.organization_id == *organization_id && defaults.category == category
            })
            .cloned())
    }

    async fn list(&self, organization_id: &Id) -> Result<Vec<OrganizationNotifyDefault>> {
        Ok(self
            .organization_defaults
            .lock()
            .unwrap()
            .values()
            .filter(|defaults| defaults.organization_id == *organization_id)
            .cloned()
            .collect())
    }

    async fn upsert(
        &self,
        defaults: OrganizationNotifyDefault,
    ) -> Result<OrganizationNotifyDefault> {
        self.organization_defaults
            .lock()
            .unwrap()
            .insert(defaults.id.0.clone(), defaults.clone());

        Ok(defaults)
    }

    async fn delete(&self, organization_id: &Id, category: NotifyCategory) -> Result<()> {
        let id = OrganizationNotifyDefaultRepository::get(self, organization_id, category)
            .await?
            .ok_or_else(|| Error::not_found("organization notify default"))?
            .id;

        self.organization_defaults
            .lock()
            .unwrap()
            .remove(id.as_str());

        Ok(())
    }
}

#[async_trait]
impl NotifyTemplateRepository for MemoryStore {
    async fn get(&self, organization_id: &Id, id: &Id) -> Result<NotifyTemplate> {
        self.templates
            .lock()
            .unwrap()
            .get(id.as_str())
            .filter(|template| template.organization_id == *organization_id)
            .cloned()
            .ok_or_else(|| Error::not_found("notify template"))
    }

    async fn list(&self, organization_id: &Id) -> Result<Vec<NotifyTemplate>> {
        let mut templates = self
            .templates
            .lock()
            .unwrap()
            .values()
            .filter(|template| template.organization_id == *organization_id)
            .cloned()
            .collect::<Vec<_>>();

        templates.sort_by(|left, right| left.id.0.cmp(&right.id.0));
        Ok(templates)
    }
}

impl MemoryStore {
    /// 存下一个通知模板。
    pub(crate) fn add_template(&self, template: NotifyTemplate) {
        self.templates
            .lock()
            .unwrap()
            .insert(template.id.0.clone(), template);
    }
}

#[async_trait]
impl TeamRepository for MemoryStore {
    async fn create(&self, team: Team) -> Result<Team> {
        self.teams
            .lock()
            .unwrap()
            .insert(team.id.0.clone(), team.clone());

        Ok(team)
    }

    async fn update(&self, team: Team) -> Result<Team> {
        TeamRepository::create(self, team).await
    }

    async fn get(&self, id: &Id) -> Result<Team> {
        self.teams
            .lock()
            .unwrap()
            .get(id.as_str())
            .cloned()
            .ok_or_else(|| Error::not_found("team"))
    }

    async fn list(&self, organization_id: &Id) -> Result<Vec<Team>> {
        Ok(self
            .teams
            .lock()
            .unwrap()
            .values()
            .filter(|team| team.org_id == *organization_id)
            .cloned()
            .collect())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.teams.lock().unwrap().remove(id.as_str());
        Ok(())
    }
}

#[async_trait]
impl MuteRuleRepository for MemoryStore {
    async fn create(&self, rule: MuteRule) -> Result<MuteRule> {
        let mut rules = self.mute_rules.lock().unwrap();
        rules.retain(|existing| existing.id != rule.id);
        rules.push(rule.clone());
        Ok(rule)
    }

    async fn update(&self, rule: MuteRule) -> Result<MuteRule> {
        MuteRuleRepository::create(self, rule).await
    }

    async fn get(&self, id: &Id) -> Result<MuteRule> {
        self.mute_rules
            .lock()
            .unwrap()
            .iter()
            .find(|rule| rule.id == *id)
            .cloned()
            .ok_or_else(|| Error::not_found("mute rule"))
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<MuteRule>> {
        Ok(self
            .mute_rules
            .lock()
            .unwrap()
            .iter()
            .filter(|rule| rule.org_id == *org_id)
            .cloned()
            .collect())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.mute_rules
            .lock()
            .unwrap()
            .retain(|rule| rule.id != *id);

        Ok(())
    }

    async fn list_enabled(&self, org_id: &Id) -> Result<Vec<MuteRule>> {
        let rules = MuteRuleRepository::list(self, org_id).await?;
        Ok(rules.into_iter().filter(|rule| rule.enabled).collect())
    }
}

#[async_trait]
impl IncidentRepository for MemoryStore {
    async fn create(&self, incident: Incident) -> Result<Incident> {
        self.incidents
            .lock()
            .unwrap()
            .insert(incident.id.0.clone(), incident.clone());

        Ok(incident)
    }

    async fn update(&self, incident: Incident) -> Result<Incident> {
        IncidentRepository::create(self, incident).await
    }

    async fn update_if_status(
        &self,
        incident: Incident,
        expected: &[IncidentStatus],
    ) -> Result<Option<Incident>> {
        let current = IncidentRepository::get(self, &incident.id).await?;

        if !expected.contains(&current.status) || current.revision != incident.revision {
            return Ok(None);
        }

        let written = Incident {
            revision: incident.revision + 1,
            ..incident
        };

        IncidentRepository::create(self, written).await.map(Some)
    }

    async fn list_merged_into(&self, _org_id: &Id, target_id: &Id) -> Result<Vec<Incident>> {
        Ok(self
            .incidents
            .lock()
            .unwrap()
            .values()
            .filter(|incident| incident.handling.merged_into.as_ref() == Some(target_id))
            .cloned()
            .collect())
    }

    async fn get(&self, id: &Id) -> Result<Incident> {
        self.incidents
            .lock()
            .unwrap()
            .get(id.as_str())
            .cloned()
            .ok_or_else(|| Error::not_found("incident"))
    }

    async fn list_active(&self, org_id: &Id) -> Result<Vec<Incident>> {
        Ok(self
            .incidents
            .lock()
            .unwrap()
            .values()
            .filter(|incident| {
                incident.org_id == *org_id
                    && matches!(
                        incident.status,
                        IncidentStatus::Open | IncidentStatus::Acknowledged
                    )
            })
            .cloned()
            .collect())
    }

    async fn list_active_for_rule(&self, org_id: &Id, rule_id: &Id) -> Result<Vec<Incident>> {
        let active = IncidentRepository::list_active(self, org_id).await?;

        Ok(active
            .into_iter()
            .filter(|incident| incident.rule_id == *rule_id)
            .collect())
    }

    async fn find_by_fingerprint(
        &self,
        org_id: &Id,
        fingerprint: &str,
    ) -> Result<Option<Incident>> {
        let active = IncidentRepository::list_active(self, org_id).await?;

        Ok(active
            .into_iter()
            .find(|incident| incident.fingerprint == fingerprint))
    }

    async fn list_by_status(&self, org_id: &Id, status: IncidentStatus) -> Result<Vec<Incident>> {
        Ok(self
            .incidents
            .lock()
            .unwrap()
            .values()
            .filter(|incident| incident.org_id == *org_id && incident.status == status)
            .cloned()
            .collect())
    }

    async fn list_since(&self, org_id: &Id, since: TimestampMicros) -> Result<Vec<Incident>> {
        Ok(self
            .incidents
            .lock()
            .unwrap()
            .values()
            .filter(|incident| incident.org_id == *org_id && incident.created_at.0 >= since.0)
            .cloned()
            .collect())
    }
}

struct StaticResolver;

#[async_trait]
impl RecipientResolver for StaticResolver {
    fn resolver_type(&self) -> &'static str {
        "static_test"
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        if config.get("userId").and_then(Value::as_str).is_none() {
            return Err(Error::invalid("static_test requires userId"));
        }

        Ok(())
    }

    async fn resolve(&self, _event: &NotifyEvent, config: &Value) -> Result<Vec<NotifyRecipient>> {
        self.validate_config(config)?;

        Ok(vec![NotifyRecipient {
            user_id: Id::from_string(config["userId"].as_str().unwrap()),
            team_id: config
                .get("teamId")
                .and_then(Value::as_str)
                .map(Id::from_string),
        }])
    }
}

#[derive(Default)]
struct FakeAdapter {
    sends: Mutex<HashMap<String, usize>>,
    total: AtomicUsize,
    /// 设了就像打电话的连接器：同一个号码一个窗口只拨一次。
    merge_window: Option<u32>,
    /// 每次发送的标题，按先后。
    titles: Mutex<Vec<String>>,
    /// 每次发出去的消息，按先后。
    messages: Mutex<Vec<NotifyMessage>>,
    /// 发往哪个渠道；空着是 IM。
    channel: Option<NotifyChannel>,
}

impl FakeAdapter {
    fn sends_to(&self, target: &str) -> usize {
        self.sends
            .lock()
            .unwrap()
            .get(target)
            .copied()
            .unwrap_or_default()
    }
}

#[async_trait]
impl ConnectorAdapter for FakeAdapter {
    fn connector_type(&self) -> &'static str {
        "fake"
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            direct_user: true,
            group: true,
            ..ConnectorCapabilities::default()
        }
    }

    fn channel(&self) -> NotifyChannel {
        self.channel.unwrap_or(NotifyChannel::Im)
    }

    fn merge_window_secs(&self) -> Option<u32> {
        self.merge_window
    }

    fn validate_config(&self, _config: &Value) -> Result<()> {
        Ok(())
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        if target.value.trim().is_empty() {
            Err(Error::invalid("target cannot be empty"))
        } else {
            Ok(())
        }
    }

    async fn send(
        &self,
        _config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        self.total.fetch_add(1, Ordering::SeqCst);
        self.titles.lock().unwrap().push(message.title.clone());
        self.messages.lock().unwrap().push(message.clone());

        *self
            .sends
            .lock()
            .unwrap()
            .entry(target.value.clone())
            .or_default() += 1;

        if target.value.starts_with("fail-") {
            return Err(Error::unavailable("simulated provider failure"));
        }

        Ok(ConnectorDeliveryResult {
            provider_message_id: Some("provider-message".into()),
            delivered: true,
            latency_ms: 3,
            metadata: Default::default(),
        })
    }
}

pub(crate) fn connector(org: &Id) -> NotifyConnector {
    NotifyConnector {
        id: Id::from_string("connector-a"),
        organization_id: org.clone(),
        name: "Fake connector".into(),
        connector_type: "fake".into(),
        config: serde_json::json!({}),
        capabilities: ConnectorCapabilities {
            direct_user: true,
            group: true,
            ..ConnectorCapabilities::default()
        },
        enabled: true,
        status: ConnectorStatus::Connected,
        last_tested_at: None,
        last_test_status: None,
        last_test_error: None,
        created_at: TimestampMicros(1),
        updated_at: TimestampMicros(1),
    }
}

fn endpoint(org: &Id, user: &Id, id: &str, value: &str) -> UserNotifyEndpoint {
    UserNotifyEndpoint {
        id: Id::from_string(id),
        organization_id: org.clone(),
        user_id: user.clone(),
        connector_id: Id::from_string("connector-a"),
        provider_type: "fake".into(),
        external_identity: value.into(),
        display_name: None,
        metadata: serde_json::json!({}),
        verified: true,
        enabled: true,
        created_at: TimestampMicros(1),
        updated_at: TimestampMicros(1),
    }
}

fn default_route(target: &str) -> NotifyDefaultRoute {
    NotifyDefaultRoute {
        connector_id: Id::from_string("connector-a"),
        target_type: NotifyTargetType::FixedAddress,
        target: target.into(),
        order: 1,
    }
}

#[tokio::test]
async fn falls_back_from_user_to_team_and_organization_and_deduplicates_success() {
    let store = Arc::new(MemoryStore::default());
    let adapter = Arc::new(FakeAdapter::default());
    let connector_registry =
        Arc::new(ConnectorRegistry::new([adapter.clone() as Arc<dyn ConnectorAdapter>]).unwrap());
    let resolver_registry = Arc::new(
        RecipientResolverRegistry::new([Arc::new(StaticResolver) as Arc<dyn RecipientResolver>])
            .unwrap(),
    );
    let org = Id::from_string("org-a");
    let user = Id::from_string("user-a");
    let team = Id::from_string("team-a");
    let policy_id = Id::from_string("policy-a");
    NotifyConnectorRepository::create(store.as_ref(), connector(&org))
        .await
        .unwrap();
    TeamRepository::create(
        store.as_ref(),
        Team {
            id: team.clone(),
            org_id: org.clone(),
            name: "SRE".into(),
            member_ids: vec![user.clone()],
        },
    )
    .await
    .unwrap();
    let primary = endpoint(&org, &user, "endpoint-primary", "fail-primary");
    let fallback = endpoint(&org, &user, "endpoint-fallback", "fail-fallback");
    UserNotifyEndpointRepository::create(store.as_ref(), primary.clone())
        .await
        .unwrap();
    UserNotifyEndpointRepository::create(store.as_ref(), fallback.clone())
        .await
        .unwrap();
    UserNotifyPreferenceRepository::upsert(
        store.as_ref(),
        UserNotifyPreference {
            id: Id::from_string("preference-a"),
            organization_id: org.clone(),
            user_id: user.clone(),
            category: NotifyCategory::Alert,
            enabled: true,
            quiet_hours: None,
            allow_high_urgency_bypass: true,
            steps: vec![
                UserNotifyPreferenceStep {
                    id: Id::new(),
                    preference_id: Id::from_string("preference-a"),
                    endpoint_id: primary.id,
                    step_order: 1,
                    delay_secs: 0,
                    urgency: Default::default(),
                    created_at: TimestampMicros(1),
                },
                UserNotifyPreferenceStep {
                    id: Id::new(),
                    preference_id: Id::from_string("preference-a"),
                    endpoint_id: fallback.id,
                    step_order: 2,
                    delay_secs: 0,
                    urgency: Default::default(),
                    created_at: TimestampMicros(1),
                },
            ],
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        },
    )
    .await
    .unwrap();
    TeamNotifyDefaultRepository::upsert(
        store.as_ref(),
        TeamNotifyDefault {
            id: Id::from_string("team-default-a"),
            organization_id: org.clone(),
            team_id: team.clone(),
            category: NotifyCategory::Alert,
            routes: vec![default_route("fail-team")],
            enabled: true,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        },
    )
    .await
    .unwrap();
    OrganizationNotifyDefaultRepository::upsert(
        store.as_ref(),
        OrganizationNotifyDefault {
            id: Id::from_string("org-default-a"),
            organization_id: org.clone(),
            category: NotifyCategory::Alert,
            routes: vec![default_route("success-org")],
            enabled: true,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        },
    )
    .await
    .unwrap();
    NotifyPolicyRepository::create(
        store.as_ref(),
        NotifyPolicy {
            id: policy_id.clone(),
            organization_id: org.clone(),
            name: "Critical alert".into(),
            builtin_key: None,
            event_type: "alert.triggered".into(),
            category: NotifyCategory::Alert,
            matchers: serde_json::json!({"severity": "critical"}),
            recipient_resolver: "static_test".into(),
            resolver_config: serde_json::json!({
                "userId": user,
                "teamId": team
            }),
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
        },
    )
    .await
    .unwrap();
    let engine = NotifyEngine::new(NotifyEngineDependencies {
        connectors: store.clone(),
        endpoints: store.clone(),
        preferences: store.clone(),
        deliveries: store.clone(),
        events: store.clone(),
        follow_ups: store.clone(),
        calls: store.clone(),
        policies: store.clone(),
        team_defaults: store.clone(),
        organization_defaults: store.clone(),
        teams: store.clone(),
        templates: store.clone(),
        connector_registry,
        resolver_registry,
    });
    let dispatch = NotifyDispatch {
        event: NotifyEvent {
            id: "event-a".into(),
            event_type: "alert.triggered".into(),
            organization_id: org.clone(),
            occurred_at: TimestampMicros(1),
            attributes: serde_json::json!({"severity": "critical"}),
        },
        message: NotifyMessage {
            title: "Critical".into(),
            text: "Service is unavailable".into(),
            markdown: None,
            html: None,
            metadata: Default::default(),
            provider_templates: Default::default(),
            event: None,
            card: None,
        },
    };

    let first = engine.handle_event(dispatch.clone()).await.unwrap();
    let attempts = &first.policies[0].recipients[0].attempts;
    assert!(first.policies[0].recipients[0].delivered);
    assert_eq!(attempts.len(), 4);
    assert_eq!(
        attempts
            .iter()
            .map(|attempt| attempt.stage)
            .collect::<Vec<_>>(),
        vec![
            DeliveryStage::UserPrimary,
            DeliveryStage::UserFallback,
            DeliveryStage::TeamFallback,
            DeliveryStage::OrganizationFallback
        ]
    );
    assert_eq!(adapter.sends_to("success-org"), 1);
    assert_eq!(adapter.sends_to("fail-primary"), 1);
    assert_eq!(adapter.sends_to("fail-fallback"), 1);
    assert_eq!(adapter.sends_to("fail-team"), 1);

    let second = engine.handle_event(dispatch.clone()).await.unwrap();
    assert!(second.policies[0].recipients[0].delivered);
    assert_eq!(adapter.sends_to("success-org"), 1);
    assert_eq!(adapter.sends_to("fail-primary"), 1);
    assert_eq!(adapter.sends_to("fail-fallback"), 1);
    assert_eq!(adapter.sends_to("fail-team"), 1);
    assert_eq!(
        second.policies[0].recipients[0]
            .attempts
            .last()
            .unwrap()
            .status,
        DeliveryStatus::Success
    );

    engine
        .acknowledge_event(&org, "event-a", TimestampMicros::now())
        .await
        .unwrap();

    let mut connector_b = connector(&org);
    connector_b.id = Id::from_string("connector-b");
    connector_b.name = "Second fake connector".into();
    NotifyConnectorRepository::create(store.as_ref(), connector_b)
        .await
        .unwrap();
    let mut endpoint_b = endpoint(&org, &user, "endpoint-b", "success-multi");
    endpoint_b.connector_id = Id::from_string("connector-b");
    UserNotifyEndpointRepository::create(store.as_ref(), endpoint_b)
        .await
        .unwrap();

    let mut policy = NotifyPolicyRepository::get(store.as_ref(), &org, &policy_id)
        .await
        .unwrap();
    policy.delivery_mode = NotifyDeliveryMode::MultiConnector;
    policy.delivery_config = NotifyDeliveryConfig {
        connector_ids: vec![
            Id::from_string("connector-a"),
            Id::from_string("connector-b"),
        ],
    };
    NotifyPolicyRepository::update(store.as_ref(), policy.clone())
        .await
        .unwrap();
    let mut multi_dispatch = dispatch.clone();
    multi_dispatch.event.id = "event-multi".into();
    let multi = engine.handle_event(multi_dispatch).await.unwrap();
    assert!(multi.policies[0].recipients[0].delivered);
    assert_eq!(
        multi.policies[0].recipients[0]
            .attempts
            .iter()
            .filter(|delivery| delivery.stage == DeliveryStage::UserPrimary)
            .count(),
        2
    );
    assert_eq!(adapter.sends_to("success-multi"), 1);
    engine
        .acknowledge_event(&org, "event-multi", TimestampMicros::now())
        .await
        .unwrap();

    policy.delivery_mode = NotifyDeliveryMode::PreferUser;
    policy.delivery_config = NotifyDeliveryConfig::default();
    policy.ack_timeout_seconds = Some(1);
    policy.escalation_config = Some(serde_json::json!({
        "recipientResolver": "static_test",
        "resolverConfig": {
            "userId": user,
            "teamId": team
        },
        "deliveryMode": "force_connector",
        "deliveryConfig": {
            "connectorIds": ["connector-b"]
        },
        "fallbackConfig": {
            "useUserFallbacks": false,
            "useTeamDefaults": false,
            "useOrganizationDefaults": false
        }
    }));
    NotifyPolicyRepository::update(store.as_ref(), policy)
        .await
        .unwrap();
    let mut escalation_dispatch = dispatch;
    escalation_dispatch.event.id = "event-escalation".into();
    let initial = engine.handle_event(escalation_dispatch).await.unwrap();
    assert!(initial.policies[0].recipients[0].delivered);
    assert_eq!(adapter.sends_to("success-org"), 2);

    let escalated = engine
        .process_due_escalations(
            &org,
            TimestampMicros(TimestampMicros::now().0 + 2_000_000),
            100,
        )
        .await
        .unwrap();
    assert_eq!(escalated, 1);
    assert_eq!(adapter.sends_to("success-multi"), 2);
    let escalation_attempts =
        NotifyDeliveryRepository::list(store.as_ref(), &org, &DeliveryFilter::default())
            .await
            .unwrap();
    assert!(escalation_attempts.iter().any(|delivery| {
        delivery.event_id == "event-escalation"
            && delivery.stage == DeliveryStage::Escalation
            && delivery.status == DeliveryStatus::Success
    }));

    let mut organization_default =
        OrganizationNotifyDefaultRepository::get(store.as_ref(), &org, NotifyCategory::Alert)
            .await
            .unwrap()
            .unwrap();
    organization_default.routes = vec![default_route("fail-org")];
    OrganizationNotifyDefaultRepository::upsert(store.as_ref(), organization_default)
        .await
        .unwrap();
    let failed_dispatch = NotifyDispatch {
        event: NotifyEvent {
            id: "event-retry".into(),
            event_type: "alert.triggered".into(),
            organization_id: org.clone(),
            occurred_at: TimestampMicros(2),
            attributes: serde_json::json!({"severity": "critical"}),
        },
        message: NotifyMessage {
            title: "Critical".into(),
            text: "Retry me".into(),
            markdown: None,
            html: None,
            metadata: Default::default(),
            provider_templates: Default::default(),
            event: None,
            card: None,
        },
    };
    assert!(engine.handle_event(failed_dispatch).await.is_err());
    let retry_record = NotifyEventRepository::get(store.as_ref(), &org, "event-retry")
        .await
        .unwrap();
    assert_eq!(retry_record.status, NotifyEventStatus::Pending);
    assert_eq!(retry_record.attempt, 1);

    let mut organization_default =
        OrganizationNotifyDefaultRepository::get(store.as_ref(), &org, NotifyCategory::Alert)
            .await
            .unwrap()
            .unwrap();
    organization_default.routes = vec![default_route("success-after-retry")];
    OrganizationNotifyDefaultRepository::upsert(store.as_ref(), organization_default)
        .await
        .unwrap();
    assert_eq!(
        engine
            .process_pending_events(
                &org,
                TimestampMicros(TimestampMicros::now().0 + 10_000_000),
                100,
            )
            .await
            .unwrap(),
        1
    );
    let retry_record = NotifyEventRepository::get(store.as_ref(), &org, "event-retry")
        .await
        .unwrap();
    assert_eq!(retry_record.status, NotifyEventStatus::Completed);
    assert_eq!(retry_record.attempt, 2);
    assert_eq!(adapter.sends_to("success-after-retry"), 1);
}

/// 一个人、一条告警触发的策略，接收顺序是 `steps`：各步的端点值与等待秒数。
async fn alert_order(
    adapter: Arc<FakeAdapter>,
    steps: &[(&str, i32)],
) -> (Arc<MemoryStore>, NotifyEngine, Id) {
    let store = Arc::new(MemoryStore::default());
    let org = Id::from_string("org-a");
    let user = Id::from_string("user-a");

    NotifyConnectorRepository::create(store.as_ref(), connector(&org))
        .await
        .unwrap();

    let mut order = Vec::new();

    for (index, (value, delay_secs)) in steps.iter().enumerate() {
        let endpoint = endpoint(&org, &user, &format!("endpoint-{index}"), value);

        UserNotifyEndpointRepository::create(store.as_ref(), endpoint.clone())
            .await
            .unwrap();

        order.push(UserNotifyPreferenceStep {
            id: Id::new(),
            preference_id: Id::from_string("preference-a"),
            endpoint_id: endpoint.id,
            step_order: i32::try_from(index + 1).unwrap(),
            delay_secs: *delay_secs,
            urgency: Default::default(),
            created_at: TimestampMicros(1),
        });
    }

    UserNotifyPreferenceRepository::upsert(
        store.as_ref(),
        UserNotifyPreference {
            id: Id::from_string("preference-a"),
            organization_id: org.clone(),
            user_id: user.clone(),
            category: NotifyCategory::Alert,
            enabled: true,
            quiet_hours: None,
            allow_high_urgency_bypass: true,
            steps: order,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        },
    )
    .await
    .unwrap();

    NotifyPolicyRepository::create(
        store.as_ref(),
        NotifyPolicy {
            id: Id::from_string("policy-a"),
            organization_id: org.clone(),
            name: "Alerts".into(),
            builtin_key: None,
            event_type: "alert.triggered".into(),
            category: NotifyCategory::Alert,
            matchers: serde_json::json!({"severity": "critical"}),
            recipient_resolver: "static_test".into(),
            resolver_config: serde_json::json!({"userId": user}),
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
        },
    )
    .await
    .unwrap();

    let engine = NotifyEngine::new(NotifyEngineDependencies {
        connectors: store.clone(),
        endpoints: store.clone(),
        preferences: store.clone(),
        deliveries: store.clone(),
        events: store.clone(),
        follow_ups: store.clone(),
        calls: store.clone(),
        policies: store.clone(),
        team_defaults: store.clone(),
        organization_defaults: store.clone(),
        teams: store.clone(),
        templates: store.clone(),
        connector_registry: Arc::new(
            ConnectorRegistry::new([adapter as Arc<dyn ConnectorAdapter>]).unwrap(),
        ),
        resolver_registry: Arc::new(
            RecipientResolverRegistry::new(
                [Arc::new(StaticResolver) as Arc<dyn RecipientResolver>],
            )
            .unwrap(),
        ),
    });

    (store, engine, org)
}

/// 告警事件 `incident` 触发的通知。
fn triggered(org: &Id, incident: &str, title: &str) -> NotifyDispatch {
    NotifyDispatch {
        event: NotifyEvent {
            id: format!("alert.triggered:{incident}"),
            event_type: "alert.triggered".into(),
            organization_id: org.clone(),
            occurred_at: TimestampMicros(1),
            attributes: serde_json::json!({"severity": "critical"}),
        },
        message: NotifyMessage {
            title: title.into(),
            text: "Service is unavailable".into(),
            markdown: None,
            html: None,
            metadata: Default::default(),
            provider_templates: Default::default(),
            event: None,
            card: None,
        },
    }
}

fn after(start: TimestampMicros, secs: i64) -> TimestampMicros {
    TimestampMicros(start.0 + secs * 1_000_000)
}

#[tokio::test]
async fn a_later_step_waits_for_its_delay_and_stops_once_the_alert_is_acknowledged() {
    let adapter = Arc::new(FakeAdapter::default());
    let (_store, engine, org) = alert_order(
        adapter.clone(),
        &[
            ("success-im", 0),
            ("success-sms", 180),
            ("success-phone", 300),
        ],
    )
    .await;
    let start = TimestampMicros::now();

    engine
        .handle_event(triggered(&org, "inc-1", "Checkout down"))
        .await
        .unwrap();
    assert_eq!(adapter.sends_to("success-im"), 1);
    assert_eq!(adapter.sends_to("success-sms"), 0);

    // 还没到第二步的等待时间。
    assert_eq!(
        engine
            .process_due_follow_ups(&org, after(start, 179), 100)
            .await
            .unwrap(),
        0
    );
    assert_eq!(adapter.sends_to("success-sms"), 0);

    assert_eq!(
        engine
            .process_due_follow_ups(&org, after(start, 181), 100)
            .await
            .unwrap(),
        1
    );
    assert_eq!(adapter.sends_to("success-sms"), 1);
    assert_eq!(adapter.sends_to("success-phone"), 0);

    // 有人确认了：第三步不再打。
    engine
        .acknowledge_alert(&org, &Id::from_string("inc-1"), after(start, 200))
        .await
        .unwrap();
    assert_eq!(
        engine
            .process_due_follow_ups(&org, after(start, 301), 100)
            .await
            .unwrap(),
        0
    );
    assert_eq!(adapter.sends_to("success-phone"), 0);
}

/// 按钮的字，记下问到的收件人。
#[derive(Default)]
struct FakeButtons {
    asked: Mutex<Vec<Option<Id>>>,
}

#[async_trait]
impl AlertButtonSource for FakeButtons {
    async fn labels(
        &self,
        _connector: &NotifyConnector,
        recipient: Option<&Id>,
    ) -> Option<AlertButtonLabels> {
        self.asked.lock().unwrap().push(recipient.cloned());

        Some(AlertButtonLabels {
            acknowledge: "确认".into(),
            resolve: "解决".into(),
            snooze: "暂缓 1 小时".into(),
            acknowledge_all: "全部确认".into(),
            open: None,
        })
    }
}

#[tokio::test]
async fn an_alert_sent_where_presses_are_heard_carries_what_its_status_allows() {
    let adapter = Arc::new(FakeAdapter::default());
    let (store, engine, org) = alert_order(adapter.clone(), &[("success-im", 0)]).await;
    let buttons = Arc::new(FakeButtons::default());
    engine.fill_alert_buttons(buttons.clone());

    let mut open = triggered(&org, "inc-b", "Checkout down");
    open.event.attributes =
        serde_json::json!({"severity": "critical", "alert_id": "inc-b", "status": "open"});
    engine.handle_event(open).await.unwrap();

    let sent = AlertButtons::read(&adapter.messages.lock().unwrap()[0]).unwrap();
    assert_eq!(sent.alert_id, "inc-b");
    assert_eq!(sent.event_type, "alert.triggered");
    assert_eq!(sent.policy_id.as_deref(), Some("policy-a"));
    assert_eq!(
        sent.actions,
        vec![
            (AlertAction::Acknowledge, "确认".to_string()),
            (AlertAction::Resolve, "解决".to_string()),
            (AlertAction::Snooze, "暂缓 1 小时".to_string()),
        ]
    );
    // 私聊发给一个人：字用他的语言。
    assert_eq!(
        *buttons.asked.lock().unwrap(),
        [Some(Id::from_string("user-a"))]
    );

    // 解决了的通知不带按钮。
    let mut resolved = triggered(&org, "inc-b", "Checkout down");
    resolved.event.id = "alert.resolved:inc-b".into();
    resolved.event.event_type = "alert.resolved".into();
    resolved.event.attributes =
        serde_json::json!({"severity": "critical", "alert_id": "inc-b", "status": "resolved"});
    let mut policy =
        NotifyPolicyRepository::get(store.as_ref(), &org, &Id::from_string("policy-a"))
            .await
            .unwrap();
    policy.event_type = "alert.resolved".into();
    NotifyPolicyRepository::update(store.as_ref(), policy)
        .await
        .unwrap();
    engine.handle_event(resolved).await.unwrap();

    let messages = adapter.messages.lock().unwrap();
    assert_eq!(messages.len(), 2);
    assert!(AlertButtons::read(&messages[1]).is_none());
}

#[tokio::test]
async fn a_step_that_fails_moves_on_at_once_and_the_rest_still_wait() {
    let adapter = Arc::new(FakeAdapter::default());
    let (store, engine, org) = alert_order(
        adapter.clone(),
        &[("fail-im", 0), ("success-sms", 180), ("success-phone", 300)],
    )
    .await;
    let start = TimestampMicros::now();

    let outcome = engine
        .handle_event(triggered(&org, "inc-2", "Disk full"))
        .await
        .unwrap();
    assert!(outcome.policies[0].recipients[0].delivered);
    assert_eq!(adapter.sends_to("fail-im"), 1);
    assert_eq!(adapter.sends_to("success-sms"), 1);
    assert_eq!(adapter.sends_to("success-phone"), 0);

    engine
        .process_due_follow_ups(&org, after(start, 301), 100)
        .await
        .unwrap();
    assert_eq!(adapter.sends_to("success-phone"), 1);

    // 不等人确认的事件（如恢复）不排后续步骤。
    let mut resolved = triggered(&org, "inc-2", "Disk full");
    resolved.event.id = "alert.resolved:inc-2".into();
    resolved.event.event_type = "alert.resolved".into();
    let mut policy =
        NotifyPolicyRepository::get(store.as_ref(), &org, &Id::from_string("policy-a"))
            .await
            .unwrap();
    policy.event_type = "alert.resolved".into();
    NotifyPolicyRepository::update(store.as_ref(), policy)
        .await
        .unwrap();
    engine.handle_event(resolved).await.unwrap();
    assert!(
        store
            .follow_ups
            .lock()
            .unwrap()
            .iter()
            .all(|(follow_up, _)| follow_up.event_id != "alert.resolved:inc-2")
    );
}

#[tokio::test]
async fn calls_to_one_number_within_a_minute_are_merged_into_one() {
    let adapter = Arc::new(FakeAdapter {
        merge_window: Some(60),
        channel: Some(NotifyChannel::Phone),
        ..FakeAdapter::default()
    });
    let (store, engine, org) = alert_order(adapter.clone(), &[("phone", 0)]).await;
    let start = TimestampMicros::now();

    engine
        .handle_event(triggered(&org, "inc-a", "Checkout down"))
        .await
        .unwrap();
    assert_eq!(adapter.sends_to("phone"), 1);

    // 一分钟内再来的两条排在线路上，算作送达。
    for (incident, title) in [("inc-b", "Disk full"), ("inc-c", "Payments slow")] {
        let outcome = engine
            .handle_event(triggered(&org, incident, title))
            .await
            .unwrap();
        let attempt = &outcome.policies[0].recipients[0].attempts[0];
        assert!(outcome.policies[0].recipients[0].delivered);
        assert_eq!(attempt.status, DeliveryStatus::Queued);
    }
    assert_eq!(adapter.sends_to("phone"), 1);

    assert_eq!(
        engine
            .process_merged_calls(&org, after(start, 30), 100)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .process_merged_calls(&org, after(start, 61), 100)
            .await
            .unwrap(),
        1
    );
    assert_eq!(adapter.sends_to("phone"), 2);
    assert_eq!(
        adapter.titles.lock().unwrap().last().unwrap(),
        "Disk full; Payments slow"
    );

    let deliveries =
        NotifyDeliveryRepository::list(store.as_ref(), &org, &DeliveryFilter::default())
            .await
            .unwrap();
    assert!(
        deliveries
            .iter()
            .filter(|delivery| delivery.event_id != "alert.triggered:inc-a")
            .all(|delivery| delivery.status == DeliveryStatus::Success)
    );

    // 排着的这条在拨出前被确认：不再拨，线路也放回去。
    engine
        .handle_event(triggered(&org, "inc-d", "Queue backed up"))
        .await
        .unwrap();
    engine
        .acknowledge_alert(&org, &Id::from_string("inc-d"), after(start, 70))
        .await
        .unwrap();
    assert_eq!(
        engine
            .process_merged_calls(&org, after(start, 122), 100)
            .await
            .unwrap(),
        0
    );
    assert_eq!(adapter.sends_to("phone"), 2);
}

#[tokio::test]
async fn a_policy_merge_window_lists_alerts_to_one_chat_in_one_message() {
    let adapter = Arc::new(FakeAdapter::default());
    let (store, engine, org) = alert_order(adapter.clone(), &[("chat", 0)]).await;
    let start = TimestampMicros::now();

    let mut policy =
        NotifyPolicyRepository::get(store.as_ref(), &org, &Id::from_string("policy-a"))
            .await
            .unwrap();
    policy.merge_window_secs = Some(60);
    NotifyPolicyRepository::update(store.as_ref(), policy)
        .await
        .unwrap();

    // 第一条照常立即发出，窗口里的两条排着。
    for (incident, title) in [
        ("inc-a", "Checkout down"),
        ("inc-b", "Disk full"),
        ("inc-c", "Payments slow"),
    ] {
        engine
            .handle_event(triggered(&org, incident, title))
            .await
            .unwrap();
    }
    assert_eq!(adapter.sends_to("chat"), 1);

    assert_eq!(
        engine
            .process_merged_calls(&org, after(start, 61), 100)
            .await
            .unwrap(),
        1
    );
    assert_eq!(adapter.sends_to("chat"), 2);

    let merged = adapter.messages.lock().unwrap().last().cloned().unwrap();
    assert!(merged.title.ends_with("(+1)"), "{}", merged.title);
    assert_eq!(merged.text.lines().count(), 2);
    assert!(merged.card.is_none());
}

#[tokio::test]
async fn webhooks_are_never_merged() {
    let adapter = Arc::new(FakeAdapter {
        channel: Some(NotifyChannel::Webhook),
        ..FakeAdapter::default()
    });
    let (store, engine, org) = alert_order(adapter.clone(), &[("hook", 0)]).await;

    let mut policy =
        NotifyPolicyRepository::get(store.as_ref(), &org, &Id::from_string("policy-a"))
            .await
            .unwrap();
    policy.merge_window_secs = Some(60);
    NotifyPolicyRepository::update(store.as_ref(), policy)
        .await
        .unwrap();

    for incident in ["inc-a", "inc-b"] {
        engine
            .handle_event(triggered(&org, incident, "Checkout down"))
            .await
            .unwrap();
    }

    assert_eq!(adapter.sends_to("hook"), 2);
}

#[tokio::test]
async fn each_channel_is_sent_with_the_template_the_policy_chose_for_it() {
    let template =
        |id: &str, channel: Option<NotifyChannel>, body: &str, format: &str| NotifyTemplate {
            id: Id::from_string(id),
            organization_id: Id::from_string("org-a"),
            category: NotifyCategory::Alert,
            channel,
            body: body.into(),
            format: format.into(),
            provider_templates: Default::default(),
            card: None,
        };

    for (channel, title, text) in [
        (NotifyChannel::Sms, "", "SMS critical"),
        (NotifyChannel::Im, "Checkout down", "**Checkout down**"),
    ] {
        let adapter = Arc::new(FakeAdapter {
            channel: Some(channel),
            ..FakeAdapter::default()
        });
        let (store, engine, org) = alert_order(adapter.clone(), &[("success-a", 0)]).await;

        store.add_template(template(
            "template-general",
            None,
            "**{{message.title}}**",
            "markdown",
        ));
        store.add_template(template(
            "template-sms",
            Some(NotifyChannel::Sms),
            "SMS {{severity}}",
            "text",
        ));

        let policy_id = Id::from_string("policy-a");
        let mut policy = NotifyPolicyRepository::get(store.as_ref(), &org, &policy_id)
            .await
            .unwrap();
        policy.template_id = Some(Id::from_string("template-general"));
        policy.channel_templates = [(NotifyChannel::Sms, Id::from_string("template-sms"))].into();
        NotifyPolicyRepository::update(store.as_ref(), policy)
            .await
            .unwrap();

        engine
            .handle_event(triggered(&org, "inc-1", "Checkout down"))
            .await
            .unwrap();

        let sent = adapter.messages.lock().unwrap().last().cloned().unwrap();
        assert_eq!(sent.title, title, "{channel:?}");
        assert_eq!(sent.text, text, "{channel:?}");

        // 预览说出每一步用哪个模板。
        let preview = engine
            .preview_policy(
                &org,
                &policy_id,
                triggered(&org, "inc-2", "Checkout down").event,
            )
            .await
            .unwrap();
        let expected = match channel {
            NotifyChannel::Sms => "template-sms",
            _ => "template-general",
        };
        assert_eq!(
            preview.recipients[0].delivery_plan[0]
                .template_id
                .as_ref()
                .map(Id::as_str),
            Some(expected)
        );
    }
}

#[tokio::test]
async fn a_built_in_policy_keeps_its_name_and_is_turned_off_rather_than_deleted() {
    let (store, engine, org) =
        alert_order(Arc::new(FakeAdapter::default()), &[("success-a", 0)]).await;
    let policy_id = Id::from_string("policy-a");

    let input = |policy: &NotifyPolicy| crate::app::notify::NotifyPolicyInput {
        name: policy.name.clone(),
        event_type: policy.event_type.clone(),
        category: policy.category,
        matchers: policy.matchers.clone(),
        recipient_resolver: policy.recipient_resolver.clone(),
        resolver_config: policy.resolver_config.clone(),
        delivery_mode: policy.delivery_mode,
        delivery_config: policy.delivery_config.clone(),
        template_id: policy.template_id.clone(),
        channel_templates: policy.channel_templates.clone(),
        fallback_config: policy.fallback_config.clone(),
        ack_timeout_seconds: policy.ack_timeout_seconds,
        escalation_config: policy.escalation_config.clone(),
        merge_window_secs: policy.merge_window_secs,
        time: policy.time.clone(),
        enabled: policy.enabled,
        priority: policy.priority,
    };

    // A policy of the organization's own has nothing to be restored to.
    assert!(engine.restore_policy(&org, &policy_id).await.is_err());

    let mut policy = NotifyPolicyRepository::get(store.as_ref(), &org, &policy_id)
        .await
        .unwrap();
    policy.builtin_key = Some("alert_triggered".into());
    NotifyPolicyRepository::update(store.as_ref(), policy.clone())
        .await
        .unwrap();

    let renamed = crate::app::notify::NotifyPolicyInput {
        name: "Renamed".into(),
        ..input(&policy)
    };
    assert!(
        engine
            .update_policy(&org, &policy_id, renamed)
            .await
            .is_err()
    );

    let turned_off = crate::app::notify::NotifyPolicyInput {
        enabled: false,
        ..input(&policy)
    };
    let updated = engine
        .update_policy(&org, &policy_id, turned_off)
        .await
        .unwrap();
    assert!(!updated.enabled);
    assert_eq!(updated.builtin_key.as_deref(), Some("alert_triggered"));

    assert!(engine.delete_policy(&org, &policy_id).await.is_err());
    assert!(
        NotifyPolicyRepository::get(store.as_ref(), &org, &policy_id)
            .await
            .is_ok()
    );
}

/// 带 `service=checkout` 的 critical 告警事件，`opened` 时开启。
fn checkout_incident(org: &Id, id: &str, opened: TimestampMicros) -> Incident {
    Incident {
        id: Id::from_string(id),
        org_id: org.clone(),
        rule_id: Id::from_string("rule-a"),
        escalation_policy_id: Id::from_string(""),
        status: IncidentStatus::Open,
        severity: Severity::Critical,
        summary: "Checkout down".into(),
        fingerprint: format!("fingerprint-{id}"),
        current_step: 0,
        current_loop: 0,
        current_step_started_at: opened,
        assignees: Vec::new(),
        labels: [("service".to_string(), "checkout".to_string())].into(),
        annotations: Default::default(),
        trace_ids: Vec::new(),
        host_ids: Vec::new(),
        affected_services: Vec::new(),
        triggering_query: None,
        created_at: opened,
        acknowledged_at: None,
        acknowledged_by: None,
        resolved_at: None,
        resolved_by: None,
        team_id: None,
        routing: None,
        handling: Default::default(),
        revision: 0,
    }
}

/// 从 `start` 起 `secs` 秒内静默 `service=checkout` 的静默。
fn checkout_silence(org: &Id, start: TimestampMicros, secs: i64) -> MuteRule {
    MuteRule {
        id: Id::from_string("silence-checkout"),
        org_id: org.clone(),
        name: "Checkout maintenance".into(),
        enabled: true,
        matchers: vec![LabelMatcher {
            label: "service".into(),
            op: MatchOp::Eq,
            value: "checkout".into(),
        }],
        window: MuteWindow::Fixed {
            start,
            end: after(start, secs),
        },
        comment: String::new(),
        created_by: None,
        created_at: start,
        updated_at: start,
    }
}

#[tokio::test]
async fn a_silenced_alert_is_sent_once_its_silence_ends() {
    let adapter = Arc::new(FakeAdapter::default());
    let (store, engine, org) = alert_order(adapter.clone(), &[("success-im", 0)]).await;
    let engine = engine.with_silences(store.clone(), store.clone());
    let start = TimestampMicros::now();
    let incident = checkout_incident(&org, "inc-1", start);

    IncidentRepository::create(store.as_ref(), incident.clone())
        .await
        .unwrap();
    MuteRuleRepository::create(store.as_ref(), checkout_silence(&org, start, 3_600))
        .await
        .unwrap();

    engine
        .enqueue_event(alert_dispatch(&incident, ALERT_TRIGGERED_EVENT))
        .await
        .unwrap();

    // 静默生效：通知先不发。
    for secs in [1, 60, 600] {
        engine
            .process_pending_events(&org, after(start, secs), 100)
            .await
            .unwrap();
    }
    assert_eq!(adapter.sends_to("success-im"), 0);

    // 静默结束，事件还开着：照常发出，只发一次。
    for secs in [3_601, 3_700] {
        engine
            .process_pending_events(&org, after(start, secs), 100)
            .await
            .unwrap();
    }
    assert_eq!(adapter.sends_to("success-im"), 1);
}

/// 和 `policy-a` 一样，但通知告警恢复。
async fn add_resolved_policy(store: &MemoryStore, org: &Id) {
    let mut policy = NotifyPolicyRepository::get(store, org, &Id::from_string("policy-a"))
        .await
        .unwrap();

    policy.id = Id::from_string("policy-resolved");
    policy.event_type = ALERT_RESOLVED_EVENT.into();

    NotifyPolicyRepository::create(store, policy).await.unwrap();
}

/// 告警事件在 `at` 恢复，发出恢复的通知。
async fn resolve(
    store: &MemoryStore,
    engine: &NotifyEngine,
    incident: &Incident,
    at: TimestampMicros,
) {
    let mut resolved = incident.clone();
    resolved.status = IncidentStatus::Resolved;
    resolved.resolved_at = Some(at);

    IncidentRepository::update(store, resolved.clone())
        .await
        .unwrap();
    engine
        .enqueue_event(alert_dispatch(&resolved, ALERT_RESOLVED_EVENT))
        .await
        .unwrap();
}

#[tokio::test]
async fn an_alert_that_recovers_under_its_silence_is_never_sent() {
    let adapter = Arc::new(FakeAdapter::default());
    let (store, engine, org) = alert_order(adapter.clone(), &[("success-im", 0)]).await;
    let engine = engine.with_silences(store.clone(), store.clone());
    let start = TimestampMicros::now();
    let incident = checkout_incident(&org, "inc-1", start);

    add_resolved_policy(&store, &org).await;
    IncidentRepository::create(store.as_ref(), incident.clone())
        .await
        .unwrap();
    MuteRuleRepository::create(store.as_ref(), checkout_silence(&org, start, 3_600))
        .await
        .unwrap();

    engine
        .enqueue_event(alert_dispatch(&incident, ALERT_TRIGGERED_EVENT))
        .await
        .unwrap();
    engine
        .process_pending_events(&org, after(start, 1), 100)
        .await
        .unwrap();

    resolve(&store, &engine, &incident, after(start, 120)).await;

    for secs in [130, 3_601, 3_700] {
        engine
            .process_pending_events(&org, after(start, secs), 100)
            .await
            .unwrap();
    }

    // 触发与恢复都没发，两条通知都处理完了。
    assert_eq!(adapter.total.load(Ordering::SeqCst), 0);

    for event_id in ["alert.triggered:inc-1", "alert.resolved:inc-1"] {
        let record = NotifyEventRepository::get(store.as_ref(), &org, event_id)
            .await
            .unwrap();
        assert_eq!(record.status, NotifyEventStatus::Completed, "{event_id}");
    }
}

#[tokio::test]
async fn a_failed_notification_is_tried_again_only_while_the_alert_is_open() {
    let adapter = Arc::new(FakeAdapter::default());
    let (store, engine, org) = alert_order(adapter.clone(), &[("fail-im", 0)]).await;
    let engine = engine.with_silences(store.clone(), store.clone());
    let start = TimestampMicros::now();
    let incident = checkout_incident(&org, "inc-1", start);

    IncidentRepository::create(store.as_ref(), incident.clone())
        .await
        .unwrap();

    engine
        .enqueue_event(alert_dispatch(&incident, ALERT_TRIGGERED_EVENT))
        .await
        .unwrap();

    // 开着的时候照常重试。
    for (secs, tries) in [(1, 1), (60, 2)] {
        engine
            .process_pending_events(&org, after(start, secs), 100)
            .await
            .unwrap();

        assert_eq!(adapter.sends_to("fail-im"), tries, "after {secs} s");
    }

    let waiting = NotifyEventRepository::get(store.as_ref(), &org, "alert.triggered:inc-1")
        .await
        .unwrap();

    assert_eq!(waiting.status, NotifyEventStatus::Pending);
    assert!(waiting.last_error.is_some());

    resolve(&store, &engine, &incident, after(start, 120)).await;

    engine
        .process_pending_events(&org, after(start, 600), 100)
        .await
        .unwrap();

    // 恢复之后不再去叫人，这条通知处理完了。
    assert_eq!(adapter.sends_to("fail-im"), 2);

    let settled = NotifyEventRepository::get(store.as_ref(), &org, "alert.triggered:inc-1")
        .await
        .unwrap();

    assert_eq!(settled.status, NotifyEventStatus::Completed);
}

#[tokio::test]
async fn an_alert_sent_before_its_silence_still_reports_its_recovery() {
    let adapter = Arc::new(FakeAdapter::default());
    let (store, engine, org) = alert_order(adapter.clone(), &[("success-im", 0)]).await;
    let engine = engine.with_silences(store.clone(), store.clone());
    let start = TimestampMicros::now();
    let incident = checkout_incident(&org, "inc-1", start);

    add_resolved_policy(&store, &org).await;
    IncidentRepository::create(store.as_ref(), incident.clone())
        .await
        .unwrap();
    MuteRuleRepository::create(
        store.as_ref(),
        checkout_silence(&org, after(start, 60), 3_600),
    )
    .await
    .unwrap();

    engine
        .enqueue_event(alert_dispatch(&incident, ALERT_TRIGGERED_EVENT))
        .await
        .unwrap();
    engine
        .process_pending_events(&org, after(start, 1), 100)
        .await
        .unwrap();
    assert_eq!(adapter.sends_to("success-im"), 1);

    // 静默开始以后恢复：触发通知已经发了，恢复也照常发。
    resolve(&store, &engine, &incident, after(start, 120)).await;
    engine
        .process_pending_events(&org, after(start, 130), 100)
        .await
        .unwrap();
    assert_eq!(adapter.sends_to("success-im"), 2);
}

#[tokio::test]
async fn a_silence_holds_the_later_steps_of_a_person_until_it_ends() {
    let adapter = Arc::new(FakeAdapter::default());
    let (store, engine, org) =
        alert_order(adapter.clone(), &[("success-im", 0), ("success-sms", 180)]).await;
    let engine = engine.with_silences(store.clone(), store.clone());
    let start = TimestampMicros::now();
    let incident = checkout_incident(&org, "inc-1", start);

    IncidentRepository::create(store.as_ref(), incident.clone())
        .await
        .unwrap();
    MuteRuleRepository::create(
        store.as_ref(),
        checkout_silence(&org, after(start, 60), 540),
    )
    .await
    .unwrap();

    engine
        .enqueue_event(alert_dispatch(&incident, ALERT_TRIGGERED_EVENT))
        .await
        .unwrap();
    engine
        .process_pending_events(&org, after(start, 1), 100)
        .await
        .unwrap();
    assert_eq!(adapter.sends_to("success-im"), 1);

    // 第二步到期时在静默里：先不发。
    for secs in [181, 400] {
        engine
            .process_due_follow_ups(&org, after(start, secs), 100)
            .await
            .unwrap();
    }
    assert_eq!(adapter.sends_to("success-sms"), 0);

    // 静默结束，仍没人确认：接着发。
    engine
        .process_due_follow_ups(&org, after(start, 601), 100)
        .await
        .unwrap();
    assert_eq!(adapter.sends_to("success-sms"), 1);
}

#[tokio::test]
async fn a_silence_holds_the_acknowledgement_escalation_of_a_policy() {
    let adapter = Arc::new(FakeAdapter::default());
    let (store, engine, org) = alert_order(adapter.clone(), &[("success-im", 0)]).await;
    let engine = engine.with_silences(store.clone(), store.clone());
    let start = TimestampMicros::now();
    let incident = checkout_incident(&org, "inc-1", start);

    let mut policy =
        NotifyPolicyRepository::get(store.as_ref(), &org, &Id::from_string("policy-a"))
            .await
            .unwrap();
    policy.ack_timeout_seconds = Some(60);
    policy.escalation_config = Some(serde_json::json!({
        "recipientResolver": "static_test",
        "resolverConfig": {"userId": "user-a"},
        "deliveryMode": "force_connector",
        "deliveryConfig": {"connectorIds": ["connector-a"]}
    }));
    NotifyPolicyRepository::update(store.as_ref(), policy)
        .await
        .unwrap();

    IncidentRepository::create(store.as_ref(), incident.clone())
        .await
        .unwrap();
    MuteRuleRepository::create(
        store.as_ref(),
        checkout_silence(&org, after(start, 30), 570),
    )
    .await
    .unwrap();

    engine
        .enqueue_event(alert_dispatch(&incident, ALERT_TRIGGERED_EVENT))
        .await
        .unwrap();
    engine
        .process_pending_events(&org, after(start, 1), 100)
        .await
        .unwrap();
    assert_eq!(adapter.sends_to("success-im"), 1);

    // 确认超时到了，但在静默里：先不升级。
    engine
        .process_due_escalations(&org, after(start, 61), 100)
        .await
        .unwrap();
    assert_eq!(adapter.sends_to("success-im"), 1);

    // 静默结束，仍没人确认：升级。
    engine
        .process_due_escalations(&org, after(start, 601), 100)
        .await
        .unwrap();
    assert_eq!(adapter.sends_to("success-im"), 2);
}

/// 超预算的通知走通知策略：同一档、同一周期、同一额度只发一次，调高额度后再次超出才再发。
#[tokio::test]
async fn a_budget_past_its_limit_is_told_once_until_its_limit_changes() {
    use crate::{
        app::{
            intake::usage::{BudgetStanding, SourceLabel},
            notify::events::budget_dispatch,
        },
        domain::intake::usage::{
            budget::{BudgetPeriod, BudgetState, IntakeBudget},
            source::UsageDimension,
        },
        shared::i18n_language_seed::seeded_pack,
    };

    const MIB: u64 = 1024 * 1024;

    let adapter = Arc::new(FakeAdapter::default());
    let (store, engine, org) = alert_order(adapter.clone(), &[]).await;
    let user = Id::from_string("user-a");
    let endpoint = endpoint(&org, &user, "endpoint-budget", "success-budget");

    UserNotifyEndpointRepository::create(store.as_ref(), endpoint.clone())
        .await
        .unwrap();

    UserNotifyPreferenceRepository::upsert(
        store.as_ref(),
        UserNotifyPreference {
            id: Id::from_string("preference-budget"),
            organization_id: org.clone(),
            user_id: user.clone(),
            category: NotifyCategory::System,
            enabled: true,
            quiet_hours: None,
            allow_high_urgency_bypass: true,
            steps: vec![UserNotifyPreferenceStep {
                id: Id::new(),
                preference_id: Id::from_string("preference-budget"),
                endpoint_id: endpoint.id,
                step_order: 1,
                delay_secs: 0,
                urgency: Default::default(),
                created_at: TimestampMicros(1),
            }],
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        },
    )
    .await
    .unwrap();

    NotifyPolicyRepository::create(
        store.as_ref(),
        NotifyPolicy {
            id: Id::from_string("policy-budget"),
            organization_id: org.clone(),
            name: "Intake budget exceeded".into(),
            builtin_key: Some("intake_budget_exceeded".into()),
            event_type: crate::app::notify::INTAKE_BUDGET_EXCEEDED_EVENT.into(),
            category: NotifyCategory::System,
            matchers: serde_json::json!({}),
            recipient_resolver: "static_test".into(),
            resolver_config: serde_json::json!({"userId": user}),
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
        },
    )
    .await
    .unwrap();

    let copy = seeded_pack("intake_budget.", "en-us");

    let label = SourceLabel {
        kind: "service",
        subkind: None,
        name: "checkout".into(),
        detail: None,
        missing: false,
    };

    let told = |limit_size: u64| {
        let budget = IntakeBudget {
            id: Id::from_string("budget-a"),
            org_id: org.clone(),
            dimension: UsageDimension::Service,
            source_key: "checkout".into(),
            period: BudgetPeriod::Day,
            limit_size,
            warn_percent: 80,
            notify_user_ids: vec![user.clone()],
            created_by: user.clone(),
            updated_by: user.clone(),
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        };

        let standing = BudgetStanding {
            period_start: 0,
            used_bytes: Some(500 * MIB),
            used_complete: true,
            used_ratio: Some(1.0),
            state: Some(BudgetState::Exceeded),
        };

        budget_dispatch(&budget, &standing, &label, &copy, TimestampMicros(2))
            .unwrap()
            .unwrap()
    };

    let later = TimestampMicros(TimestampMicros::now().0 + 10_000_000);

    engine.enqueue_event(told(100)).await.unwrap();
    engine.enqueue_event(told(100)).await.unwrap();
    assert_eq!(
        engine
            .process_pending_events(&org, later, 100)
            .await
            .unwrap(),
        1
    );
    assert_eq!(adapter.sends_to("success-budget"), 1);

    // The next look finds the same budget still over: nothing new to say.
    engine.enqueue_event(told(100)).await.unwrap();
    assert_eq!(
        engine
            .process_pending_events(&org, later, 100)
            .await
            .unwrap(),
        0
    );
    assert_eq!(adapter.sends_to("success-budget"), 1);

    engine.enqueue_event(told(200)).await.unwrap();
    assert_eq!(
        engine
            .process_pending_events(&org, later, 100)
            .await
            .unwrap(),
        1
    );
    assert_eq!(adapter.sends_to("success-budget"), 2);
}

/// 组织启用着的告警流水线，照给的；只记有没有被记数。
struct NotifyStagePipelines(Vec<crate::domain::pipeline::alert::AlertPipeline>);

#[async_trait]
impl crate::domain::pipeline::alert::AlertPipelineRepository for NotifyStagePipelines {
    async fn enabled(
        &self,
        _org: &Id,
    ) -> Result<Vec<crate::domain::pipeline::alert::AlertPipeline>> {
        Ok(self.0.clone())
    }

    async fn record(
        &self,
        _pipeline: &Id,
        _outcome: crate::domain::pipeline::alert::AlertOutcome,
    ) -> Result<()> {
        Ok(())
    }
}

/// 只为 `policies` 里的策略（空着是每一条）在发送前跑 `script` 的流水线。
fn notify_stage(
    org: &Id,
    name: &str,
    policies: serde_json::Value,
    script: &str,
) -> Arc<crate::app::alerting::event_pipelines::AlertPipelines> {
    let config = crate::domain::pipeline::alert::AlertConfig::parse(&serde_json::json!({
        "mode": "alert",
        "alert": { "stage": "notify", "policies": policies },
        "steps": [{ "transformName": "the-step", "script": script }],
    }))
    .unwrap();

    let pipeline = crate::domain::pipeline::alert::AlertPipeline {
        id: Id::from_string(format!("pipeline-{name}")),
        org_id: org.clone(),
        name: name.into(),
        config,
        updated_at: TimestampMicros(1),
    };

    Arc::new(crate::app::alerting::event_pipelines::AlertPipelines::new(
        Arc::new(NotifyStagePipelines(vec![pipeline])),
        Arc::new(crate::infra::runtime::VrlFunctionExecutor::new()),
    ))
}

#[tokio::test]
async fn a_pipeline_before_a_notification_changes_only_what_its_policy_sends() {
    let adapter = Arc::new(FakeAdapter::default());
    let (store, engine, org) = alert_order(adapter.clone(), &[("success-im", 0)]).await;
    let incident = checkout_incident(&org, "inc-1", TimestampMicros::now());

    let engine = engine.with_alert_pipelines(notify_stage(
        &org,
        "tidy",
        serde_json::json!(["policy-a"]),
        ".summary = \"Payments down\"\n.severity = \"error\"\n.labels.team = \"payments\"",
    ));

    let outcome = engine
        .handle_event(alert_dispatch(&incident, ALERT_TRIGGERED_EVENT))
        .await
        .unwrap();

    assert!(outcome.policies[0].recipients[0].delivered);
    assert!(!outcome.policies[0].dropped_by_pipeline);

    {
        let sent = adapter.messages.lock().unwrap();

        assert_eq!(sent[0].title, "ERROR · Payments down");
        assert_eq!(sent[0].text, "Alert Payments down is open.");
    }

    // The event as it was stored stays as it came: another policy, or a retry, starts from it.
    let record = NotifyEventRepository::get(
        store.as_ref(),
        &org,
        &format!("{ALERT_TRIGGERED_EVENT}:inc-1"),
    )
    .await
    .unwrap();

    assert_eq!(record.event.attributes["summary"], "Checkout down");
    assert_eq!(record.event.attributes["severity"], "critical");
    assert!(record.event.attributes["labels"].get("team").is_none());
}

#[tokio::test]
async fn a_policy_a_pipeline_is_not_for_sends_as_it_came() {
    let adapter = Arc::new(FakeAdapter::default());
    let (_store, engine, org) = alert_order(adapter.clone(), &[("success-im", 0)]).await;
    let incident = checkout_incident(&org, "inc-1", TimestampMicros::now());

    let engine = engine.with_alert_pipelines(notify_stage(
        &org,
        "elsewhere",
        serde_json::json!(["policy-b"]),
        "abort",
    ));

    engine
        .handle_event(alert_dispatch(&incident, ALERT_TRIGGERED_EVENT))
        .await
        .unwrap();

    assert_eq!(adapter.sends_to("success-im"), 1);
    assert_eq!(
        adapter.messages.lock().unwrap()[0].title,
        "CRITICAL · Checkout down"
    );
}

#[tokio::test]
async fn a_pipeline_that_drops_a_notification_leaves_a_reason_and_is_not_a_failure() {
    let adapter = Arc::new(FakeAdapter::default());
    let (store, engine, org) = alert_order(adapter.clone(), &[("success-im", 0)]).await;
    let incident = checkout_incident(&org, "inc-1", TimestampMicros::now());

    let engine = engine.with_alert_pipelines(notify_stage(
        &org,
        "quiet",
        serde_json::json!([]),
        "if .labels.service == \"checkout\" { abort }",
    ));

    let dispatch = alert_dispatch(&incident, ALERT_TRIGGERED_EVENT);
    let outcome = engine.handle_event(dispatch.clone()).await.unwrap();

    assert_eq!(adapter.sends_to("success-im"), 0);
    assert!(outcome.policies[0].dropped_by_pipeline);
    assert!(outcome.policies[0].error.is_none());

    let record = NotifyEventRepository::get(store.as_ref(), &org, &dispatch.event.id)
        .await
        .unwrap();

    assert_eq!(record.status, NotifyEventStatus::Completed);

    let deliveries = NotifyDeliveryRepository::list(
        store.as_ref(),
        &org,
        &DeliveryFilter {
            event_id: Some(dispatch.event.id.clone()),
            limit: 10,
            ..DeliveryFilter::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].status, DeliveryStatus::Skipped);
    assert_eq!(deliveries[0].recipient_user_id, None);
    assert_eq!(
        deliveries[0].error_code.as_deref(),
        Some("dropped_by_pipeline")
    );
    assert_eq!(
        deliveries[0].error_message.as_deref(),
        Some("quiet / the-step")
    );
}

#[tokio::test]
async fn a_pipeline_that_fails_before_a_notification_leaves_it_as_it_came() {
    let adapter = Arc::new(FakeAdapter::default());
    let (_store, engine, org) = alert_order(adapter.clone(), &[("success-im", 0)]).await;
    let incident = checkout_incident(&org, "inc-1", TimestampMicros::now());

    let engine = engine.with_alert_pipelines(notify_stage(
        &org,
        "broken",
        serde_json::json!([]),
        ".severity = \"urgent\"",
    ));

    engine
        .handle_event(alert_dispatch(&incident, ALERT_TRIGGERED_EVENT))
        .await
        .unwrap();

    assert_eq!(
        adapter.messages.lock().unwrap()[0].title,
        "CRITICAL · Checkout down"
    );
}
