// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 认出回复的是哪条短信或哪通电话之后：以接收人的身份确认或解决里面的告警。

use crate::{
    agent::runs::RunPrincipal,
    api::{AppState, http::middleware::authorize_resource},
    app::iam::IamContext,
    domain::{
        alerting::{incident::Incident, timeline::ActionVia},
        iam::IamScope,
        notify::{
            connector::NotifyConnector,
            reply::{ReplyAction, ReplyRef},
        },
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub(super) async fn sms_reply(
    state: &AppState,
    connector: &NotifyConnector,
    from: &str,
    text: &str,
) {
    let found = state
        .alerting
        .reply_callbacks
        .sms_reply(connector, from, text)
        .await;

    match found {
        Ok(Some((reference, action))) => {
            act(state, &reference, action, ActionVia::Sms).await;
        }
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(connector_id = %connector.id, %error, "an SMS reply could not be read")
        }
    }
}

/// 按了一个键，返回回复的是谁、做了什么、处理了几个告警。
pub(super) async fn keypress_reply(
    state: &AppState,
    connector: &NotifyConnector,
    reference: &str,
    digits: &str,
) -> Option<(ReplyRef, ReplyAction, usize)> {
    let found = state
        .alerting
        .reply_callbacks
        .keypress(connector, reference, digits)
        .await;

    match found {
        Ok(Some((reference, action))) => {
            let done = act(state, &reference, action, ActionVia::Phone).await;
            Some((reference, action, done))
        }
        Ok(None) => None,
        Err(error) => {
            tracing::warn!(connector_id = %connector.id, %error, "a keypress could not be read");
            None
        }
    }
}

/// 按键对上了一条回复引用就处理。
pub(super) async fn press_found(
    state: &AppState,
    connector: &NotifyConnector,
    found: Result<Option<(ReplyRef, ReplyAction)>>,
) {
    match found {
        Ok(Some((reference, action))) => {
            act(state, &reference, action, ActionVia::Phone).await;
        }
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(connector_id = %connector.id, %error, "a keypress could not be read");
        }
    }
}

/// 以接收人的身份处理回复里的告警，返回处理成了几个。没有接收人、没有权限的跳过。
async fn act(state: &AppState, reference: &ReplyRef, action: ReplyAction, via: ActionVia) -> usize {
    let Some(user_id) = &reference.recipient_user_id else {
        return 0;
    };

    let Ok(ctx) = responder(state, &reference.organization_id, user_id).await else {
        return 0;
    };

    let service = &state.alerting.service;
    let now = TimestampMicros::now();
    let mut done = 0;

    for alert_id in &reference.alert_ids {
        let Ok(incident) = authorize_resource::<Incident>(
            state,
            &ctx,
            alert_id.clone(),
            "alerts.incidents.acknowledge",
        )
        .await
        else {
            continue;
        };

        if incident.org_id != reference.organization_id || !incident.is_active() {
            continue;
        }

        let result = match action {
            ReplyAction::Acknowledge => {
                service
                    .acknowledge(&incident.id, user_id.clone(), now, via)
                    .await
            }
            ReplyAction::Resolve => {
                service
                    .resolve(&incident.id, user_id.clone(), now, via)
                    .await
            }
        };

        if result.is_ok() {
            done += 1;
        }
    }

    done
}

/// 回复的人此刻在组织里的权限：账号与组织还能用，再解析成员关系与权限。
async fn responder(state: &AppState, org_id: &Id, user_id: &Id) -> Result<IamContext> {
    state.iam.service.ensure_user_access(user_id).await?;
    state.iam.service.ensure_organization_access(org_id).await?;

    let principal = RunPrincipal {
        user_id: user_id.clone(),
        scope: IamScope::Organization,
        credential_token_id: None,
        credential_application_id: None,
        credential_service_account_id: None,
    };

    let mut context = principal.unresolved_context(org_id);
    state.iam.access.enrich_context(&mut context).await?;

    Ok(context)
}
