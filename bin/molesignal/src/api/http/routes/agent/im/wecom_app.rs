// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 企业微信自建应用发的告警卡片上按钮的点击：回调的路由（`routes::notify::wecom_callback`）验过签名、
//! 解开以后交到这里。
//!
//! 自建应用不是 Mole Agent 的机器人：按这个连接器上验证过的通知方式认人，认不出时告诉他先把这个企业微信
//! 账号加成通知方式。做完换掉他眼前的那张卡片，没做成的那句话由应用发给他。

use serde_json::json;

use super::{
    alerts::carry_out,
    inbound::{Identity, identify_on},
    texts::Texts,
};
use crate::{
    agent::im::{CardAction, CardAnswer, ConversationKind, ImPlatform, NoticeKind, Sender},
    api::AppState,
    domain::notify::{buttons::AlertButtonValue, connector::NotifyConnector},
    infra::notify::adapters::{
        WecomCardEvent, tell_wecom_member, update_wecom_card, wecom_template_card,
    },
    shared::Result,
};

/// 点了自建应用发的模板卡片上的一个按钮。不是告警按钮（处理完的「✓」）时不理。
pub(crate) async fn press(state: AppState, connector: NotifyConnector, event: WecomCardEvent) {
    let Ok(value) = serde_json::from_str::<AlertButtonValue>(&event.event_key) else {
        return;
    };

    let action = CardAction {
        value,
        sender: Sender {
            ids: vec![event.user.clone()],
            name: None,
        },
        conversation_id: event.user.clone(),
        kind: ConversationKind::Direct,
        reply: json!({}),
        card: json!({ "task_id": event.task_id, "response_code": event.response_code }),
    };

    let answer = match settle(&state, &connector, &action).await {
        Ok(answer) => answer,
        Err(error) => {
            tracing::warn!(connector = %connector.id.0, %error, "a WeCom app card action failed");

            let texts = Texts::for_user(&state, &connector.organization_id, None).await;
            told(
                NoticeKind::Error,
                texts.get("alert.failed", &[("reason", &error.to_string())]),
            )
        }
    };

    if let Err(error) = answer_member(&connector, &event, &answer).await {
        tracing::warn!(connector = %connector.id.0, %error, "a WeCom app card could not be updated");
    }
}

fn told(kind: NoticeKind, text: String) -> CardAnswer {
    CardAnswer {
        notice: Some((kind, text)),
        card: None,
    }
}

async fn settle(
    state: &AppState,
    connector: &NotifyConnector,
    action: &CardAction,
) -> Result<CardAnswer> {
    let org_id = &connector.organization_id;

    match identify_on(state, org_id, &connector.id, &action.sender).await? {
        Identity::User(user_id) => carry_out(state, org_id, &user_id, action).await,
        unknown => {
            let texts = Texts::for_user(state, org_id, None).await;
            let platform = texts.platform(ImPlatform::WeCom);
            let key = match unknown {
                Identity::Ambiguous => "alert.ambiguous",
                _ => "alert.unlinked",
            };

            Ok(told(
                NoticeKind::Warning,
                texts.get(key, &[("platform", &platform)]),
            ))
        }
    }
}

/// 换掉点的人眼前的卡片；没做成，或没有卡片可换时，那句话发给他。
async fn answer_member(
    connector: &NotifyConnector,
    event: &WecomCardEvent,
    answer: &CardAnswer,
) -> Result<()> {
    if let Some(card) = &answer.card {
        let replacement = wecom_template_card(
            &card.message,
            &card.buttons,
            card.note.as_deref(),
            &event.task_id,
        );

        update_wecom_card(
            &connector.config,
            &event.user,
            &event.response_code,
            &replacement,
        )
        .await?;
    }

    if let Some((kind, text)) = &answer.notice
        && (answer.card.is_none() || *kind != NoticeKind::Success)
    {
        tell_wecom_member(&connector.config, &event.user, text).await?;
    }

    Ok(())
}
