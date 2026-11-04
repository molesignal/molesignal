// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 哪些应用连接器在 IM 里回答。
//!
//! - `GET    /api/v1/agent/im/bots`：能在 IM 里回答的连接器（飞书、Slack、钉钉、企业微信智能
//!   机器人、Google Chat、Microsoft Teams）、它们的设置与连接的状态；Teams 带上回调地址
//! - `PUT    /api/v1/agent/im/bots/{connector_id}`：开关、群里新话题的默认团队
//! - `DELETE /api/v1/agent/im/bots/{connector_id}`：不再在 IM 里回答
//!
//! 都要 `agent.manage`。连接器本身（凭据）在通知设置里管。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    agent::im::{ImBot, ImPlatform},
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::{iam::permission, notify::connector::NotifyConnector},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/agent/im/bots", get(list)).route(
        "/agent/im/bots/{connectorId}",
        axum::routing::put(put).delete(remove),
    )
}

/// 连接器还缺的、在 IM 里回答要的配置。
fn missing(connector: &NotifyConnector) -> Vec<&'static str> {
    let filled = |key: &str| {
        connector.config[key]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty())
    };

    match ImPlatform::of_connector(&connector.connector_type) {
        Some(ImPlatform::Slack) => {
            let mut missing = Vec::new();

            if !filled("botToken") {
                missing.push("botToken");
            }

            if !connector.config["appToken"]
                .as_str()
                .is_some_and(|token| token.trim().starts_with("xapp-"))
            {
                missing.push("appToken");
            }

            missing
        }
        Some(ImPlatform::Feishu) => ["appId", "appSecret"]
            .into_iter()
            .filter(|key| !filled(key))
            .collect(),
        Some(ImPlatform::DingTalk) => ["clientId", "clientSecret"]
            .into_iter()
            .filter(|key| !filled(key))
            .collect(),
        Some(ImPlatform::WeCom) => ["bot_id", "bot_secret"]
            .into_iter()
            .filter(|key| !filled(key))
            .collect(),
        Some(ImPlatform::Teams) => ["appId", "clientSecret", "tenantId"]
            .into_iter()
            .filter(|key| !filled(key))
            .collect(),
        Some(ImPlatform::GoogleChat) => {
            let credentials = &connector.config["credentials"];
            let mut missing = Vec::new();

            if !(credentials.is_object() || filled("credentials")) {
                missing.push("credentials");
            }

            if !filled("subscription") {
                missing.push("subscription");
            }

            missing
        }
        None => Vec::new(),
    }
}

/// 一行：`external_url` 是 MoleSignal 的对外地址，Teams 的回调地址在它下面。
fn view(
    connector: &NotifyConnector,
    bot: Option<&ImBot>,
    team: Option<String>,
    external_url: &str,
) -> Value {
    let platform = ImPlatform::of_connector(&connector.connector_type);
    let callback = (platform == Some(ImPlatform::Teams))
        .then(|| super::teams::callback_url(external_url, &connector.id))
        .flatten();

    json!({
        "connectorId": connector.id.0,
        "connectorName": connector.name,
        "connectorType": connector.connector_type,
        "connectorEnabled": connector.enabled,
        "platform": platform.map(ImPlatform::as_str),
        "missing": missing(connector),
        "callbackUrl": callback,
        "enabled": bot.is_some_and(|bot| bot.enabled),
        "defaultTeamId": bot.and_then(|bot| bot.default_team_id.as_ref()).map(|id| &id.0),
        "defaultTeamName": team,
        "status": bot.map_or("idle", |bot| bot.status.as_str()),
        "statusDetail": bot.and_then(|bot| bot.status_detail.as_deref()),
        "statusAt": bot.and_then(|bot| bot.status_at).map(|at| at.0),
        "lastMessageAt": bot.and_then(|bot| bot.last_message_at).map(|at| at.0),
    })
}

async fn team_name(state: &AppState, bot: Option<&ImBot>) -> Option<String> {
    let team_id = bot?.default_team_id.as_ref()?;

    state
        .iam
        .teams
        .get(team_id)
        .await
        .ok()
        .map(|team| team.name)
}

#[permission("agent.im.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let bots = state.agent.im.list_bots(&ctx.org_id).await?;
    let mut rows = Vec::new();

    for connector in state.alerting.notify.list_connectors(&ctx.org_id).await? {
        if ImPlatform::of_connector(&connector.connector_type).is_none() {
            continue;
        }

        let bot = bots.iter().find(|bot| bot.connector_id == connector.id);
        let team = team_name(&state, bot).await;
        rows.push(view(&connector, bot, team, &state.platform.external_url));
    }

    Ok(Json(json!({
        "bots": rows,
        "externalUrl": !state.platform.external_url.trim().is_empty(),
    })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BotInput {
    enabled: bool,
    #[serde(default)]
    default_team_id: Option<String>,
}

#[permission("agent.im.manage")]
async fn put(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(connector_id): Path<String>,
    Json(input): Json<BotInput>,
) -> Result<Json<Value>> {
    let connector = state
        .alerting
        .notify
        .get_connector(&ctx.org_id, &Id(connector_id))
        .await?;

    let Some(platform) = ImPlatform::of_connector(&connector.connector_type) else {
        return Err(Error::invalid(
            "only a Feishu (Lark), Slack, DingTalk, Google Chat, or Microsoft Teams app \
             connector or a WeCom smart bot connector can answer in IM",
        ));
    };

    let lacking = missing(&connector);

    if input.enabled && !lacking.is_empty() {
        return Err(Error::invalid(format!(
            "the connector needs {} to answer in IM",
            lacking.join(", ")
        )));
    }

    // Teams 只往 https 的公网地址投消息。
    if input.enabled
        && platform == ImPlatform::Teams
        && super::teams::callback_url(&state.platform.external_url, &connector.id).is_none()
    {
        return Err(Error::invalid(
            "Microsoft Teams delivers messages to http.external_url, which must be an https address",
        ));
    }

    let default_team_id = match input
        .default_team_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(team_id) => {
            let team = state.iam.teams.get(&Id::from_string(team_id)).await?;

            if team.org_id != ctx.org_id {
                return Err(Error::not_found(format!("team `{team_id}` not found")));
            }

            Some(team.id)
        }
        None => None,
    };

    let existing = state.agent.im.get_bot(&ctx.org_id, &connector.id).await?;
    let now = TimestampMicros::now();

    // 状态与租约归 worker 管，这里只写设置。
    let bot = state
        .agent
        .im
        .put_bot(&ImBot {
            connector_id: connector.id.clone(),
            org_id: ctx.org_id.clone(),
            enabled: input.enabled,
            default_team_id,
            status: existing.as_ref().map(|bot| bot.status).unwrap_or_default(),
            status_detail: None,
            status_at: None,
            last_message_at: None,
            created_by: existing
                .as_ref()
                .map_or_else(|| ctx.user_id.clone(), |bot| bot.created_by.clone()),
            updated_by: ctx.user_id.clone(),
            created_at: existing.as_ref().map_or(now, |bot| bot.created_at),
            updated_at: now,
        })
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.im.bot_updated",
        "notify_connector",
        &connector.id.0,
        json!({
            "enabled": bot.enabled,
            "defaultTeamId": bot.default_team_id.as_ref().map(|id| &id.0),
        }),
    )
    .await;

    let team = team_name(&state, Some(&bot)).await;
    Ok(Json(view(
        &connector,
        Some(&bot),
        team,
        &state.platform.external_url,
    )))
}

#[permission("agent.im.manage")]
async fn remove(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(connector_id): Path<String>,
) -> Result<Json<Value>> {
    let connector_id = Id(connector_id);

    state
        .agent
        .im
        .delete_bot(&ctx.org_id, &connector_id)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.im.bot_removed",
        "notify_connector",
        &connector_id.0,
        json!({}),
    )
    .await;

    Ok(Json(json!({ "deleted": true })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::notify::connector::{ConnectorCapabilities, ConnectorStatus};

    fn connector(kind: &str, config: Value) -> NotifyConnector {
        NotifyConnector {
            id: Id::from_string("c1"),
            organization_id: Id::from_string("org"),
            name: "Ops bot".into(),
            connector_type: kind.into(),
            config,
            capabilities: ConnectorCapabilities::default(),
            enabled: true,
            status: ConnectorStatus::Unknown,
            last_tested_at: None,
            last_test_status: None,
            last_test_error: None,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        }
    }

    #[test]
    fn a_slack_app_needs_an_app_level_token_to_answer() {
        let slack = connector("slack_app", json!({ "botToken": "xoxb-1" }));
        assert_eq!(missing(&slack), ["appToken"]);

        let slack = connector(
            "slack_app",
            json!({ "botToken": "xoxb-1", "appToken": "xapp-1" }),
        );
        assert!(missing(&slack).is_empty());

        let feishu = connector("lark_app", json!({ "appId": "cli_1", "appSecret": "" }));
        assert_eq!(missing(&feishu), ["appSecret"]);

        let chat = connector(
            "google_chat_app",
            json!({ "credentials": { "client_email": "x" } }),
        );
        assert_eq!(missing(&chat), ["subscription"]);
    }

    #[test]
    fn a_view_says_how_the_bot_is_doing() {
        let feishu = connector("lark_app", json!({ "appId": "cli_1", "appSecret": "s" }));
        let row = view(&feishu, None, None, "https://ms.example.com");

        assert_eq!(row["platform"], "feishu");
        assert_eq!(row["enabled"], false);
        assert_eq!(row["status"], "idle");
        assert_eq!(row["callbackUrl"], Value::Null);
        assert!(row.get("config").is_none());

        let teams = connector(
            "teams_app",
            json!({ "appId": "a", "clientSecret": "s", "tenantId": "t" }),
        );
        let row = view(&teams, None, None, "https://ms.example.com");
        assert_eq!(
            row["callbackUrl"],
            "https://ms.example.com/api/v1/public/im/teams/c1/messages"
        );
        assert!(missing(&teams).is_empty());
    }
}
