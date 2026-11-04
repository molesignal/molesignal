// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警通知带上按钮，点了以后按告警现在的样子重写它。

use std::borrow::Cow;

use super::{NotifyEngine, model::ResolvedRoute, pipelines::PolicyCopy};
use crate::{
    app::notify::alert_dispatch,
    domain::{
        alerting::incident::Incident,
        notify::{
            buttons::{AlertAction, AlertButtons},
            call::QueuedCall,
            connector::{
                NotifyChannel, NotifyConnector, NotifyMessage, NotifyTarget, NotifyTargetType,
            },
            policy::{NotifyEvent, NotifyPolicy},
            recipient::NotifyRecipient,
        },
    },
    shared::ids::Id,
};

impl NotifyEngine {
    /// 合成一条的通知上的「全部确认」：确认里面的告警，字用收通知的人的语言（发到群里时用工作区的）。
    pub(super) async fn with_bundle_buttons(
        &self,
        connector: &NotifyConnector,
        target: &NotifyTarget,
        calls: &[QueuedCall],
        message: &mut NotifyMessage,
    ) {
        let (Some(source), Some(first)) = (self.alert_buttons.source(), calls.first()) else {
            return;
        };

        let alert_ids: Vec<String> = calls
            .iter()
            .filter_map(|call| call.message.metadata.get("alert_id").cloned())
            .collect();

        if alert_ids.is_empty() {
            return;
        }

        let delivery = self
            .deliveries
            .get(&first.organization_id, &first.delivery_id)
            .await
            .ok();

        let recipient = delivery
            .as_ref()
            .and_then(|delivery| delivery.recipient_user_id.clone())
            .filter(|_| target.target_type == NotifyTargetType::DirectUser);

        let Some(labels) = source.labels(connector, recipient.as_ref()).await else {
            return;
        };

        let event_type = first
            .message
            .metadata
            .get("event_type")
            .cloned()
            .unwrap_or_default();

        let policy_id = delivery
            .and_then(|delivery| delivery.policy_id)
            .map(|id| id.to_string());

        AlertButtons::bundle(&alert_ids, &event_type, policy_id.as_deref(), &labels).write(message);
    }

    /// 告警通知发到收得到点击的 IM 时带上按钮：还开着的能确认、解决，确认了的能解决。私聊发给一个
    /// 人时按钮的字用他的语言。
    pub(super) async fn with_alert_buttons<'a>(
        &self,
        policy: &NotifyPolicy,
        event: &NotifyEvent,
        recipient: &NotifyRecipient,
        route: &ResolvedRoute,
        message: &'a NotifyMessage,
    ) -> Cow<'a, NotifyMessage> {
        let Some(source) = self.alert_buttons.source() else {
            return Cow::Borrowed(message);
        };

        let (Some(alert_id), Some(status)) = (
            event.attributes["alert_id"].as_str(),
            event.attributes["status"].as_str(),
        ) else {
            return Cow::Borrowed(message);
        };

        if !event.event_type.starts_with("alert.") || AlertAction::available(status).is_empty() {
            return Cow::Borrowed(message);
        }

        let direct = route.target.target_type == NotifyTargetType::DirectUser;

        let Some(labels) = source
            .labels(&route.connector, direct.then_some(&recipient.user_id))
            .await
        else {
            return Cow::Borrowed(message);
        };

        let mut message = message.clone();

        AlertButtons::new(
            alert_id,
            status,
            &event.event_type,
            Some(policy.id.as_str()),
            &labels,
        )
        .write(&mut message);

        Cow::Owned(message)
    }

    /// 按告警现在的样子重写一条发出去的告警通知：点了卡片上的按钮以后换上。发出它的通知策略给 IM
    /// 用的模板照旧套用；策略没了或模板写不出来时用默认的写法。
    pub async fn rewrite_alert(
        &self,
        incident: &Incident,
        event_type: &str,
        policy_id: Option<&Id>,
    ) -> NotifyMessage {
        let mut dispatch = alert_dispatch(incident, event_type);

        let Some(policy_id) = policy_id else {
            return dispatch.message;
        };

        self.complete_alert_event(&mut dispatch.event).await;

        let policy = match self.policies.get(&incident.org_id, policy_id).await {
            Ok(policy) => policy,
            Err(error) => {
                tracing::debug!(%error, "the notify policy of an alert card is gone");
                return dispatch.message;
            }
        };

        // 卡片说的和发出去时一样，是流水线留下的那一份。
        if let PolicyCopy::Changed(copy) = self
            .policy_copy(&policy, &dispatch.event, &dispatch.message)
            .await
        {
            (dispatch.event, dispatch.message) = *copy;
        }

        // 卡片在 IM 里：用策略给 IM 选的模板。
        match self
            .policy_messages(&policy, &dispatch.event, &dispatch.message)
            .await
            .and_then(|messages| messages.for_channel(NotifyChannel::Im, &dispatch.event))
        {
            Ok(message) => message,
            Err(error) => {
                tracing::warn!(
                    %error,
                    policy_id = %policy.id,
                    "an alert card could not be rewritten with its template"
                );
                dispatch.message
            }
        }
    }
}
