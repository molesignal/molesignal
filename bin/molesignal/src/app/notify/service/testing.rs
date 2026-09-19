// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::time::Instant;

use super::{ConnectorTestOutcome, NotifyService};
use crate::{
    app::notify::config::{mask_target, truncate_error},
    domain::notify::{
        connector::{
            ConnectorTestStatus, NotifyConnector, NotifyMessage, NotifyTarget, NotifyTargetType,
        },
        delivery::{DeliveryCompletion, DeliveryStage, DeliveryStatus, NotifyDelivery},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

impl NotifyService {
    pub async fn test_connector(
        &self,
        organization_id: &Id,
        id: &Id,
        target: NotifyTarget,
        message: NotifyMessage,
    ) -> Result<ConnectorTestOutcome> {
        let connector = self.connectors.get(organization_id, id).await?;
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
        let adapter = self.registry.get(&connector.connector_type)?;
        adapter.validate_config(&connector.config)?;
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
        let result = adapter.send(&connector.config, &target, &message).await;
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

fn completion_from_result(
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
    use super::*;
    use crate::domain::notify::connector::ConnectorDeliveryResult;

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
}
