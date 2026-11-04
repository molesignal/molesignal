// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 重大事件的会议：不开、预约飞书视频会议、腾讯会议、Zoom、Google Meet，或用固定的会议链接。各家的
//! 凭证放在重大事件的设置里；密钥（腾讯会议的 SecretKey、Zoom 的 Client Secret、Google 服务账号的密钥）
//! 另外加密保存，只写不读。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result};

/// 会议怎么开。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingMode {
    /// 不开会议。
    #[default]
    None,
    /// 用飞书 App 预约飞书视频会议。
    FeishuVc,
    /// 用腾讯会议的企业自建应用预约。
    TencentMeeting,
    /// 用 Zoom 的服务器到服务器 OAuth 应用开一场会。
    Zoom,
    /// 用服务账号代组织里的人建一个 Google Meet 会议。
    GoogleMeet,
    /// 用固定的会议链接。
    FixedUrl,
}

pub const TENCENT_SECRET_KEY: &str = "tencentMeeting.secretKey";
pub const ZOOM_CLIENT_SECRET: &str = "zoom.clientSecret";
pub const GOOGLE_MEET_CREDENTIALS: &str = "googleMeet.credentials";

/// 设置里能存的密钥。
pub const MEETING_SECRET_KEYS: [&str; 3] = [
    TENCENT_SECRET_KEY,
    ZOOM_CLIENT_SECRET,
    GOOGLE_MEET_CREDENTIALS,
];

impl MeetingMode {
    /// 这种会议要的密钥。
    pub const fn secret_key(self) -> Option<&'static str> {
        match self {
            Self::TencentMeeting => Some(TENCENT_SECRET_KEY),
            Self::Zoom => Some(ZOOM_CLIENT_SECRET),
            Self::GoogleMeet => Some(GOOGLE_MEET_CREDENTIALS),
            Self::None | Self::FeishuVc | Self::FixedUrl => None,
        }
    }
}

/// 腾讯会议：企业自建应用的 AppId、SdkId（应用有才填）和 SecretId；会议挂在创建人 `userid` 名下。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TencentMeetingSettings {
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub sdk_id: String,
    #[serde(default)]
    pub secret_id: String,
    /// 会议创建人：企业里的 userid。
    #[serde(default)]
    pub userid: String,
}

/// Zoom：服务器到服务器 OAuth 应用的 Account ID 和 Client ID；会议挂在主持人 `host` 名下。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoomSettings {
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub client_id: String,
    /// 主持人：Zoom 账号里一个用户的邮箱或用户 ID。
    #[serde(default)]
    pub host: String,
}

/// Google Meet：服务账号代组织里的 `organizer`（邮箱）建会议；Google Workspace 里要给服务账号开全网域
/// 委派，授权 `https://www.googleapis.com/auth/meetings.space.created`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoogleMeetSettings {
    #[serde(default)]
    pub organizer: String,
}

/// 服务账号密钥文件里用到的部分。
#[derive(Debug, Clone, Deserialize)]
pub struct ServiceAccountKey {
    pub client_email: String,
    pub private_key: String,
}

impl ServiceAccountKey {
    pub fn parse(text: &str) -> Result<Self> {
        let key: Self = serde_json::from_str(text).map_err(|_| {
            Error::invalid(
                "the Google service account key is the JSON file with client_email and private_key",
            )
        })?;

        if key.client_email.trim().is_empty() || !key.private_key.contains("PRIVATE KEY") {
            return Err(Error::invalid(
                "the Google service account key has no client_email or private_key",
            ));
        }

        Ok(key)
    }
}

/// 所选的会议要的字段都填了、要的密钥都在。`secrets` 是合并好的密钥。
pub fn validate_meeting(
    mode: MeetingMode,
    tencent: &mut TencentMeetingSettings,
    zoom: &mut ZoomSettings,
    google: &mut GoogleMeetSettings,
    secrets: &BTreeMap<String, String>,
) -> Result<()> {
    for value in [
        &mut tencent.app_id,
        &mut tencent.sdk_id,
        &mut tencent.secret_id,
        &mut tencent.userid,
        &mut zoom.account_id,
        &mut zoom.client_id,
        &mut zoom.host,
        &mut google.organizer,
    ] {
        *value = value.trim().to_string();
    }

    let filled = |values: &[&str]| values.iter().all(|value| !value.is_empty());

    let complete = match mode {
        MeetingMode::TencentMeeting => {
            filled(&[&tencent.app_id, &tencent.secret_id, &tencent.userid])
        }
        MeetingMode::Zoom => filled(&[&zoom.account_id, &zoom.client_id, &zoom.host]),
        MeetingMode::GoogleMeet => google.organizer.contains('@'),
        MeetingMode::None | MeetingMode::FeishuVc | MeetingMode::FixedUrl => true,
    };

    if !complete {
        return Err(Error::invalid(match mode {
            MeetingMode::TencentMeeting => {
                "Tencent Meeting needs the AppId, the SecretId and the creator's userid"
            }
            MeetingMode::Zoom => "Zoom needs the Account ID, the Client ID and the host",
            _ => "Google Meet needs the organizer's email",
        }));
    }

    let Some(key) = mode.secret_key() else {
        return Ok(());
    };

    let Some(secret) = secrets.get(key).filter(|secret| !secret.trim().is_empty()) else {
        return Err(Error::invalid(match mode {
            MeetingMode::TencentMeeting => "Tencent Meeting needs the SecretKey",
            MeetingMode::Zoom => "Zoom needs the Client Secret",
            _ => "Google Meet needs the service account key",
        }));
    };

    if mode == MeetingMode::GoogleMeet {
        ServiceAccountKey::parse(secret)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secrets(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn each_meeting_names_its_account_and_holds_its_secret() {
        let mut tencent = TencentMeetingSettings {
            app_id: " 200000001 ".into(),
            secret_id: "AKID".into(),
            userid: "dana".into(),
            ..Default::default()
        };
        let mut zoom = ZoomSettings::default();
        let mut google = GoogleMeetSettings::default();

        assert!(
            validate_meeting(
                MeetingMode::TencentMeeting,
                &mut tencent,
                &mut zoom,
                &mut google,
                &secrets(&[]),
            )
            .is_err()
        );
        assert!(
            validate_meeting(
                MeetingMode::TencentMeeting,
                &mut tencent,
                &mut zoom,
                &mut google,
                &secrets(&[(TENCENT_SECRET_KEY, "key")]),
            )
            .is_ok()
        );
        assert_eq!(tencent.app_id, "200000001");

        assert!(
            validate_meeting(
                MeetingMode::Zoom,
                &mut tencent,
                &mut zoom,
                &mut google,
                &secrets(&[(ZOOM_CLIENT_SECRET, "s")]),
            )
            .is_err()
        );

        google.organizer = "oncall@example.com".into();
        assert!(
            validate_meeting(
                MeetingMode::GoogleMeet,
                &mut tencent,
                &mut zoom,
                &mut google,
                &secrets(&[(GOOGLE_MEET_CREDENTIALS, "{}")]),
            )
            .is_err()
        );
        assert!(
            validate_meeting(
                MeetingMode::GoogleMeet,
                &mut tencent,
                &mut zoom,
                &mut google,
                &secrets(&[(
                    GOOGLE_MEET_CREDENTIALS,
                    r#"{"client_email": "mole@p.iam.gserviceaccount.com", "private_key": "-----BEGIN PRIVATE KEY-----\nx\n-----END PRIVATE KEY-----\n"}"#,
                )]),
            )
            .is_ok()
        );
    }
}
