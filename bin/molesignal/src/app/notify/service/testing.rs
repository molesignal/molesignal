// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::time::Instant;

use super::{ConnectorTestOutcome, NotifyService};
use crate::{
    app::notify::{
        config::{mask_target, truncate_error},
        engine::render_sample,
    },
    domain::notify::{
        connector::{
            ConnectorAdapter, ConnectorTestStatus, NotifyConnector, NotifyMessage, NotifyTarget,
            NotifyTargetType,
        },
        delivery::{DeliveryCompletion, DeliveryStage, DeliveryStatus, NotifyDelivery},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

impl NotifyService {
    /// 用连接器发一条测试通知。选了 `template_id` 就用示例事件套用那个模板发。
    pub async fn test_connector(
        &self,
        organization_id: &Id,
        id: &Id,
        target: NotifyTarget,
        message: NotifyMessage,
        template_id: Option<&Id>,
    ) -> Result<ConnectorTestOutcome> {
        let connector = self.connectors.get(organization_id, id).await?;
        let adapter = self.registry.get(&connector.connector_type)?;

        let message = self
            .test_message(&connector, adapter.as_ref(), message, template_id)
            .await?;

        self.test_target(connector, target, message, None, None)
            .await
    }

    pub async fn test_endpoint(
        &self,
        organization_id: &Id,
        user_id: &Id,
        id: &Id,
        message: NotifyMessage,
    ) -> Result<ConnectorTestOutcome> {
        let endpoint = self.endpoints.get(organization_id, user_id, id).await?;

        if !endpoint.enabled {
            return Err(Error::invalid("user notify endpoint is disabled"));
        }

        let connector = self
            .connectors
            .get(organization_id, &endpoint.connector_id)
            .await?;

        let adapter = self.registry.get(&connector.connector_type)?;

        let message = self
            .test_message(&connector, adapter.as_ref(), message, None)
            .await?;

        self.test_target(
            connector,
            NotifyTarget {
                target_type: NotifyTargetType::DirectUser,
                value: endpoint.external_identity,
                metadata: Default::default(),
            },
            message,
            Some(user_id.clone()),
            Some(endpoint.id),
        )
        .await
    }

    /// 测试发送的内容：选了模板就用示例事件套用它。只发审核过模板的连接器（阿里云）没选模板时，用组织里
    /// 按名称排第一个、这个渠道、写了它的服务商模板的模板；一个都没有就发不出去。其余照原样。
    async fn test_message(
        &self,
        connector: &NotifyConnector,
        adapter: &dyn ConnectorAdapter,
        message: NotifyMessage,
        template_id: Option<&Id>,
    ) -> Result<NotifyMessage> {
        let channel = adapter.channel();

        let template = match (template_id, adapter.template_provider()) {
            (Some(id), _) => {
                let template = self.templates.get(&connector.organization_id, id).await?;

                if template.channel.is_some_and(|own| own != channel) {
                    return Err(Error::invalid(format!(
                        "the template is not for {} notifications",
                        channel.as_str()
                    )));
                }

                template
            }
            (None, Some(provider)) => self
                .templates
                .list(&connector.organization_id)
                .await?
                .into_iter()
                .find(|template| {
                    template.channel == Some(channel)
                        && template.provider_templates.contains_key(provider)
                })
                .ok_or_else(|| {
                    Error::invalid(format!(
                        "{} sends only templates its provider approved: write the template code into a {} notify template first",
                        connector.connector_type,
                        channel.as_str()
                    ))
                })?,
            (None, None) => return Ok(message),
        };

        render_sample(&template, &connector.organization_id)
    }

    async fn test_target(
        &self,
        connector: NotifyConnector,
        target: NotifyTarget,
        message: NotifyMessage,
        recipient_user_id: Option<Id>,
        endpoint_id: Option<Id>,
    ) -> Result<ConnectorTestOutcome> {
        if !connector.enabled {
            return Err(Error::invalid("notify connector is disabled"));
        }

        let adapter = self
            .registry
            .validated(&connector.connector_type, &connector.config)?;
        adapter.validate_target(&target)?;

        let tested_at = TimestampMicros::now();
        let test_id = Id::new();
        let event_id = format!("notify.test:{}", test_id.as_str());

        let claim = self
            .deliveries
            .claim(NotifyDelivery {
                id: Id::new(),
                organization_id: connector.organization_id.clone(),
                event_id,
                policy_id: None,
                recipient_user_id,
                connector_id: Some(connector.id.clone()),
                endpoint_id,
                target_type: target.target_type.as_str().into(),
                target_value_masked: Some(mask_target(&target.value)),
                stage: DeliveryStage::Test,
                attempt: 1,
                status: DeliveryStatus::Sending,
                error_code: None,
                error_message: None,
                latency_ms: None,
                sent_at: Some(tested_at),
                delivered_at: None,
                acknowledged_at: None,
                escalated_at: None,
                idempotency_key: format!(
                    "notify:test:{}:{}:{}",
                    connector.organization_id.as_str(),
                    connector.id.as_str(),
                    test_id.as_str()
                ),
                created_at: tested_at,
            })
            .await?;

        if !claim.acquired {
            return Err(Error::conflict("notify test delivery was already claimed"));
        }

        let started = Instant::now();
        let result = adapter.send_as(&connector, &target, &message).await;
        let finished_at = TimestampMicros::now();
        let (sent, provider_message_id, elapsed_ms, error, status, completion) =
            completion_from_result(result, started, finished_at);

        self.deliveries
            .complete(&connector.organization_id, &claim.delivery.id, completion)
            .await?;

        self.connectors
            .record_test_result(
                &connector.organization_id,
                &connector.id,
                tested_at,
                status,
                error.clone(),
            )
            .await?;

        Ok(ConnectorTestOutcome {
            sent,
            tested_at,
            elapsed_ms,
            provider_message_id,
            error,
        })
    }
}

pub(super) fn completion_from_result(
    result: Result<crate::domain::notify::connector::ConnectorDeliveryResult>,
    started: Instant,
    finished_at: TimestampMicros,
) -> (
    bool,
    Option<String>,
    u64,
    Option<String>,
    ConnectorTestStatus,
    DeliveryCompletion,
) {
    match result {
        Ok(result) if result.delivered => {
            let elapsed_ms = result.latency_ms;

            (
                true,
                result.provider_message_id,
                elapsed_ms,
                None,
                ConnectorTestStatus::Success,
                DeliveryCompletion {
                    status: DeliveryStatus::Success,
                    error_code: None,
                    error_message: None,
                    latency_ms: Some(to_i32_millis(elapsed_ms)),
                    delivered_at: Some(finished_at),
                },
            )
        }
        Ok(result) => {
            let error = "notify provider did not confirm delivery".to_string();
            let elapsed_ms = result.latency_ms;

            (
                false,
                result.provider_message_id,
                elapsed_ms,
                Some(error.clone()),
                ConnectorTestStatus::Failed,
                DeliveryCompletion {
                    status: DeliveryStatus::Failed,
                    error_code: Some("provider_not_delivered".into()),
                    error_message: Some(error),
                    latency_ms: Some(to_i32_millis(elapsed_ms)),
                    delivered_at: None,
                },
            )
        }
        Err(send_error) => {
            let error = truncate_error(&send_error.to_string());
            let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

            (
                false,
                None,
                elapsed_ms,
                Some(error.clone()),
                ConnectorTestStatus::Failed,
                DeliveryCompletion {
                    status: DeliveryStatus::Failed,
                    error_code: Some("connector_send_failed".into()),
                    error_message: Some(error),
                    latency_ms: Some(to_i32_millis(elapsed_ms)),
                    delivered_at: None,
                },
            )
        }
    }
}

fn to_i32_millis(value: u64) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };

    use async_trait::async_trait;
    use serde_json::Value;

    use super::*;
    use crate::{
        app::notify::{
            ConnectorRegistry,
            engine::tests::{MemoryStore, connector},
        },
        domain::notify::{
            connector::{ConnectorCapabilities, ConnectorDeliveryResult, NotifyChannel},
            preference::NotifyCategory,
            repositories::{NotifyConnectorRepository, NotifyRouteReferenceRepository},
            template::{NotifyTemplate, ProviderTemplate},
        },
    };

    #[test]
    fn provider_non_delivery_is_a_failed_test() {
        let result = ConnectorDeliveryResult {
            provider_message_id: Some("provider-message".into()),
            delivered: false,
            latency_ms: 23,
            metadata: Default::default(),
        };
        let (sent, provider_message_id, elapsed_ms, error, status, completion) =
            completion_from_result(Ok(result), Instant::now(), TimestampMicros::now());

        assert!(!sent);
        assert_eq!(provider_message_id.as_deref(), Some("provider-message"));
        assert_eq!(elapsed_ms, 23);
        assert_eq!(status, ConnectorTestStatus::Failed);
        assert_eq!(completion.status, DeliveryStatus::Failed);
        assert_eq!(
            completion.error_code.as_deref(),
            Some("provider_not_delivered")
        );
        assert!(error.is_some());
    }

    /// 像阿里云短信：只发审核过的模板，记下发了什么。
    #[derive(Default)]
    struct Approved {
        sent: Mutex<Vec<NotifyMessage>>,
    }

    #[async_trait]
    impl ConnectorAdapter for Approved {
        fn connector_type(&self) -> &'static str {
            "fake"
        }

        fn capabilities(&self) -> ConnectorCapabilities {
            ConnectorCapabilities {
                direct_user: true,
                ..ConnectorCapabilities::default()
            }
        }

        fn channel(&self) -> NotifyChannel {
            NotifyChannel::Sms
        }

        fn template_provider(&self) -> Option<&'static str> {
            Some("aliyun")
        }

        fn validate_config(&self, _config: &Value) -> Result<()> {
            Ok(())
        }

        fn validate_target(&self, _target: &NotifyTarget) -> Result<()> {
            Ok(())
        }

        async fn send(
            &self,
            _config: &Value,
            _target: &NotifyTarget,
            message: &NotifyMessage,
        ) -> Result<ConnectorDeliveryResult> {
            self.sent.lock().unwrap().push(message.clone());

            Ok(ConnectorDeliveryResult {
                provider_message_id: None,
                delivered: true,
                latency_ms: 1,
                metadata: Default::default(),
            })
        }
    }

    struct NoReferences;

    #[async_trait]
    impl NotifyRouteReferenceRepository for NoReferences {
        async fn count_for_connector(&self, _org: &Id, _connector: &Id) -> Result<u64> {
            Ok(0)
        }
    }

    fn template(
        id: &str,
        org: &Id,
        channel: Option<NotifyChannel>,
        aliyun: bool,
    ) -> NotifyTemplate {
        let provider_templates = if aliyun {
            BTreeMap::from([(
                "aliyun".to_string(),
                ProviderTemplate {
                    code: "SMS_1".into(),
                    params: BTreeMap::from([("name".to_string(), "{{rule.name}}".to_string())]),
                    max_param_chars: None,
                },
            )])
        } else {
            BTreeMap::new()
        };

        NotifyTemplate {
            id: Id::from_string(id),
            organization_id: org.clone(),
            category: NotifyCategory::Alert,
            channel,
            body: "[{{severity}}] {{rule.name}}".into(),
            format: "text".into(),
            provider_templates,
            card: None,
        }
    }

    fn message() -> NotifyMessage {
        NotifyMessage {
            title: "MoleSignal notify connector test".into(),
            text: "Your MoleSignal notify connector is configured correctly.".into(),
            markdown: None,
            html: None,
            metadata: Default::default(),
            provider_templates: Default::default(),
            event: None,
            card: None,
        }
    }

    fn phone() -> NotifyTarget {
        NotifyTarget {
            target_type: NotifyTargetType::FixedAddress,
            value: "13800138000".into(),
            metadata: Default::default(),
        }
    }

    #[tokio::test]
    async fn a_connector_of_approved_templates_is_tested_with_a_template_of_its_channel() {
        let store = Arc::new(MemoryStore::default());
        let adapter = Arc::new(Approved::default());
        let org = Id::from_string("org-a");

        NotifyConnectorRepository::create(store.as_ref(), connector(&org))
            .await
            .unwrap();

        let registry =
            ConnectorRegistry::new([adapter.clone() as Arc<dyn ConnectorAdapter>]).unwrap();

        let service = NotifyService::new(
            store.clone(),
            store.clone(),
            store.clone(),
            store.clone(),
            Arc::new(NoReferences),
            store.clone(),
            Arc::new(registry),
        );

        let connector_id = Id::from_string("connector-a");

        // 还没有写了阿里云模板的短信模板：发不出去。
        assert!(
            service
                .test_connector(&org, &connector_id, phone(), message(), None)
                .await
                .is_err()
        );

        store.add_template(template("template-a", &org, None, false));
        store.add_template(template("template-b", &org, Some(NotifyChannel::Sms), true));
        store.add_template(template("template-c", &org, Some(NotifyChannel::Im), false));

        // 没选模板：用这个渠道里写了阿里云模板的那个，占位符填示例值。
        let outcome = service
            .test_connector(&org, &connector_id, phone(), message(), None)
            .await
            .unwrap();

        assert!(outcome.sent);

        let sent = adapter.sent.lock().unwrap().last().cloned().unwrap();
        assert_eq!(sent.text, "[critical] API unavailable");
        assert_eq!(
            sent.provider_templates["aliyun"].params["name"],
            "API unavailable"
        );

        // 别的渠道的模板不能拿来测。
        assert!(
            service
                .test_connector(
                    &org,
                    &connector_id,
                    phone(),
                    message(),
                    Some(&Id::from_string("template-c"))
                )
                .await
                .is_err()
        );

        // 直接发送不走这种连接器。
        assert!(
            service
                .validate_direct_target(&org, &connector_id, &phone())
                .await
                .is_err()
        );
    }
}
