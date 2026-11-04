// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警卡片上的按钮。开了「在 IM 里回答」的飞书 App、Slack App 发出的告警通知带着确认、解决与
//! 「在 MoleSignal 中查看」（[`ImAlertButtons`]）。点了以后对上点的人，按他在 MoleSignal 里的权限
//! （`alerts.incidents.acknowledge`，与在 MoleSignal 里确认、解决一样）确认或解决告警，换上按告警现在的样子
//! 重写的卡片，再告诉他做成了没有（[`act`]）。
//!
//! 只认已验证的通知身份：对不上人时私下发绑定链接，对上多个人时不做。

use std::sync::Arc;

use async_trait::async_trait;

use super::{
    inbound::{Identity, Place, identify, principal},
    links::{self, Offer},
    texts::{TextSources, Texts},
};
use crate::{
    agent::im::{
        AlertCard, CardAction, CardAnswer, ImBot, ImPlatform, ImRepository, Inbound, NoticeKind,
    },
    api::{
        AppState,
        http::{
            middleware::authorize_resource, routes::agent::chat::run::identity::resolve_principal,
        },
    },
    app::{
        alerting::Actor,
        iam::IamContext,
        notify::{AlertButtonSource, ReplyHints},
    },
    domain::{
        alerting::{
            incident::{Incident, IncidentStatus},
            timeline::ActionVia,
        },
        notify::{
            buttons::{
                AlertAction, AlertButtonLabels, AlertButtons, MAX_BUNDLE_ALERTS, SNOOZE_SECS,
            },
            connector::NotifyConnector,
            reply::ReplyAction,
        },
    },
    infra::{
        im,
        notify::adapters::{WECOM_APP_CONNECTOR_TYPE, WecomCallback},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 告警通知上的按钮：机器人开着才画，字用收通知的人的语言（发到群里时用工作区的）。
pub(crate) struct ImAlertButtons {
    bots: Arc<dyn ImRepository>,
    texts: TextSources,
    external_url: String,
}

impl ImAlertButtons {
    pub(crate) fn new(
        bots: Arc<dyn ImRepository>,
        texts: TextSources,
        external_url: String,
    ) -> Self {
        Self {
            bots,
            texts,
            external_url,
        }
    }
}

#[async_trait]
impl AlertButtonSource for ImAlertButtons {
    async fn reply_hints(
        &self,
        organization_id: &Id,
        recipient: Option<&Id>,
        ack: &str,
        resolve: &str,
    ) -> Option<ReplyHints> {
        let texts = Texts::from(&self.texts, organization_id, recipient).await;

        Some(ReplyHints {
            sms: texts.get("alert.reply_sms", &[("ack", ack), ("resolve", resolve)]),
            voice: texts.get("alert.reply_voice", &[]),
        })
    }

    async fn labels(
        &self,
        connector: &NotifyConnector,
        recipient: Option<&Id>,
    ) -> Option<AlertButtonLabels> {
        // 企业微信自建应用不是 Mole Agent 的机器人：配了「接收消息」的回调，点击就回得来。
        if connector.connector_type == WECOM_APP_CONNECTOR_TYPE {
            let takes = !self.external_url.trim().is_empty()
                && matches!(WecomCallback::of(&connector.config), Ok(Some(_)));

            if !takes {
                return None;
            }

            let texts = Texts::from(&self.texts, &connector.organization_id, recipient).await;
            return Some(labels(&texts, &self.external_url));
        }

        let platform = ImPlatform::of_connector(&connector.connector_type)?;

        if !im::takes_card_actions(platform, &connector.config) {
            return None;
        }

        let bot = self
            .bots
            .get_bot(&connector.organization_id, &connector.id)
            .await
            .ok()
            .flatten()?;

        if !bot.enabled {
            return None;
        }

        let texts = Texts::from(&self.texts, &connector.organization_id, recipient).await;
        Some(labels(&texts, &self.external_url))
    }
}

fn labels(texts: &Texts, external_url: &str) -> AlertButtonLabels {
    let base = external_url.trim();

    AlertButtonLabels {
        acknowledge: texts.get("alert.acknowledge", &[]),
        resolve: texts.get("alert.resolve", &[]),
        snooze: texts.get("alert.snooze", &[]),
        acknowledge_all: texts.get("alert.acknowledge_all", &[]),
        open: (!base.is_empty()).then(|| (texts.get("open", &[]), base.to_string())),
    }
}

fn notice(kind: NoticeKind, text: String) -> CardAnswer {
    CardAnswer {
        notice: Some((kind, text)),
        card: None,
    }
}

/// 点了告警卡片上的一个按钮。做不成时告诉点的人为什么。
pub(crate) async fn act(
    state: &AppState,
    bot: &ImBot,
    connector: &NotifyConnector,
    action: &CardAction,
) -> CardAnswer {
    match settle(state, bot, connector, action).await {
        Ok(answer) => answer,
        Err(error) => {
            tracing::warn!(connector = %bot.connector_id.0, %error, "an alert card action failed");

            let texts = Texts::for_user(state, &bot.org_id, None).await;
            notice(
                NoticeKind::Error,
                texts.get("alert.failed", &[("reason", &error.to_string())]),
            )
        }
    }
}

async fn settle(
    state: &AppState,
    bot: &ImBot,
    connector: &NotifyConnector,
    action: &CardAction,
) -> Result<CardAnswer> {
    let Some(platform) = ImPlatform::of_connector(&connector.connector_type) else {
        return Ok(CardAnswer {
            notice: None,
            card: None,
        });
    };

    let user_id = match identify(state, bot, &action.sender).await? {
        Identity::User(user_id) => user_id,
        Identity::Unknown => return Ok(offer_link(state, bot, connector, platform, action).await),
        Identity::Ambiguous => {
            let texts = Texts::for_user(state, &bot.org_id, None).await;
            let platform = texts.platform(platform);

            return Ok(notice(
                NoticeKind::Warning,
                texts.get("alert.ambiguous", &[("platform", &platform)]),
            ));
        }
    };

    carry_out(state, &bot.org_id, &user_id, action).await
}

/// 认出点按钮的人以后：以他的身份确认、解决、暂缓或全部确认，按告警现在的样子换卡片。
pub(crate) async fn carry_out(
    state: &AppState,
    org_id: &Id,
    user_id: &Id,
    action: &CardAction,
) -> Result<CardAnswer> {
    let texts = Texts::for_user(state, org_id, Some(user_id)).await;

    let ctx = match resolve_principal(state, org_id, &principal(user_id)).await {
        Ok(ctx) => ctx,
        Err(Error::Forbidden(_) | Error::NotFound(_)) => {
            return Ok(notice(
                NoticeKind::Warning,
                texts.get("alert.no_access", &[]),
            ));
        }
        Err(error) => return Err(error),
    };

    if action.value.action == AlertAction::AcknowledgeAll {
        return acknowledge_all(state, org_id, &ctx, &texts, user_id, action).await;
    }

    let incident = match authorize_resource::<Incident>(
        state,
        &ctx,
        Id::from_string(action.value.alert_id.clone()),
        "alerts.incidents.acknowledge",
    )
    .await
    {
        Ok(incident) if incident.org_id == *org_id => incident,
        Ok(_) | Err(Error::NotFound(_)) => {
            return Ok(notice(
                NoticeKind::Warning,
                texts.get("alert.not_found", &[]),
            ));
        }
        Err(Error::Forbidden(_)) => {
            return Ok(notice(
                NoticeKind::Warning,
                texts.get("alert.no_access", &[]),
            ));
        }
        Err(error) => return Err(error),
    };

    let before = incident.status;
    let service = &state.alerting.service;
    let now = TimestampMicros::now();

    let done = match action.value.action {
        AlertAction::Acknowledge => {
            service
                .acknowledge(&incident.id, user_id.clone(), now, ActionVia::Im)
                .await
        }
        AlertAction::Resolve => {
            service
                .resolve(&incident.id, user_id.clone(), now, ActionVia::Im)
                .await
        }
        AlertAction::Snooze => {
            let actor = Actor::new(org_id, user_id, ActionVia::Im);

            state
                .alerting
                .actions
                .snooze(&actor, &incident.id, SNOOZE_SECS)
                .await
        }
        // 前面已经处理了。
        AlertAction::AcknowledgeAll => Ok(incident.clone()),
    };

    // 已经解决了的不能再确认：按库里现在的样子换卡片。
    let incident = match done {
        Ok(incident) => incident,
        Err(Error::Conflict(_)) => service.get_incident(&incident.id).await?,
        Err(error) => return Err(error),
    };

    let (kind, said) = outcome(action.value.action, before);

    Ok(CardAnswer {
        notice: Some((kind, texts.get(said, &[]))),
        card: Some(card(state, &texts, &incident, action).await),
    })
}

/// 合成一条的通知上点了「全部确认」：逐个确认里面还没确认的告警，点的人没有权限的跳过。卡片不换，
/// 回一句确认了几个。
async fn acknowledge_all(
    state: &AppState,
    org_id: &Id,
    ctx: &IamContext,
    texts: &Texts,
    user_id: &Id,
    action: &CardAction,
) -> Result<CardAnswer> {
    let now = TimestampMicros::now();
    let mut acknowledged = 0usize;

    for alert_id in action.value.alert_ids.iter().take(MAX_BUNDLE_ALERTS) {
        let Ok(incident) = authorize_resource::<Incident>(
            state,
            ctx,
            Id::from_string(alert_id.clone()),
            "alerts.incidents.acknowledge",
        )
        .await
        else {
            continue;
        };

        if incident.org_id != *org_id || incident.status != IncidentStatus::Open {
            continue;
        }

        if state
            .alerting
            .service
            .acknowledge(&incident.id, user_id.clone(), now, ActionVia::Im)
            .await
            .is_ok()
        {
            acknowledged += 1;
        }
    }

    let count = acknowledged.to_string();

    Ok(notice(
        NoticeKind::Success,
        texts.get("alert.done_acknowledge_all", &[("count", &count)]),
    ))
}

/// 电话上按键之后念的那句，用接收人的语言：确认了、解决了，或者没有可以处理的告警。
pub(crate) async fn keypress_answer(
    state: &AppState,
    org_id: &Id,
    user_id: Option<&Id>,
    done: Option<(ReplyAction, usize)>,
) -> String {
    let texts = Texts::for_user(state, org_id, user_id).await;

    let key = match done {
        Some((ReplyAction::Acknowledge, count)) if count > 0 => "alert.phone_acknowledged",
        Some((ReplyAction::Resolve, count)) if count > 0 => "alert.phone_resolved",
        _ => "alert.phone_nothing",
    };

    texts.get(key, &[])
}

/// 点了以后给点的人的那句话（`agent.im.` 下的键）：做成了，或者告警早就是这样了。
fn outcome(action: AlertAction, before: IncidentStatus) -> (NoticeKind, &'static str) {
    match (action, before) {
        (AlertAction::Snooze, IncidentStatus::Open | IncidentStatus::Acknowledged) => {
            (NoticeKind::Success, "alert.done_snooze")
        }
        (AlertAction::AcknowledgeAll, IncidentStatus::Open | IncidentStatus::Acknowledged) => {
            (NoticeKind::Success, "alert.done_acknowledge")
        }
        (AlertAction::Acknowledge, IncidentStatus::Open) => {
            (NoticeKind::Success, "alert.done_acknowledge")
        }
        (AlertAction::Resolve, IncidentStatus::Open | IncidentStatus::Acknowledged) => {
            (NoticeKind::Success, "alert.done_resolve")
        }
        (AlertAction::Acknowledge, IncidentStatus::Acknowledged) => {
            (NoticeKind::Info, "alert.already_acknowledged")
        }
        (_, IncidentStatus::Resolved | IncidentStatus::Closed) => {
            (NoticeKind::Info, "alert.already_resolved")
        }
    }
}

/// 点的人还没对上 MoleSignal 用户：私下给他绑定链接，告诉他发了。
async fn offer_link(
    state: &AppState,
    bot: &ImBot,
    connector: &NotifyConnector,
    platform: ImPlatform,
    action: &CardAction,
) -> CardAnswer {
    let texts = Texts::for_user(state, &bot.org_id, None).await;

    let inbound = Inbound {
        message_id: String::new(),
        conversation_id: action.conversation_id.clone(),
        kind: action.kind,
        thread_key: String::new(),
        sender: action.sender.clone(),
        text: String::new(),
        addressed: true,
        reply: action.reply.clone(),
    };

    let place = Place {
        state,
        bot,
        connector,
        platform,
        inbound: &inbound,
    };

    match links::give(&place, &texts).await {
        // Slack 的链接本来就只有他看得到。
        Offer::Given if platform == ImPlatform::Slack => CardAnswer {
            notice: None,
            card: None,
        },
        Offer::Given => {
            let platform = texts.platform(platform);

            notice(
                NoticeKind::Info,
                texts.get("link_sent", &[("platform", &platform)]),
            )
        }
        Offer::Refused(text) => notice(NoticeKind::Warning, text),
    }
}

/// 换上的卡片：按告警现在的样子、用发出它的通知策略重写，按钮是告警现在还能做的，加一句谁确认或
/// 解决了它。
async fn card(
    state: &AppState,
    texts: &Texts,
    incident: &Incident,
    action: &CardAction,
) -> AlertCard {
    let value = &action.value;
    let policy_id = value.policy_id.as_deref().map(Id::from_string);

    let message = state
        .alerting
        .notify_engine
        .rewrite_alert(incident, &value.event_type, policy_id.as_ref())
        .await;

    let buttons = AlertButtons::new(
        incident.id.as_str(),
        incident.status.as_str(),
        &value.event_type,
        value.policy_id.as_deref(),
        &labels(texts, &state.platform.external_url),
    );

    AlertCard {
        message,
        buttons,
        note: note(state, texts, incident).await,
    }
}

/// 一句谁确认或解决了告警；没人解决（规则自己恢复）的说已恢复。
async fn note(state: &AppState, texts: &Texts, incident: &Incident) -> Option<String> {
    match incident.status {
        IncidentStatus::Open => None,
        IncidentStatus::Acknowledged => match (
            &incident.handling.snoozed_until,
            &incident.handling.snoozed_by,
        ) {
            (Some(until), Some(user_id)) => {
                let user = name(state, user_id).await;
                let until = readable_time(*until);
                Some(texts.get("alert.snoozed_by", &[("user", &user), ("until", &until)]))
            }
            _ => {
                let user = name(state, incident.acknowledged_by.as_ref()?).await;
                Some(texts.get("alert.acknowledged_by", &[("user", &user)]))
            }
        },
        IncidentStatus::Resolved | IncidentStatus::Closed => match &incident.resolved_by {
            Some(user_id) => {
                let user = name(state, user_id).await;
                Some(texts.get("alert.resolved_by", &[("user", &user)]))
            }
            None => Some(texts.get("alert.recovered", &[])),
        },
    }
}

/// 卡片上写的时刻（UTC），如 2026-10-28 09:30 UTC。
fn readable_time(at: TimestampMicros) -> String {
    use chrono::TimeZone as _;

    chrono::Utc
        .timestamp_micros(at.0)
        .single()
        .map(|time| time.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_default()
}

/// 一个用户在 MoleSignal 里的名字；没有名字时是邮箱。
async fn name(state: &AppState, user_id: &Id) -> String {
    match state.iam.service.users.get(user_id).await {
        Ok(user) if !user.display_name.trim().is_empty() => user.display_name,
        Ok(user) => user.email,
        Err(_) => user_id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_says_whether_it_changed_the_alert() {
        assert_eq!(
            outcome(AlertAction::Acknowledge, IncidentStatus::Open),
            (NoticeKind::Success, "alert.done_acknowledge")
        );
        assert_eq!(
            outcome(AlertAction::Resolve, IncidentStatus::Acknowledged),
            (NoticeKind::Success, "alert.done_resolve")
        );
        assert_eq!(
            outcome(AlertAction::Acknowledge, IncidentStatus::Acknowledged),
            (NoticeKind::Info, "alert.already_acknowledged")
        );
        assert_eq!(
            outcome(AlertAction::Acknowledge, IncidentStatus::Resolved),
            (NoticeKind::Info, "alert.already_resolved")
        );
        assert_eq!(
            outcome(AlertAction::Resolve, IncidentStatus::Closed),
            (NoticeKind::Info, "alert.already_resolved")
        );
    }

    #[test]
    fn the_link_button_needs_an_address_to_open() {
        let texts = Texts::empty();

        assert_eq!(labels(&texts, "  ").open, None);
        assert_eq!(
            labels(&texts, "https://molesignal.example ").open,
            Some((
                "agent.im.open".to_string(),
                "https://molesignal.example".to_string()
            ))
        );
    }
}
