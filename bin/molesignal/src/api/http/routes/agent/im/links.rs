// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 账号绑定：IM 里的人还没对上 MoleSignal 用户时，私下发一个一次性链接；他登录 MoleSignal 后
//! 打开、确认，这个 IM 账号就成了他已验证的通知身份。
//!
//! - `GET  /api/v1/agent/im/links/{token}`：链接要绑定的 IM 账号（没用过、没过期时）
//! - `POST /api/v1/agent/im/links/{token}`：确认绑定到自己
//!
//! 链接只私下发：群里的人看不到别人的链接。只存令牌的摘要，用一次就作废，15 分钟过期。

use aws_lc_rs::{
    digest,
    rand::{SecureRandom, SystemRandom},
};
use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};

use super::{inbound::Place, texts::Texts};
use crate::{
    agent::im::{ConversationKind, ImLink, ImPlatform, LINK_LIFETIME_MICROS},
    api::{AppState, http::routes::activity_audit},
    app::{
        iam::IamContext,
        notify::{CreateUserNotifyEndpoint, UpdateUserNotifyEndpoint},
    },
    domain::iam::permission,
    infra::im,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/agent/im/links/{token}", get(peek).post(confirm))
}

fn new_token() -> Result<String> {
    let mut bytes = [0_u8; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| Error::internal("a link token could not be generated"))?;

    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn token_hash(token: &str) -> String {
    digest::digest(&digest::SHA256, token.trim().as_bytes())
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 绑定链接给到了没有。
pub(super) enum Offer {
    /// 私下给了：私聊里回复在原处，群里私下发（Slack 是只有他看得到的消息）。
    Given,
    /// 给不了，这句话说怎么办。
    Refused(String),
}

/// 私下给发消息的人一个绑定链接；群里只说发了。没有对外地址时请他找管理员。
pub(super) async fn offer(place: &Place<'_>, texts: &Texts) {
    match give(place, texts).await {
        // Slack 发的是只有他看得到的消息，不用再说。
        Offer::Given
            if place.inbound.kind == ConversationKind::Direct
                || place.platform == ImPlatform::Slack => {}
        Offer::Given => {
            let platform = texts.platform(place.platform);
            place
                .say(&texts.get("link_sent", &[("platform", &platform)]))
                .await;
        }
        Offer::Refused(text) => place.say(&text).await,
    }
}

/// 私下给发消息的人一个绑定链接：私聊里回复在原处，群里私下发。
pub(super) async fn give(place: &Place<'_>, texts: &Texts) -> Offer {
    let platform = texts.platform(place.platform);
    let platform = platform.as_str();
    let base = place
        .state
        .platform
        .external_url
        .trim()
        .trim_end_matches('/');

    let unavailable = texts.get("link_unavailable", &[("platform", platform)]);

    if base.is_empty() {
        return Offer::Refused(unavailable);
    }

    let Ok(token) = new_token() else {
        return Offer::Refused(unavailable);
    };

    let now = TimestampMicros::now();
    let link = ImLink {
        token_hash: token_hash(&token),
        org_id: place.bot.org_id.clone(),
        connector_id: place.bot.connector_id.clone(),
        external_identity: place.inbound.sender.primary().to_string(),
        display_name: place.inbound.sender.name.clone(),
        expires_at: TimestampMicros(now.0 + LINK_LIFETIME_MICROS),
        created_at: now,
    };

    if let Err(error) = place.state.agent.im.create_link(&link).await {
        tracing::warn!(%error, "an IM link could not be saved");
        return Offer::Refused(unavailable);
    }

    let url = format!("{base}/agent/im/link?token={token}");
    let text = texts.get("link", &[("platform", platform), ("link", &url)]);

    if place.inbound.kind == ConversationKind::Direct {
        place.say(&text).await;
        return Offer::Given;
    }

    match im::tell_sender(
        place.platform,
        &place.connector.config,
        &place.inbound.reply,
        place.inbound.sender.primary(),
        &text,
    )
    .await
    {
        Ok(()) => Offer::Given,
        // 私聊还没开过（企业微信要先私聊过机器人）：请他先私聊，那里直接给链接。
        Err(error) => {
            tracing::warn!(%error, "an IM link could not be sent privately");
            Offer::Refused(texts.get("link_dm_first", &[("platform", platform)]))
        }
    }
}

/// 本组织里没用过、没过期的链接。
async fn find(state: &AppState, ctx: &IamContext, token: &str) -> Result<ImLink> {
    state
        .agent
        .im
        .peek_link(&token_hash(token), TimestampMicros::now())
        .await?
        .filter(|link| link.org_id == ctx.org_id)
        .ok_or_else(|| Error::not_found("this link was used or has expired; ask in IM again"))
}

#[permission("agent.use")]
async fn peek(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(token): Path<String>,
) -> Result<Json<Value>> {
    let link = find(&state, &ctx, &token).await?;
    let connector = state
        .alerting
        .notify
        .get_connector(&ctx.org_id, &link.connector_id)
        .await?;

    Ok(Json(json!({
        "platform": ImPlatform::of_connector(&connector.connector_type).map(ImPlatform::as_str),
        "connectorName": connector.name,
        "externalIdentity": link.external_identity,
        "displayName": link.display_name,
        "expiresAt": link.expires_at.0,
    })))
}

#[permission("agent.use")]
async fn confirm(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(token): Path<String>,
) -> Result<Json<Value>> {
    let link = find(&state, &ctx, &token).await?;
    let notify = &state.alerting.notify;
    let endpoints = notify.list_organization_endpoints(&ctx.org_id).await?;
    let same = |connector_id: &Id, identity: &str| {
        *connector_id == link.connector_id && identity.trim() == link.external_identity
    };

    // 已经是别人已验证的身份时不改：请管理员处理。
    if endpoints.iter().any(|endpoint| {
        same(&endpoint.connector_id, &endpoint.external_identity)
            && endpoint.user_id != ctx.user_id
            && endpoint.verified
            && endpoint.enabled
    }) {
        return Err(Error::conflict(
            "this IM account is already linked to another MoleSignal user",
        ));
    }

    let link = state
        .agent
        .im
        .take_link(&token_hash(&token), TimestampMicros::now())
        .await?
        .ok_or_else(|| Error::not_found("this link was used or has expired; ask in IM again"))?;

    let own = endpoints.into_iter().find(|endpoint| {
        endpoint.user_id == ctx.user_id && same(&endpoint.connector_id, &endpoint.external_identity)
    });

    let endpoint = match own {
        Some(endpoint) => {
            notify
                .update_endpoint(
                    &ctx.org_id,
                    &ctx.user_id,
                    &endpoint.id,
                    UpdateUserNotifyEndpoint {
                        connector_id: endpoint.connector_id,
                        external_identity: endpoint.external_identity,
                        display_name: endpoint.display_name.or(link.display_name.clone()),
                        metadata: endpoint.metadata,
                        verified: true,
                        enabled: true,
                    },
                )
                .await?
        }
        None => {
            notify
                .create_endpoint(
                    &ctx.org_id,
                    &ctx.user_id,
                    CreateUserNotifyEndpoint {
                        connector_id: link.connector_id.clone(),
                        external_identity: link.external_identity.clone(),
                        display_name: link.display_name.clone(),
                        metadata: json!({ "linkedFrom": "agent_im" }),
                        verified: true,
                        enabled: true,
                    },
                )
                .await?
        }
    };

    activity_audit::record(
        &state,
        &ctx,
        "agent.im.account_linked",
        "user_notify_endpoint",
        &endpoint.id.0,
        json!({ "connectorId": link.connector_id.0 }),
    )
    .await;

    Ok(Json(json!({ "linked": true, "endpointId": endpoint.id.0 })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_random_and_only_its_hash_is_kept() {
        let first = new_token().unwrap();
        let second = new_token().unwrap();

        assert_ne!(first, second);
        assert_eq!(first.len(), 43);
        assert_eq!(token_hash(&first).len(), 64);
        assert_eq!(token_hash(&first), token_hash(&format!(" {first} ")));
        assert_ne!(token_hash(&first), token_hash(&second));
    }
}
