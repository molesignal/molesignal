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
    app::notify::{ConnectorRegistry, RecipientResolverRegistry},
    domain::{
        iam::{Team, TeamRepository},
        notify::{
            connector::{
                ConnectorAdapter, ConnectorCapabilities, ConnectorDeliveryResult, ConnectorStatus,
                ConnectorTestStatus, NotifyConnector, NotifyMessage, NotifyTarget,
                NotifyTargetType,
            },
            delivery::{
                DeliveryClaim, DeliveryCompletion, DeliveryFilter, DeliveryStage, DeliveryStatus,
                NotifyDelivery,
            },
            endpoint::UserNotifyEndpoint,
            event::{NotifyEventClaim, NotifyEventRecord, NotifyEventStatus},
            policy::{
                NotifyDeliveryConfig, NotifyDeliveryMode, NotifyEvent, NotifyFallbackConfig,
                NotifyPolicy,
            },
            preference::{NotifyCategory, UserNotifyPreference, UserNotifyPreferenceStep},
            recipient::{NotifyRecipient, RecipientResolver},
            repositories::{
                NotifyConnectorRepository, NotifyDeliveryRepository, NotifyEventRepository,
                NotifyPolicyRepository, NotifyTemplateRepository,
                OrganizationNotifyDefaultRepository, TeamNotifyDefaultRepository,
                UserNotifyEndpointRepository, UserNotifyPreferenceRepository,
            },
            routing::{NotifyDefaultRoute, OrganizationNotifyDefault, TeamNotifyDefault},
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Default)]
struct MemoryStore {
    connectors: Mutex<HashMap<String, NotifyConnector>>,
    endpoints: Mutex<HashMap<String, UserNotifyEndpoint>>,
    preferences: Mutex<HashMap<String, UserNotifyPreference>>,
    deliveries: Mutex<HashMap<String, NotifyDelivery>>,
    events: Mutex<HashMap<String, NotifyEventRecord>>,
    policies: Mutex<HashMap<String, NotifyPolicy>>,
    team_defaults: Mutex<HashMap<String, TeamNotifyDefault>>,
    organization_defaults: Mutex<HashMap<String, OrganizationNotifyDefault>>,
    teams: Mutex<HashMap<String, Team>>,
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
                    DeliveryStatus::Success | DeliveryStatus::Acknowledged
                )
        }) {
            delivery.status = DeliveryStatus::Acknowledged;
            delivery.acknowledged_at = Some(acknowledged_at);
            count += 1;
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
    async fn get(
        &self,
        _organization_id: &Id,
        _id: &Id,
    ) -> Result<crate::domain::notify::template::NotifyTemplate> {
        Err(Error::not_found("notify template"))
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

struct StaticResolver;

#[async_trait]
impl RecipientResolver for StaticResolver {
    fn resolver_type(&self) -> &'static str {
        "static_test"
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        if config.get("user_id").and_then(Value::as_str).is_none() {
            return Err(Error::invalid("static_test requires user_id"));
        }
        Ok(())
    }

    async fn resolve(&self, _event: &NotifyEvent, config: &Value) -> Result<Vec<NotifyRecipient>> {
        self.validate_config(config)?;
        Ok(vec![NotifyRecipient {
            user_id: Id::from_string(config["user_id"].as_str().unwrap()),
            team_id: config
                .get("team_id")
                .and_then(Value::as_str)
                .map(Id::from_string),
        }])
    }
}

#[derive(Default)]
struct FakeAdapter {
    sends: Mutex<HashMap<String, usize>>,
    total: AtomicUsize,
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
        _message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        self.total.fetch_add(1, Ordering::SeqCst);
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

fn connector(org: &Id) -> NotifyConnector {
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
            allow_critical_bypass: true,
            steps: vec![
                UserNotifyPreferenceStep {
                    id: Id::new(),
                    preference_id: Id::from_string("preference-a"),
                    endpoint_id: primary.id,
                    step_order: 1,
                    created_at: TimestampMicros(1),
                },
                UserNotifyPreferenceStep {
                    id: Id::new(),
                    preference_id: Id::from_string("preference-a"),
                    endpoint_id: fallback.id,
                    step_order: 2,
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
            event_type: "alert.triggered".into(),
            category: NotifyCategory::Alert,
            matchers: serde_json::json!({"severity": "critical"}),
            recipient_resolver: "static_test".into(),
            resolver_config: serde_json::json!({
                "user_id": user,
                "team_id": team
            }),
            delivery_mode: NotifyDeliveryMode::PreferUser,
            delivery_config: NotifyDeliveryConfig::default(),
            template_id: None,
            fallback_config: NotifyFallbackConfig::default(),
            ack_timeout_seconds: None,
            escalation_config: None,
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
        "recipient_resolver": "static_test",
        "resolver_config": {
            "user_id": user,
            "team_id": team
        },
        "delivery_mode": "force_connector",
        "delivery_config": {
            "connector_ids": ["connector-b"]
        },
        "fallback_config": {
            "use_user_fallbacks": false,
            "use_team_defaults": false,
            "use_organization_defaults": false
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
