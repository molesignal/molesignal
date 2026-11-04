// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 合并通知：一条线路（一个连接器发往一个地址：一个号码、一个人的飞书、一个群）一个窗口只发一次。
//! 窗口里再来的通知排在线路上，窗口结束时合成一条发出。
//!
//! - 打电话的连接器自己就要合并（[`ConnectorAdapter::merge_window_secs`]，服务商限频）：各条通知的
//!   标题依次念出，正文不念，免得太长。短信也这样合并。
//! - 告警类通知策略可以设合并窗口（`NotifyPolicy::merge_window_secs`），取它与连接器的较大者。即时
//!   通讯与邮件合成的一条：标题是第一条的标题加「(+N)」，正文逐条列出，能链到事件的写成链接。
//! - Webhook 不合并：机器要一个事件一个请求。
//!
//! [`ConnectorAdapter::merge_window_secs`]: crate::domain::notify::connector::ConnectorAdapter::merge_window_secs

use std::time::Instant;

use super::NotifyEngine;
use crate::{
    app::notify::config::truncate_error,
    domain::notify::{
        call::{CallLine, QueuedCall},
        connector::{
            ConnectorAdapter, NotifyChannel, NotifyConnector, NotifyMessage, NotifyTarget,
        },
        delivery::{DeliveryCompletion, DeliveryStatus, NotifyDelivery},
        policy::{NotifyEvent, NotifyPolicy},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 这条通知在线路上合并的窗口：连接器自己的（电话），与告警类策略设的取较大者；Webhook 不合并。
pub(super) fn merge_window(
    adapter: &dyn ConnectorAdapter,
    policy: &NotifyPolicy,
    event: &NotifyEvent,
) -> Option<u32> {
    let chosen = policy.merge_window_secs.filter(|_| {
        event.event_type.starts_with("alert.") && adapter.channel() != NotifyChannel::Webhook
    });

    adapter.merge_window_secs().max(chosen)
}

/// 同一个连接器拨同一个号码的线路。线路只记号码的摘要。
fn line_of(connector: &NotifyConnector, target: &NotifyTarget, window_secs: u32) -> CallLine {
    let number = format!("{}|{}", target.target_type.as_str(), target.value.trim());

    CallLine {
        organization_id: connector.organization_id.clone(),
        connector_id: connector.id.clone(),
        key: blake3::hash(number.as_bytes()).to_hex().to_string(),
        window_secs,
    }
}

/// 合成的一条：电话与短信依次念、依次写各条的标题（[`spoken`]），其余渠道列成一张清单
/// （[`listed`]）。只有一条时就是它本身。
pub(super) fn merged_message(
    calls: &[QueuedCall],
    channel: NotifyChannel,
    external_url: &str,
) -> NotifyMessage {
    if let [call] = calls {
        return call.message.clone();
    }

    match channel {
        NotifyChannel::Phone | NotifyChannel::Sms => spoken(calls),
        _ => listed(calls, external_url),
    }
}

/// 列成清单的一条：标题是第一条的标题加「(+N)」，正文逐条一行；有对外地址时 Markdown 里每条链到它的
/// 事件。卡片不带。
fn listed(calls: &[QueuedCall], external_url: &str) -> NotifyMessage {
    let base = external_url.trim().trim_end_matches('/');
    let first = calls.first().map(|call| &call.message);

    let lines: Vec<(String, Option<String>)> = calls
        .iter()
        .map(|call| {
            let link = call
                .message
                .metadata
                .get("alert_id")
                .filter(|_| !base.is_empty())
                .map(|alert_id| format!("{base}/alerts/incidents/{alert_id}"));

            (headline(&call.message).to_string(), link)
        })
        .collect();

    let markdown = lines
        .iter()
        .map(|(line, link)| match link {
            Some(link) => format!("- [{line}]({link})"),
            None => format!("- {line}"),
        })
        .collect::<Vec<_>>()
        .join("\n");

    NotifyMessage {
        title: format!(
            "{} (+{})",
            first.map(headline).unwrap_or_default(),
            calls.len() - 1
        ),
        text: lines
            .iter()
            .map(|(line, _)| line.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        markdown: Some(markdown),
        html: None,
        metadata: first
            .map(|message| message.metadata.clone())
            .unwrap_or_default(),
        provider_templates: Default::default(),
        event: None,
        card: None,
    }
}

/// 电话与短信合成的一条：依次念、依次写各条的标题；电话模板写的通知没有标题，念它的正文。写给服务商
/// 的模板用第一条的，每个变量依次填用同一个模板的各条的值。
fn spoken(calls: &[QueuedCall]) -> NotifyMessage {
    let mut provider_templates = calls
        .first()
        .map(|call| call.message.provider_templates.clone())
        .unwrap_or_default();

    for (provider, merged) in &mut provider_templates {
        let code = merged.code.clone();

        for (name, value) in &mut merged.params {
            let mut values = Vec::new();

            for filled in calls
                .iter()
                .filter_map(|call| call.message.provider_templates.get(provider))
                .filter(|template| template.code == code)
                .filter_map(|template| template.params.get(name))
                .map(|value| value.trim())
            {
                if !filled.is_empty() && !values.contains(&filled) {
                    values.push(filled);
                }
            }

            *value = values.join("; ");
        }
    }

    NotifyMessage {
        title: calls
            .iter()
            .map(|call| headline(&call.message))
            .filter(|title| !title.is_empty())
            .collect::<Vec<_>>()
            .join("; "),
        text: String::new(),
        markdown: None,
        html: None,
        metadata: calls
            .first()
            .map(|call| call.message.metadata.clone())
            .unwrap_or_default(),
        provider_templates,
        event: None,
        card: None,
    }
}

/// 一条通知在合成的一条里那一句：标题，没有标题时是正文。
fn headline(message: &NotifyMessage) -> &str {
    match message.title.trim() {
        "" => message.text.trim(),
        title => title,
    }
}

impl NotifyEngine {
    /// 这个号码在窗口里拨过：把通知排在线路上，投递记为排队并返回它。线路空着就占下、返回 `None`，
    /// 照常立即拨打。线路读写出错时也照常拨打：宁可多打一通，不能漏打。
    pub(super) async fn queue_if_line_busy(
        &self,
        connector: &NotifyConnector,
        target: &NotifyTarget,
        window_secs: u32,
        message: &NotifyMessage,
        delivery: &NotifyDelivery,
    ) -> Option<NotifyDelivery> {
        let line = line_of(connector, target, window_secs);
        let now = TimestampMicros::now();

        match self.calls.take_line(&line, now).await {
            Ok(Some(_)) => return None,
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(error = %error, "notify call line could not be read; calling now");
                return None;
            }
        }

        let queued = QueuedCall {
            organization_id: delivery.organization_id.clone(),
            delivery_id: delivery.id.clone(),
            connector_id: connector.id.clone(),
            line_key: line.key,
            target: target.clone(),
            message: message.clone(),
            created_at: now,
        };

        if let Err(error) = self.calls.enqueue(queued).await {
            tracing::warn!(error = %error, "notify call could not be queued; calling now");
            return None;
        }

        let completion = DeliveryCompletion {
            status: DeliveryStatus::Queued,
            error_code: None,
            error_message: None,
            latency_ms: None,
            delivered_at: None,
        };

        // 已经排上了：记不下排队也不再立即拨，合并的那一通拨出后会完成这次投递。
        match self
            .deliveries
            .complete(&delivery.organization_id, &delivery.id, completion)
            .await
        {
            Ok(queued) => Some(queued),
            Err(error) => {
                tracing::warn!(error = %error, "queued notify call could not be recorded");
                Some(delivery.clone())
            }
        }
    }

    /// 拨出窗口已过的线路上排着的通知，一条线路一通电话。返回拨出的通数。
    pub async fn process_merged_calls(
        &self,
        organization_id: &Id,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<u32> {
        let lines = self.calls.due_lines(organization_id, now, limit).await?;
        let mut placed = 0;

        for line in lines {
            match self.call_line(&line, now).await {
                Ok(true) => placed += 1,
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(
                        org_id = %organization_id,
                        connector_id = %line.connector_id,
                        error = %error,
                        "merged notify call failed"
                    );
                }
            }
        }

        Ok(placed)
    }

    /// 占下线路，拨出排着的通知。别的节点先占下了，或排着的都已被确认时返回 `false`。
    async fn call_line(&self, line: &CallLine, now: TimestampMicros) -> Result<bool> {
        let Some(previous) = self.calls.take_line(line, now).await? else {
            return Ok(false);
        };

        let mut calls = Vec::new();

        for call in self.calls.drain(line).await? {
            // 排队期间被确认的不再拨。
            match self
                .deliveries
                .get(&call.organization_id, &call.delivery_id)
                .await
            {
                Ok(delivery) if delivery.status == DeliveryStatus::Queued => calls.push(call),
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(error = %error, "queued notify call has no delivery");
                }
            }
        }

        let Some(first) = calls.first() else {
            self.calls.release_line(line, now, previous).await?;
            return Ok(false);
        };

        let target = first.target.clone();
        let started = Instant::now();
        let result = self.send_merged(line, &target, &calls).await;
        let latency_ms = i32::try_from(started.elapsed().as_millis()).unwrap_or(i32::MAX);

        let completion = match result {
            Ok(true) => DeliveryCompletion {
                status: DeliveryStatus::Success,
                error_code: None,
                error_message: None,
                latency_ms: Some(latency_ms),
                delivered_at: Some(TimestampMicros::now()),
            },
            Ok(false) => DeliveryCompletion {
                status: DeliveryStatus::Failed,
                error_code: Some("provider_not_delivered".into()),
                error_message: Some("notify provider did not confirm delivery".into()),
                latency_ms: Some(latency_ms),
                delivered_at: None,
            },
            Err(error) => DeliveryCompletion {
                status: DeliveryStatus::Failed,
                error_code: Some("connector_send_failed".into()),
                error_message: Some(truncate_error(&error.to_string())),
                latency_ms: Some(latency_ms),
                delivered_at: None,
            },
        };

        for call in &calls {
            if let Err(error) = self
                .deliveries
                .complete(&call.organization_id, &call.delivery_id, completion.clone())
                .await
            {
                tracing::warn!(error = %error, "merged notify call could not be recorded");
            }
        }

        Ok(true)
    }

    /// 合成的一条私聊发给谁：第一条排着的投递的接收人；发到群或固定号码时为空。
    async fn recipient_of(&self, calls: &[QueuedCall], target: &NotifyTarget) -> Option<Id> {
        let first = calls.first()?;

        if target.target_type != crate::domain::notify::connector::NotifyTargetType::DirectUser {
            return None;
        }

        self.deliveries
            .get(&first.organization_id, &first.delivery_id)
            .await
            .ok()?
            .recipient_user_id
    }

    /// 用线路的连接器把几条通知合成一条发出，返回服务商是否受理。
    async fn send_merged(
        &self,
        line: &CallLine,
        target: &NotifyTarget,
        calls: &[QueuedCall],
    ) -> Result<bool> {
        let connector = self
            .connectors
            .get(&line.organization_id, &line.connector_id)
            .await?;

        if !connector.enabled {
            return Err(Error::invalid("notify connector is disabled"));
        }

        let adapter = self.connector_registry.get(&connector.connector_type)?;
        let mut message = merged_message(calls, adapter.channel(), &self.external_url);

        // 即时通讯里合成的一条带「全部确认」；短信、电话合成的一条收得到回复时，回复处理里面全部的告警。
        if calls.len() > 1 && adapter.channel() == NotifyChannel::Im {
            self.with_bundle_buttons(&connector, target, calls, &mut message)
                .await;
        }

        if matches!(adapter.channel(), NotifyChannel::Sms | NotifyChannel::Phone) {
            let recipient = self.recipient_of(calls, target).await;

            let alert_ids = calls
                .iter()
                .filter(|call| {
                    call.message
                        .metadata
                        .get("event_type")
                        .is_some_and(|kind| kind == "alert.triggered" || kind == "alert.escalated")
                })
                .filter_map(|call| call.message.metadata.get("alert_id"))
                .map(|id| Id::from_string(id.clone()))
                .collect();

            message = self
                .with_reply(
                    &connector,
                    adapter.as_ref(),
                    target,
                    recipient.as_ref(),
                    alert_ids,
                    message,
                )
                .await;
        }

        let result = adapter.send_as(&connector, target, &message).await?;

        Ok(result.delivered)
    }
}
