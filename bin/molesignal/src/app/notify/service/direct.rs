// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 不经通知策略的直接发送：功能自己已经选定了连接器和固定目标（例如定时巡检的通知渠道），
//! 这里只负责发出去，并和别的通知一样在投递记录里留一条。

use std::time::Instant;

use super::{NotifyService, testing::completion_from_result};
use crate::{
    app::notify::config::{mask_target, truncate_error},
    domain::notify::{
        connector::{NotifyConnector, NotifyMessage, NotifyTarget, NotifyTargetType},
        delivery::{DeliveryStage, DeliveryStatus, NotifyDelivery},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 一次直接发送。
#[derive(Debug, Clone)]
pub struct DirectSend {
    pub connector_id: Id,
    pub target: NotifyTarget,
    pub message: NotifyMessage,
    /// 投递记录里的事件标识，说明这条通知是为哪件事发的，例如 `log_inspection.run:{id}`。
    pub event_id: String,
    /// 同一件事对同一个目标只发一次：重复调用时不会再发。
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectSendOutcome {
    /// 打过码的目标地址，与投递记录里的一致。
    pub target_masked: String,
    pub delivered: bool,
    pub error: Option<String>,
}

impl NotifyService {
    /// 直接发送能用的目标：连接器在这个组织里，目标是固定地址或固定群组，且连接器接受它。
    /// 发给个人要经过通知策略和个人的接收端点，不走这条路。
    pub async fn validate_direct_target(
        &self,
        organization_id: &Id,
        connector_id: &Id,
        target: &NotifyTarget,
    ) -> Result<NotifyConnector> {
        if !matches!(
            target.target_type,
            NotifyTargetType::FixedAddress | NotifyTargetType::FixedGroup
        ) {
            return Err(Error::invalid(
                "a direct notify target must be a fixed_address or a fixed_group",
            ));
        }

        let connector = self.connectors.get(organization_id, connector_id).await?;

        self.registry
            .get(&connector.connector_type)?
            .validate_target(target)?;

        Ok(connector)
    }

    /// 向一个连接器上的固定目标发一条消息，并写入投递记录（阶段 `direct`）。
    ///
    /// 结果总是一条 [`DirectSendOutcome`]：连接器不存在、已停用、不接受这个目标，或者发的
    /// 过程出了错，都是 `delivered: false` 加原因。调用方的事情已经做完，通知发不出去只需要
    /// 被记下来。
    pub async fn send_direct(&self, organization_id: &Id, send: DirectSend) -> DirectSendOutcome {
        let target_masked = mask_target(&send.target.value);

        match self
            .deliver_direct(organization_id, send, &target_masked)
            .await
        {
            Ok((delivered, error)) => DirectSendOutcome {
                target_masked,
                delivered,
                error,
            },
            Err(error) => DirectSendOutcome {
                target_masked,
                delivered: false,
                error: Some(truncate_error(&error.to_string())),
            },
        }
    }

    /// 发送并落投递记录，返回是否送达和没送达的原因。连接器这一侧没到发送那一步的问题以
    /// 错误返回，此时投递记录里没有这一条。
    async fn deliver_direct(
        &self,
        organization_id: &Id,
        send: DirectSend,
        target_masked: &str,
    ) -> Result<(bool, Option<String>)> {
        let connector = self
            .validate_direct_target(organization_id, &send.connector_id, &send.target)
            .await?;

        if !connector.enabled {
            return Err(Error::invalid("notify connector is disabled"));
        }

        let adapter = self.registry.get(&connector.connector_type)?;
        adapter.validate_config(&connector.config)?;
        let sent_at = TimestampMicros::now();

        let claim = self
            .deliveries
            .claim(NotifyDelivery {
                id: Id::new(),
                organization_id: connector.organization_id.clone(),
                event_id: send.event_id,
                policy_id: None,
                recipient_user_id: None,
                connector_id: Some(connector.id.clone()),
                endpoint_id: None,
                target_type: send.target.target_type.as_str().into(),
                target_value_masked: Some(target_masked.to_string()),
                stage: DeliveryStage::Direct,
                attempt: 1,
                status: DeliveryStatus::Sending,
                error_code: None,
                error_message: None,
                latency_ms: None,
                sent_at: Some(sent_at),
                delivered_at: None,
                acknowledged_at: None,
                escalated_at: None,
                idempotency_key: send.idempotency_key,
                created_at: sent_at,
            })
            .await?;

        // 这件事已经对这个目标发过，或正在发：不再发第二遍。
        if !claim.acquired {
            let delivered = matches!(
                claim.delivery.status,
                DeliveryStatus::Success | DeliveryStatus::Acknowledged
            );

            return Ok((delivered, claim.delivery.error_message));
        }

        let started = Instant::now();

        let result = adapter
            .send(&connector.config, &send.target, &send.message)
            .await;

        let (delivered, _, _, error, _, completion) =
            completion_from_result(result, started, TimestampMicros::now());

        self.deliveries
            .complete(&connector.organization_id, &claim.delivery.id, completion)
            .await?;

        Ok((delivered, error))
    }
}

#[cfg(test)]
mod tests;
