// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Feishu / Lark message urgent: after the message reaches a person, the app pages them harder —
//! a persistent in-app urgent, or, from the tenant's urgent quota, a text message or a phone call
//! (`PATCH /im/v1/messages/{message_id}/urgent_app|urgent_sms|urgent_phone`). Only the app's own
//! messages can be made urgent, and only to people in the chat. Lark, the international site,
//! has only the in-app urgent.

use domain::notify::preference::StepBoost;
use kernel::{Error, Result};
use serde::Deserialize;
use serde_json::json;
use signals::http_trace::{self, HttpTarget};

use super::{LARK_APP_CONNECTOR_TYPE, safe_provider_message};

/// Every way Feishu pages a person.
pub(super) const BOOSTS: [StepBoost; 3] = [StepBoost::App, StepBoost::Sms, StepBoost::Phone];

#[derive(Debug, Default, Deserialize)]
struct UrgentResponse {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    data: Option<UrgentData>,
}

#[derive(Debug, Default, Deserialize)]
struct UrgentData {
    #[serde(default)]
    invalid_user_id_list: Vec<String>,
}

fn endpoint(boost: StepBoost) -> &'static str {
    match boost {
        StepBoost::App => "urgent_app",
        StepBoost::Sms => "urgent_sms",
        StepBoost::Phone => "urgent_phone",
    }
}

/// Pages `user_id` (of `user_id_type`) about `message_id`, the message just sent to them.
pub(super) async fn page(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    message_id: &str,
    user_id_type: &str,
    user_id: &str,
    boost: StepBoost,
) -> Result<()> {
    if !matches!(user_id_type, "open_id" | "user_id" | "union_id") {
        return Err(Error::invalid(format!(
            "{LARK_APP_CONNECTOR_TYPE} pages people by open_id, user_id or union_id, not {user_id_type}"
        )));
    }

    let kind = endpoint(boost);

    if boost != StepBoost::App && base.contains("larksuite.com") {
        return Err(Error::invalid(format!(
            "Lark offers only the in-app urgent; {kind} needs Feishu"
        )));
    }

    let response = http_trace::send(
        client,
        client
            .patch(format!(
                "{base}/im/v1/messages/{message_id}/{kind}?user_id_type={user_id_type}"
            ))
            .bearer_auth(token)
            .json(&json!({ "user_id_list": [user_id] })),
        HttpTarget::ThirdParty,
    )
    .await
    .map_err(|_| Error::unavailable(format!("{LARK_APP_CONNECTOR_TYPE} {kind} request failed")))?;

    let response: UrgentResponse = response.json().await.unwrap_or_default();

    if response.code != 0 {
        return Err(Error::unavailable(format!(
            "{LARK_APP_CONNECTOR_TYPE} sent the message but could not page with {kind}: {}",
            safe_provider_message(&response.msg)
        )));
    }

    if response
        .data
        .is_some_and(|data| !data.invalid_user_id_list.is_empty())
    {
        return Err(Error::unavailable(format!(
            "{LARK_APP_CONNECTOR_TYPE} sent the message but Feishu refused to page this person with {kind}"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, header, method, path, query_param},
    };

    use super::*;

    #[tokio::test]
    async fn a_phone_urgent_names_the_person_on_the_message() {
        let server = MockServer::start().await;

        Mock::given(method("PATCH"))
            .and(path("/im/v1/messages/om_1/urgent_phone"))
            .and(query_param("user_id_type", "open_id"))
            .and(header("authorization", "Bearer t"))
            .and(body_json(json!({ "user_id_list": ["ou_1"] })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 0,
                "data": { "invalid_user_id_list": [] }
            })))
            .expect(1)
            .mount(&server)
            .await;

        page(
            &reqwest::Client::new(),
            &server.uri(),
            "t",
            "om_1",
            "open_id",
            "ou_1",
            StepBoost::Phone,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn an_exhausted_quota_or_a_refused_person_fails_the_step() {
        let server = MockServer::start().await;

        Mock::given(method("PATCH"))
            .and(path("/im/v1/messages/om_1/urgent_sms"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 230024,
                "msg": "urgent quota exhausted"
            })))
            .mount(&server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/im/v1/messages/om_1/urgent_app"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 0,
                "data": { "invalid_user_id_list": ["ou_1"] }
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let uri = server.uri();
        let page_with = |boost| page(&client, &uri, "t", "om_1", "open_id", "ou_1", boost);

        let quota = page_with(StepBoost::Sms).await.unwrap_err();
        assert!(quota.to_string().contains("urgent quota exhausted"));

        let refused = page_with(StepBoost::App).await.unwrap_err();
        assert!(refused.to_string().contains("refused"));
    }

    #[tokio::test]
    async fn lark_pages_only_in_the_app() {
        let refused = page(
            &reqwest::Client::new(),
            "https://open.larksuite.com/open-apis",
            "t",
            "om_1",
            "open_id",
            "ou_1",
            StepBoost::Phone,
        )
        .await
        .unwrap_err();

        assert!(refused.to_string().contains("only the in-app urgent"));
    }

    #[tokio::test]
    async fn people_known_only_by_email_cannot_be_paged() {
        let refused = page(
            &reqwest::Client::new(),
            "http://127.0.0.1:9",
            "t",
            "om_1",
            "email",
            "a@example.com",
            StepBoost::App,
        )
        .await
        .unwrap_err();

        assert!(refused.to_string().contains("open_id"));
    }
}
