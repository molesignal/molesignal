// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 服务商回调：短信回复、电话按键处理告警。
//!
//! - `POST /api/v1/public/notify/{connector_id}/replies`：收到的短信。Twilio 验 `X-Twilio-Signature`；
//!   AWS 经 SNS 推送，验 SNS 的签名（订阅确认也在这里）；阿里云、腾讯云、华为云的上行短信与通用短信网关
//!   验地址里的回调 Token（`?token=`）。
//! - `POST /api/v1/public/notify/{connector_id}/keypress`：电话上按的键。Twilio 是 `<Gather>` 的回调，
//!   验签名；阿里云 IvrCall、腾讯云、华为云的按键推送验回调 Token。
//! - 华为云的回调地址写在控制台里，不一定收查询串：回调 Token 也可以放在路径的最后一段
//!   （`.../replies/{token}`、`.../keypress/{token}`）。
//! - `GET  /api/v1/notify/connectors/{id}/callbacks`：连接器页显示的回调地址（`notify.connectors.manage`），
//!   也包括企业微信自建应用「接收消息」的地址（路由在 `wecom_callback`）。
//!
//! 认出回复的是哪条短信或哪通电话后，以它的接收人的身份确认或解决里面的告警，要有
//! `alerts.incidents.acknowledge`。回复不对、找不到、没有权限时照样答 2xx，免得服务商反复重推。

use std::collections::BTreeMap;

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use self::respond::{keypress_reply, press_found, sms_reply};
use super::callback_auth::{sns_verified, twilio_signature_matches};
use crate::{
    api::{AppState, http::routes::agent::im::keypress_answer},
    app::iam::IamContext,
    domain::{
        iam::permission,
        notify::{
            connector::NotifyConnector,
            reply::{ReplyAction, ReplyRef},
        },
    },
    shared::{Error, Result, ids::Id},
};

mod respond;

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/notify/connectors/{id}/callbacks", get(callback_urls))
}

pub fn public_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/public/notify/{connectorId}/replies", post(replies))
        .route(
            "/api/v1/public/notify/{connectorId}/keypress",
            post(keypress),
        )
        .route(
            "/api/v1/public/notify/{connectorId}/replies/{token}",
            post(replies_with_path_token),
        )
        .route(
            "/api/v1/public/notify/{connectorId}/keypress/{token}",
            post(keypress_with_path_token),
        )
}

/// 回调 Token 在路径里的 `replies`。
async fn replies_with_path_token(
    state: State<AppState>,
    Path((connector_id, token)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    let query = CallbackQuery {
        token: Some(token),
        reference: None,
    };

    replies(state, Path(connector_id), Query(query), headers, body).await
}

/// 回调 Token 在路径里的 `keypress`。
async fn keypress_with_path_token(
    state: State<AppState>,
    Path((connector_id, token)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    let query = CallbackQuery {
        token: Some(token),
        reference: None,
    };

    keypress(state, Path(connector_id), Query(query), headers, body).await
}

#[derive(Debug, Default, Deserialize)]
struct CallbackQuery {
    #[serde(default)]
    token: Option<String>,
    /// 电话按键：拨出时记的回复引用。
    #[serde(default, rename = "ref")]
    reference: Option<String>,
}

/// 连接器页显示的回调地址；没有对外地址、或这种连接器不用配地址时为空。
#[permission("notify.connectors.manage")]
async fn callback_urls(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let connector = state
        .alerting
        .notify
        .get_connector(&ctx.org_id, &Id::from_string(id))
        .await?;

    let base = state.platform.external_url.trim().trim_end_matches('/');
    let callbacks = &state.alerting.reply_callbacks;
    let token = callbacks.token(&connector.id);
    let address = |kind: &str, signed: bool| -> Value {
        if base.is_empty() {
            return Value::Null;
        }

        let url = format!("{base}/api/v1/public/notify/{}/{kind}", connector.id);

        Value::String(if signed {
            url
        } else {
            format!("{url}?token={token}")
        })
    };

    // 企业微信自建应用的「接收消息」URL：模板卡片按钮的点击回到这里，签名由企业微信带着。
    let message = if connector.connector_type == "wecom_app" {
        address("wecom", true)
    } else {
        Value::Null
    };

    // 华为云控制台里填的地址：Token 放在路径里。
    let in_path = |kind: &str| -> Value {
        if base.is_empty() {
            return Value::Null;
        }

        Value::String(format!(
            "{base}/api/v1/public/notify/{}/{kind}/{token}",
            connector.id
        ))
    };

    let (replies, keypress) = match connector.connector_type.as_str() {
        "twilio_sms" => (address("replies", true), Value::Null),
        "aws_sms" => (address("replies", true), Value::Null),
        "aliyun_sms" | "tencent_sms" | "http_sms" => (address("replies", false), Value::Null),
        "aliyun_voice" | "tencent_voice" => (Value::Null, address("keypress", false)),
        "huawei_sms" => (in_path("replies"), Value::Null),
        "huawei_voice" => (Value::Null, in_path("keypress")),
        // Twilio 的按键地址在每通电话里带着，不用配置。
        _ => (Value::Null, Value::Null),
    };

    Ok(Json(json!({
        "external_url_set": !base.is_empty(),
        "replies_url": replies,
        "keypress_url": keypress,
        "message_url": message,
    })))
}

async fn replies(
    State(state): State<AppState>,
    Path(connector_id): Path<String>,
    Query(query): Query<CallbackQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    let connector = known_connector(&state, &connector_id).await?;

    match connector.connector_type.as_str() {
        "twilio_sms" => {
            let form = twilio_form(&state, &connector, &headers, &body, "replies", None)?;
            let from = form.get("From").cloned().unwrap_or_default();
            let text = form.get("Body").cloned().unwrap_or_default();

            sms_reply(&state, &connector, &from, &text).await;
            Ok(twiml("<Response></Response>"))
        }
        "aws_sms" => {
            let notice: Value = serde_json::from_slice(&body)
                .map_err(|_| Error::invalid("not an Amazon SNS message"))?;

            let Some(message) = sns_verified(&connector, &notice).await? else {
                // 订阅确认已经做了。
                return Ok(StatusCode::OK.into_response());
            };

            let from = message["originationNumber"].as_str().unwrap_or_default();
            let text = message["messageBody"].as_str().unwrap_or_default();

            sms_reply(&state, &connector, from, text).await;
            Ok(StatusCode::OK.into_response())
        }
        "aliyun_sms" => {
            check_token(&state, &connector, query.token.as_deref())?;

            for item in items(&body)? {
                let from = item["phone_number"].as_str().unwrap_or_default();
                let text = item["content"].as_str().unwrap_or_default();
                sms_reply(&state, &connector, from, text).await;
            }

            Ok(aliyun_received())
        }
        "tencent_sms" => {
            check_token(&state, &connector, query.token.as_deref())?;

            for item in items(&body)? {
                let from = item["mobile"].as_str().unwrap_or_default();
                let text = item["text"].as_str().unwrap_or_default();
                sms_reply(&state, &connector, from, text).await;
            }

            Ok(tencent_received())
        }
        "huawei_sms" => {
            check_token(&state, &connector, query.token.as_deref())?;

            let form: BTreeMap<String, String> = url::form_urlencoded::parse(&body)
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect();

            let from = form.get("from").map_or("", String::as_str);
            let text = form.get("body").map_or("", String::as_str);

            sms_reply(&state, &connector, from, text).await;
            Ok(StatusCode::OK.into_response())
        }
        "http_sms" => {
            check_token(&state, &connector, query.token.as_deref())?;

            for item in items(&body)? {
                let from = item["phone"].as_str().unwrap_or_default();
                let text = item["text"].as_str().unwrap_or_default();
                sms_reply(&state, &connector, from, text).await;
            }

            Ok(Json(json!({ "ok": true })).into_response())
        }
        _ => Err(Error::not_found("this connector takes no replies")),
    }
}

async fn keypress(
    State(state): State<AppState>,
    Path(connector_id): Path<String>,
    Query(query): Query<CallbackQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    let connector = known_connector(&state, &connector_id).await?;

    match connector.connector_type.as_str() {
        "twilio_voice" => {
            let reference = query.reference.clone().unwrap_or_default();
            let form = twilio_form(
                &state,
                &connector,
                &headers,
                &body,
                "keypress",
                Some(&reference),
            )?;
            let digits = form.get("Digits").cloned().unwrap_or_default();
            let done = keypress_reply(&state, &connector, &reference, &digits).await;

            Ok(twiml(&spoken_answer(&state, &connector, done).await))
        }
        "aliyun_voice" => {
            check_token(&state, &connector, query.token.as_deref())?;

            for item in items(&body)? {
                let reference = item["out_id"].as_str().unwrap_or_default();
                let digits = match &item["digits"] {
                    Value::String(digits) => digits.clone(),
                    Value::Number(number) => number.to_string(),
                    _ => String::new(),
                };

                keypress_reply(&state, &connector, reference, &digits).await;
            }

            Ok(aliyun_received())
        }
        "tencent_voice" => {
            check_token(&state, &connector, query.token.as_deref())?;

            for item in items(&body)? {
                let press = &item["voicekey_callback"];
                let number = press["mobile"].as_str().unwrap_or_default();
                let digits = press["keypress"].as_str().unwrap_or_default();

                let found = state
                    .alerting
                    .reply_callbacks
                    .keypress_on_line(&connector, number, digits)
                    .await;

                press_found(&state, &connector, found).await;
            }

            Ok(tencent_received())
        }
        "huawei_voice" => {
            check_token(&state, &connector, query.token.as_deref())?;

            let push: Value = serde_json::from_slice(&body)
                .map_err(|_| Error::invalid("the callback body is not JSON"))?;

            huawei_call_status(&state, &connector, &push).await;
            Ok(StatusCode::OK.into_response())
        }
        _ => Err(Error::not_found("this connector takes no keypresses")),
    }
}

/// 回调地址里的连接器：要存在、开着、开了回复。别的一律当作没有，不透露连接器在不在。
async fn known_connector(state: &AppState, connector_id: &str) -> Result<NotifyConnector> {
    let connector = state
        .alerting
        .reply_callbacks
        .connector(&Id::from_string(connector_id))
        .await?
        .filter(|connector| connector.enabled)
        .filter(|connector| {
            connector
                .config
                .get("replies")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        });

    connector.ok_or_else(|| Error::not_found("no such callback"))
}

fn check_token(state: &AppState, connector: &NotifyConnector, token: Option<&str>) -> Result<()> {
    let token = token.unwrap_or_default();

    if state
        .alerting
        .reply_callbacks
        .token_matches(&connector.id, token)
    {
        Ok(())
    } else {
        Err(Error::unauthorized("invalid callback token"))
    }
}

/// Twilio 的表单参数，签名对了才收。签名按 Twilio 请求的地址算：对外地址加上回调的路径与查询。
fn twilio_form(
    state: &AppState,
    connector: &NotifyConnector,
    headers: &HeaderMap,
    body: &[u8],
    kind: &str,
    reference: Option<&str>,
) -> Result<BTreeMap<String, String>> {
    let form: BTreeMap<String, String> = url::form_urlencoded::parse(body)
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();

    let base = state.platform.external_url.trim().trim_end_matches('/');
    let mut url = format!("{base}/api/v1/public/notify/{}/{kind}", connector.id);

    if let Some(reference) = reference {
        url.push_str(&format!("?ref={reference}"));
    }

    let auth_token = connector
        .config
        .get("authToken")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let signature = headers
        .get("x-twilio-signature")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    if base.is_empty() || !twilio_signature_matches(auth_token, &url, &form, signature) {
        return Err(Error::unauthorized("invalid Twilio signature"));
    }

    Ok(form)
}

/// 推送的内容：一个对象或一组对象。
fn items(body: &[u8]) -> Result<Vec<Value>> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Array(items)) => Ok(items),
        Ok(item @ Value::Object(_)) => Ok(vec![item]),
        _ => Err(Error::invalid("the callback body is not JSON")),
    }
}

/// 华为云的呼叫状态推送：收号结果只带会话 ID，更早的呼出、振铃、接听带着我们的回复引用（`userData`），收到时
/// 把会话 ID 记到引用上。
async fn huawei_call_status(state: &AppState, connector: &NotifyConnector, push: &Value) {
    let info = &push["statusInfo"];
    let session = info["sessionId"].as_str().unwrap_or_default();
    let callbacks = &state.alerting.reply_callbacks;

    if session.is_empty() {
        return;
    }

    if push["eventType"].as_str() == Some("collectInfo") {
        let digits = info["digitInfo"].as_str().unwrap_or_default();
        let found = callbacks.keypress_on_call(connector, session, digits).await;

        press_found(state, connector, found).await;
    } else if let Some(reference) = info["userData"].as_str()
        && let Err(error) = callbacks.bind_call(connector, reference, session).await
    {
        tracing::warn!(connector_id = %connector.id, %error, "a call could not be tied to its reply reference");
    }
}

/// 按键之后电话里念的那句：用接收人的语言，按连接器设的语言念。
async fn spoken_answer(
    state: &AppState,
    connector: &NotifyConnector,
    done: Option<(ReplyRef, ReplyAction, usize)>,
) -> String {
    let language = connector
        .config
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or("en-US");

    let recipient = done
        .as_ref()
        .and_then(|(reference, _, _)| reference.recipient_user_id.clone());

    let said = keypress_answer(
        state,
        &connector.organization_id,
        recipient.as_ref(),
        done.map(|(_, action, count)| (action, count)),
    )
    .await;

    format!(
        "<Response><Say language=\"{}\">{}</Say></Response>",
        xml_escape(language),
        xml_escape(&said)
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn twiml(body: &str) -> Response {
    ([(CONTENT_TYPE, "text/xml")], body.to_string()).into_response()
}

/// 腾讯云推送要的回答：收到了。
fn tencent_received() -> Response {
    Json(json!({ "result": 0, "errmsg": "OK" })).into_response()
}

/// 阿里云推送要的回答：收到了。
fn aliyun_received() -> Response {
    Json(json!({ "code": 0, "msg": "成功" })).into_response()
}
