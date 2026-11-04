// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Microsoft Teams 的回调：`POST /api/v1/public/im/teams/{connector_id}/messages`。
//!
//! Bot Framework 把发给 Bot 的消息 POST 到这里：Azure Bot 的 Messaging endpoint 填
//! [`callback_url`]，在 `http.external_url` 下。这个地址不走 MoleSignal 的登录，每个请求都要带
//! Bot Framework 签的令牌，按 `infra::im::teams::auth` 验过才处理：
//!
//! - 连接器没开在 IM 里回答、停用了或不是 Teams 的，一律 404；
//! - 只处理 Teams 渠道（`msteams`）的消息，回复地址要是 Bot Framework 的；单租户的 Bot 只收自己
//!   租户的消息，别的回 200 不处理；
//! - 正文最多 256 KiB。
//!
//! 验过的消息另起任务处理，马上回 200：Bot Framework 只等 15 秒。点了告警卡片上的按钮
//! （`adaptiveCard/action`）最多等 [`im::ANSWER_WITHIN`]，结果放进回应（[`pressed`]）。

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::Value;
use tokio::sync::oneshot;

use super::{alerts::act, handle};
use crate::{
    agent::im::{CardAction, ImBot, ImPlatform},
    api::AppState,
    domain::notify::connector::NotifyConnector,
    infra::{
        im::{
            self,
            teams::{self, auth},
        },
        notify::adapters::TeamsApp,
    },
    shared::{Error, Result, ids::Id},
};

const MAX_BODY_BYTES: usize = 256 * 1024;
const CALLBACK_PATH: &str = "/api/v1/public/im/teams";

pub fn public_routes() -> Router<AppState> {
    Router::new()
        .route(
            &format!("{CALLBACK_PATH}/{{connector_id}}/messages"),
            post(messages),
        )
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
}

/// 填进 Azure Bot 的 Messaging endpoint 的地址；对外地址不是 https 时没有（Teams 只往 https 投）。
pub(crate) fn callback_url(external_url: &str, connector_id: &Id) -> Option<String> {
    let base = external_url.trim().trim_end_matches('/');

    base.starts_with("https://")
        .then(|| format!("{base}{CALLBACK_PATH}/{}/messages", connector_id.0))
}

fn not_found() -> Error {
    Error::not_found("no Microsoft Teams bot answers here")
}

async fn messages(
    State(state): State<AppState>,
    Path(connector_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    let bot = state
        .agent
        .im
        .find_bot(&Id(connector_id))
        .await?
        .filter(|bot| bot.enabled)
        .ok_or_else(not_found)?;

    let connector = state
        .alerting
        .notify
        .get_connector(&bot.org_id, &bot.connector_id)
        .await
        .map_err(|_| not_found())?;

    if !connector.enabled
        || ImPlatform::of_connector(&connector.connector_type) != Some(ImPlatform::Teams)
    {
        return Err(not_found());
    }

    let app = TeamsApp::parse(&connector.config).map_err(|_| not_found())?;
    let activity: Value = serde_json::from_slice(&body)
        .map_err(|_| Error::invalid("the body is not a Bot Framework activity"))?;

    let authorization = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok());

    // 美国政府云（GCC High、DoD）的请求由政府云的 Bot Framework 签。
    let trust = if app.government() {
        auth::Trust::government()
    } else {
        auth::Trust::public()
    };

    if let Err(error) = auth::authenticate(&trust, authorization, app.app_id(), &activity).await {
        tracing::warn!(connector = %bot.connector_id.0, %error, "a Teams callback was refused");
        return Err(error);
    }

    if activity["channelId"] != "msteams" {
        return Ok(StatusCode::OK.into_response());
    }

    if !app.trusts(activity["serviceUrl"].as_str().unwrap_or_default()) {
        return Err(Error::forbidden(
            "the activity's serviceUrl is not a Bot Framework address",
        ));
    }

    let tenant = activity["conversation"]["tenantId"]
        .as_str()
        .or_else(|| activity["channelData"]["tenant"]["id"].as_str());

    if let Some(own) = app.single_tenant()
        && tenant.is_none_or(|tenant| !tenant.eq_ignore_ascii_case(own))
    {
        tracing::debug!(connector = %bot.connector_id.0, "a Teams message from another tenant was ignored");
        return Ok(StatusCode::OK.into_response());
    }

    if let Some(action) = teams::card_action(&activity) {
        let answer = pressed(state, bot, connector, action).await;

        // 只认得 `Action.Submit` 的旧客户端发来的是消息，不看回应。
        return Ok(if activity["type"] == "invoke" {
            Json(answer).into_response()
        } else {
            StatusCode::OK.into_response()
        });
    }

    if let Some(inbound) = teams::inbound(&activity) {
        tokio::spawn(async move { handle(&state, &bot, &connector, inbound).await });
    }

    Ok(StatusCode::OK.into_response())
}

/// 点了告警卡片上的按钮。另起任务处理，结果最多等 [`im::ANSWER_WITHIN`] 放进回应：点的人马上看到
/// 换上的卡片或那句话。任务再换掉原消息，会话里每个人都看得到，来不及放进回应的也这样换上。
async fn pressed(
    state: AppState,
    bot: ImBot,
    connector: NotifyConnector,
    action: CardAction,
) -> Value {
    let (sender, receiver) = oneshot::channel();

    tokio::spawn(async move {
        let answer = act(&state, &bot, &connector, &action).await;
        let _ = sender.send(answer.clone());

        if let Err(error) = im::answer(ImPlatform::Teams, &connector.config, &action, &answer).await
        {
            tracing::warn!(connector = %bot.connector_id.0, %error, "a Teams alert card could not be updated");
        }
    });

    let answer = tokio::time::timeout(im::ANSWER_WITHIN, receiver)
        .await
        .ok()
        .and_then(Result::ok);

    teams::invoke_response(answer.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_callback_lives_under_the_https_external_url() {
        let connector = Id::from_string("c1");

        assert_eq!(
            callback_url("https://ms.example.com/", &connector).as_deref(),
            Some("https://ms.example.com/api/v1/public/im/teams/c1/messages")
        );
        assert_eq!(callback_url("http://ms.example.com", &connector), None);
        assert_eq!(callback_url("", &connector), None);
    }
}
