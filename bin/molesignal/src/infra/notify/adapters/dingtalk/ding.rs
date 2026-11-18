// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! DingTalk DING: after the message reaches a person, the app robot pages them harder — an
//! in-app DING, or a DING by text message or phone call (`POST /v1.0/robot/ding/send`,
//! `remindType` 1, 2 or 3). DingTalk offers it to Professional and Exclusive editions; text and
//! phone DINGs need a purchased quota (`ding.serverquota.insufficient` when it runs out). The app
//! needs the `Premium.Ding.Write` permission.

use domain::notify::preference::StepBoost;
use kernel::{Error, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use signals::http_trace::{self, HttpTarget};

use super::{
    super::common::{provider_message, truncate_chars},
    DINGTALK_APP_CONNECTOR_TYPE, DingTalkApp,
};

/// Every way DingTalk pages a person.
pub(super) const BOOSTS: [StepBoost; 3] = [StepBoost::App, StepBoost::Sms, StepBoost::Phone];

/// A DING's text is short; DingTalk reads it out on a phone DING.
const MAX_CONTENT_CHARS: usize = 200;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DingResponse {
    #[serde(default)]
    open_ding_id: Option<String>,
    #[serde(default)]
    failed_list: Value,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

fn remind_type(boost: StepBoost) -> u8 {
    match boost {
        StepBoost::App => 1,
        StepBoost::Sms => 2,
        StepBoost::Phone => 3,
    }
}

/// Whether DingTalk named anyone it could not page: `failedList` maps a reason to the user IDs.
fn anyone_failed(failed: &Value) -> bool {
    let named = |value: &Value| match value {
        Value::Array(users) => !users.is_empty(),
        Value::String(user) => !user.is_empty(),
        _ => false,
    };

    match failed {
        Value::Object(reasons) => reasons.values().any(named),
        other => named(other),
    }
}

impl DingTalkApp {
    /// DINGs `user_id` with `content`, the alert's title.
    pub(super) async fn ding(
        &self,
        http: &reqwest::Client,
        user_id: &str,
        content: &str,
        boost: StepBoost,
    ) -> Result<()> {
        let token = self.access_token(http).await?;

        let response = http_trace::send(
            http,
            http.post(format!("{}/v1.0/robot/ding/send", self.base()))
                .header("x-acs-dingtalk-access-token", token)
                .json(&json!({
                    "robotCode": self.robot_code(),
                    "remindType": remind_type(boost),
                    "receiverUserIdList": [user_id],
                    "content": truncate_chars(content, MAX_CONTENT_CHARS),
                })),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(|_| {
            Error::unavailable(format!("{DINGTALK_APP_CONNECTOR_TYPE} DING request failed"))
        })?;

        let status = response.status();
        let ding: DingResponse = response.json().await.unwrap_or_default();

        if !status.is_success() || ding.open_ding_id.is_none() {
            return Err(Error::unavailable(format!(
                "{DINGTALK_APP_CONNECTOR_TYPE} sent the message but could not DING the person: HTTP {} {} {}",
                status.as_u16(),
                ding.code.as_deref().unwrap_or_default(),
                provider_message(ding.message.as_deref().unwrap_or_default())
            )));
        }

        if anyone_failed(&ding.failed_list) {
            return Err(Error::unavailable(format!(
                "{DINGTALK_APP_CONNECTOR_TYPE} sent the message but DingTalk refused to DING this person"
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_partial_json, header, method, path},
    };

    use super::*;

    async fn app(server: &MockServer) -> DingTalkApp {
        Mock::given(method("POST"))
            .and(path("/v1.0/oauth2/accessToken"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "accessToken": "t", "expireIn": 7200 })),
            )
            .mount(server)
            .await;

        DingTalkApp::parse(&json!({
            "clientId": format!("ding-{}", server.address().port()),
            "clientSecret": "s",
            "apiBaseUrl": server.uri(),
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn a_phone_ding_goes_to_the_person_with_the_robot() {
        let server = MockServer::start().await;
        let app = app(&server).await;

        Mock::given(method("POST"))
            .and(path("/v1.0/robot/ding/send"))
            .and(header("x-acs-dingtalk-access-token", "t"))
            .and(body_partial_json(json!({
                "remindType": 3,
                "receiverUserIdList": ["manager1"],
                "content": "CRITICAL · API unavailable",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "openDingId": "ding-1",
                "failedList": {}
            })))
            .expect(1)
            .mount(&server)
            .await;

        app.ding(
            &app.http().unwrap(),
            "manager1",
            "CRITICAL · API unavailable",
            StepBoost::Phone,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_refused_ding_fails_the_step() {
        let server = MockServer::start().await;
        let app = app(&server).await;

        Mock::given(method("POST"))
            .and(path("/v1.0/robot/ding/send"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "openDingId": "ding-1",
                "failedList": { "ding.receiver.invalid": ["manager1"] }
            })))
            .mount(&server)
            .await;

        let refused = app
            .ding(&app.http().unwrap(), "manager1", "title", StepBoost::Sms)
            .await
            .unwrap_err();

        assert!(refused.to_string().contains("refused"));
    }
}
