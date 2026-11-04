// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Zoom：服务器到服务器 OAuth 应用。`POST https://zoom.us/oauth/token?grant_type=account_credentials`
//! 用 Account ID、Client ID 和 Client Secret 换令牌，再 `POST https://api.zoom.us/v2/users/{主持人}/meetings`
//! 开一场即时会议。应用要有 `meeting:write:meeting:admin`（或旧的 `meeting:write:admin`）权限。

use serde_json::json;

use super::{client, send};
use crate::{
    domain::alerting::meeting::ZoomSettings,
    shared::{Error, Result},
};

const TOKEN_URL: &str = "https://zoom.us/oauth/token";
const API: &str = "https://api.zoom.us/v2";

pub(super) async fn book(
    settings: &ZoomSettings,
    client_secret: &str,
    topic: &str,
) -> Result<String> {
    let token = token(settings, client_secret).await?;
    let host =
        percent_encoding::utf8_percent_encode(&settings.host, percent_encoding::NON_ALPHANUMERIC);

    let created = send(
        client(API)
            .await?
            .post(format!("{API}/users/{host}/meetings"))
            .bearer_auth(token)
            .json(&json!({
                "topic": topic.chars().take(200).collect::<String>(),
                "type": 1,
                "settings": { "join_before_host": true, "waiting_room": false },
            })),
        "Zoom",
    )
    .await?;

    created["join_url"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| Error::unavailable("Zoom did not return the meeting link"))
}

async fn token(settings: &ZoomSettings, client_secret: &str) -> Result<String> {
    let account = percent_encoding::utf8_percent_encode(
        &settings.account_id,
        percent_encoding::NON_ALPHANUMERIC,
    );

    let issued = send(
        client(TOKEN_URL)
            .await?
            .post(format!(
                "{TOKEN_URL}?grant_type=account_credentials&account_id={account}"
            ))
            .basic_auth(&settings.client_id, Some(client_secret)),
        "Zoom",
    )
    .await?;

    issued["access_token"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| Error::unavailable("Zoom did not issue a token"))
}
