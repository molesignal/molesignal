// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 重大事件的会议：腾讯会议、Zoom、Google Meet 在这里预约；飞书视频会议由 `room` 用飞书 App 约（见
//! `war_room`），固定链接原样用。都是尽力而为，约不成只记日志。

mod google_meet;
mod tencent;
mod zoom;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::USER_AGENT;
use serde_json::Value;

use crate::{
    domain::alerting::{meeting::MeetingMode, response::ResponseSettings},
    infra::connectors::dispatcher::http::pinned_client,
    shared::{Error, Result},
};

const TIMEOUT: Duration = Duration::from_secs(15);

/// 会议预约多久（秒）。
pub const MEETING_SECS: i64 = 6 * 3_600;

/// 按设置开一场会，返回入会链接；不开会议、或是飞书视频会议（由调用方约）时为 `None`。
pub async fn book(settings: &ResponseSettings, topic: &str) -> Result<Option<String>> {
    let secret = |mode: MeetingMode| {
        mode.secret_key()
            .and_then(|key| settings.secrets.get(key))
            .map(String::as_str)
            .filter(|secret| !secret.trim().is_empty())
            .ok_or_else(|| Error::invalid("the meeting's secret is not set"))
    };

    match settings.meeting {
        MeetingMode::None | MeetingMode::FeishuVc => Ok(None),
        MeetingMode::FixedUrl => {
            Ok(Some(settings.meeting_url.clone()).filter(|url| !url.is_empty()))
        }
        MeetingMode::TencentMeeting => tencent::book(
            &settings.tencent_meeting,
            secret(MeetingMode::TencentMeeting)?,
            topic,
        )
        .await
        .map(Some),
        MeetingMode::Zoom => zoom::book(&settings.zoom, secret(MeetingMode::Zoom)?, topic)
            .await
            .map(Some),
        MeetingMode::GoogleMeet => {
            google_meet::book(&settings.google_meet, secret(MeetingMode::GoogleMeet)?)
                .await
                .map(Some)
        }
    }
}

/// 只连这个地址解析出的 IP、不跟随跳转的客户端。
async fn client(address: &str) -> Result<reqwest::Client> {
    let url = url::Url::parse(address)
        .map_err(|error| Error::internal(format!("meeting address: {error}")))?;

    pinned_client(&url, TIMEOUT).await
}

/// 发请求，读出 JSON；不是 2xx 时把服务商的话带进错误。
async fn send(builder: reqwest::RequestBuilder, vendor: &str) -> Result<Value> {
    let response = builder
        .header(USER_AGENT, "MoleSignal")
        .send()
        .await
        .map_err(|error| Error::unavailable(format!("{vendor} could not be reached: {error}")))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);

    if !status.is_success() {
        return Err(Error::unavailable(format!(
            "{vendor} answered HTTP {}: {}",
            status.as_u16(),
            body.to_string().chars().take(300).collect::<String>()
        )));
    }

    Ok(body)
}

fn unix_seconds() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
        .map_err(|_| Error::internal("system time is before unix epoch"))
}
