// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::Value;

use super::*;
use crate::{
    app::notify::{
        ConnectorRegistry,
        engine::tests::{MemoryStore, connector},
    },
    domain::notify::{
        connector::{ConnectorAdapter, ConnectorCapabilities, ConnectorDeliveryResult},
        repositories::{
            NotifyConnectorRepository, NotifyDeliveryRepository, NotifyRouteReferenceRepository,
        },
    },
};

const TITLE: &str = "Log inspection found a problem: payments";

/// 记下发了什么的连接器。目标以 `fail-` 开头时对方报错，以 `lost-` 开头时对方收下但不确认
/// 送达，带空格的目标不被接受。
#[derive(Default)]
struct Provider {
    sent: Mutex<Vec<(String, String)>>,
}

impl Provider {
    fn sent(&self) -> Vec<(String, String)> {
        self.sent.lock().unwrap().clone()
    }
}

#[async_trait]
impl ConnectorAdapter for Provider {
    fn connector_type(&self) -> &'static str {
        "fake"
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            group: true,
            ..ConnectorCapabilities::default()
        }
    }

    fn validate_config(&self, _config: &Value) -> Result<()> {
        Ok(())
    }

    fn validate_target(&self, target: &NotifyTarget) -> Result<()> {
        if target.value.contains(' ') {
            return Err(Error::invalid("a group id has no spaces"));
        }

        Ok(())
    }

    async fn send(
        &self,
        _config: &Value,
        target: &NotifyTarget,
        message: &NotifyMessage,
    ) -> Result<ConnectorDeliveryResult> {
        self.sent
            .lock()
            .unwrap()
            .push((target.value.clone(), message.title.clone()));

        if target.value.starts_with("fail-") {
            return Err(Error::unavailable("simulated provider failure"));
        }

        Ok(ConnectorDeliveryResult {
            provider_message_id: Some("provider-message".into()),
            delivered: !target.value.starts_with("lost-"),
            latency_ms: 3,
            metadata: Default::default(),
        })
    }
}

struct NoReferences;

#[async_trait]
impl NotifyRouteReferenceRepository for NoReferences {
    async fn count_for_connector(&self, _organization_id: &Id, _connector_id: &Id) -> Result<u64> {
        Ok(0)
    }
}

struct Fixture {
    service: NotifyService,
    store: Arc<MemoryStore>,
    provider: Arc<Provider>,
    org: Id,
}

impl Fixture {
    /// 一个组织，带一个启用的连接器 `connector-a`。
    async fn new() -> Self {
        let store = Arc::new(MemoryStore::default());
        let provider = Arc::new(Provider::default());
        let org = Id::from_string("org-a");

        let registry =
            ConnectorRegistry::new([provider.clone() as Arc<dyn ConnectorAdapter>]).unwrap();

        NotifyConnectorRepository::create(store.as_ref(), connector(&org))
            .await
            .unwrap();

        let service = NotifyService::new(
            store.clone(),
            store.clone(),
            store.clone(),
            store.clone(),
            Arc::new(NoReferences),
            Arc::new(registry),
        );

        Self {
            service,
            store,
            provider,
            org,
        }
    }

    async fn send(&self, target: NotifyTarget, key: &str) -> DirectSendOutcome {
        self.service
            .send_direct(&self.org, direct(target, key))
            .await
    }

    async fn recorded(&self, key: &str) -> Option<NotifyDelivery> {
        NotifyDeliveryRepository::find_by_idempotency_key(self.store.as_ref(), &self.org, key)
            .await
            .unwrap()
    }
}

fn target(target_type: NotifyTargetType, value: &str) -> NotifyTarget {
    NotifyTarget {
        target_type,
        value: value.into(),
        metadata: Default::default(),
    }
}

fn group(value: &str) -> NotifyTarget {
    target(NotifyTargetType::FixedGroup, value)
}

fn direct(target: NotifyTarget, key: &str) -> DirectSend {
    DirectSend {
        connector_id: Id::from_string("connector-a"),
        target,
        message: NotifyMessage {
            title: TITLE.into(),
            text: "Stream: app_logs".into(),
            markdown: None,
            html: None,
            metadata: Default::default(),
        },
        event_id: "log_inspection.run:run-1".into(),
        idempotency_key: key.into(),
    }
}

#[tokio::test]
async fn sends_to_a_fixed_target_and_records_the_delivery() {
    let fixture = Fixture::new().await;
    let outcome = fixture.send(group("oc_ops_7f"), "key-1").await;

    assert_eq!(
        outcome,
        DirectSendOutcome {
            target_masked: mask_target("oc_ops_7f"),
            delivered: true,
            error: None,
        }
    );

    assert_eq!(
        fixture.provider.sent(),
        vec![("oc_ops_7f".to_string(), TITLE.to_string())]
    );

    // 投递记录里是一条不属于任何策略的 `direct`，目标打过码。
    let delivery = fixture.recorded("key-1").await.unwrap();

    assert_eq!(delivery.stage, DeliveryStage::Direct);
    assert_eq!(delivery.status, DeliveryStatus::Success);
    assert_eq!(delivery.event_id, "log_inspection.run:run-1");
    assert_eq!(delivery.policy_id, None);
    assert_eq!(delivery.recipient_user_id, None);
    assert_eq!(delivery.connector_id, Some(Id::from_string("connector-a")));
    assert_eq!(delivery.target_type, "fixed_group");
    assert_eq!(delivery.target_value_masked, Some(outcome.target_masked));
    assert_ne!(delivery.target_value_masked.as_deref(), Some("oc_ops_7f"));
    assert!(delivery.delivered_at.is_some());
}

#[tokio::test]
async fn the_same_thing_is_told_to_a_target_once() {
    let fixture = Fixture::new().await;

    fixture.send(group("oc_ops_7f"), "key-1").await;
    let again = fixture.send(group("oc_ops_7f"), "key-1").await;

    // 第二次不再发，结果仍然是「送到了」。
    assert!(again.delivered);
    assert_eq!(again.error, None);
    assert_eq!(fixture.provider.sent().len(), 1);

    // 另一件事对同一个目标照常发。
    fixture.send(group("oc_ops_7f"), "key-2").await;
    assert_eq!(fixture.provider.sent().len(), 2);
}

#[tokio::test]
async fn a_send_that_fails_is_reported_and_recorded_with_its_reason() {
    let fixture = Fixture::new().await;
    let refused = fixture.send(group("fail-oc_ops"), "key-1").await;

    assert!(!refused.delivered);

    assert!(
        refused
            .error
            .as_deref()
            .is_some_and(|error| error.contains("simulated provider failure")),
        "{:?}",
        refused.error
    );

    let delivery = fixture.recorded("key-1").await.unwrap();

    assert_eq!(delivery.status, DeliveryStatus::Failed);
    assert_eq!(
        delivery.error_code.as_deref(),
        Some("connector_send_failed")
    );
    assert_eq!(delivery.error_message, refused.error);

    // 对方收下了却不确认送达，也不算送到。
    let unconfirmed = fixture.send(group("lost-oc_ops"), "key-2").await;

    assert!(!unconfirmed.delivered);
    assert!(unconfirmed.error.is_some());

    assert_eq!(
        fixture
            .recorded("key-2")
            .await
            .unwrap()
            .error_code
            .as_deref(),
        Some("provider_not_delivered")
    );
}

#[tokio::test]
async fn a_disabled_connector_sends_nothing_and_says_why() {
    let fixture = Fixture::new().await;

    let disabled = NotifyConnector {
        enabled: false,
        ..connector(&fixture.org)
    };

    NotifyConnectorRepository::update(fixture.store.as_ref(), disabled)
        .await
        .unwrap();

    let outcome = fixture.send(group("oc_ops_7f"), "key-1").await;

    assert!(!outcome.delivered);

    assert!(
        outcome
            .error
            .as_deref()
            .is_some_and(|error| error.contains("disabled")),
        "{:?}",
        outcome.error
    );

    assert!(fixture.provider.sent().is_empty());
    // 没到发送那一步，投递记录里没有这一条。
    assert!(fixture.recorded("key-1").await.is_none());
}

#[tokio::test]
async fn a_connector_of_another_organization_is_not_there() {
    let fixture = Fixture::new().await;

    let outcome = fixture
        .service
        .send_direct(
            &Id::from_string("org-b"),
            direct(group("oc_ops_7f"), "key-1"),
        )
        .await;

    assert!(!outcome.delivered);
    assert!(outcome.error.is_some());
    assert!(fixture.provider.sent().is_empty());
}

#[tokio::test]
async fn only_a_fixed_target_the_connector_accepts_can_be_sent_to() {
    let fixture = Fixture::new().await;
    let connector_id = Id::from_string("connector-a");
    let service = &fixture.service;

    // 固定群组和固定地址可以，连接器停用与否不影响它能不能被选为渠道。
    for target_type in [NotifyTargetType::FixedGroup, NotifyTargetType::FixedAddress] {
        let accepted = service
            .validate_direct_target(
                &fixture.org,
                &connector_id,
                &target(target_type, "oc_ops_7f"),
            )
            .await
            .unwrap();

        assert_eq!(accepted.id, connector_id);
    }

    // 发给个人要经过通知策略，不走这条路。
    let person = service
        .validate_direct_target(
            &fixture.org,
            &connector_id,
            &target(NotifyTargetType::DirectUser, "user-a"),
        )
        .await;

    assert!(
        matches!(person, Err(Error::InvalidArgument(_))),
        "{person:?}"
    );

    // 连接器自己不接受的目标。
    let malformed = service
        .validate_direct_target(&fixture.org, &connector_id, &group("oc ops"))
        .await;

    assert!(
        matches!(malformed, Err(Error::InvalidArgument(_))),
        "{malformed:?}"
    );

    let missing = service
        .validate_direct_target(&fixture.org, &Id::from_string("gone"), &group("oc_ops_7f"))
        .await;

    assert!(matches!(missing, Err(Error::NotFound(_))), "{missing:?}");

    // 被拒绝的目标也发不出去。
    let outcome = fixture
        .send(target(NotifyTargetType::DirectUser, "user-a"), "key-1")
        .await;

    assert!(!outcome.delivered);
    assert!(fixture.provider.sent().is_empty());
}
