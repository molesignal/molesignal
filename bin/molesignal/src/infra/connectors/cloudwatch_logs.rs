// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! AWS CloudWatch Logs pull connector。
//!
//! 周期调 `FilterLogEvents`（AWS JSON 协议，SigV4 签名，不引 AWS SDK——见 [`super::aws::sigv4`]）
//! 拉取 `[last_run, now]` 窗口内的日志，按 `nextToken` 翻页，转 [`RawEvent`] 交由 ConnectorRunner
//! 写入。
//!
//! 配置（`config_json`）：`region` / `log_group` / `target_stream`，凭据按 `auth`（访问密钥、
//! 临时密钥、节点身份、扮演角色、Web Identity，见 [`super::config::aws`]）。缺区域或日志组 →
//! no-op（返回 0 条）。

use async_trait::async_trait;
use object_store::aws::AwsCredentialProvider;
use serde_json::{Map, Value};

use super::{
    PullBatch, PullConnector, PullContext,
    aws::{
        self,
        sigv4::{self, SigningKey},
    },
};
use crate::{
    domain::intake::RawEvent,
    shared::{Error, Result, time::TimestampMicros},
};

/// 默认回溯窗口（首次运行、无 last_run 时）：5 分钟。
const DEFAULT_LOOKBACK_MS: i64 = 5 * 60 * 1000;

/// 单次 run_once 的翻页上限（防异常时无限翻页）。
const MAX_PAGES: usize = 50;

const TARGET: &str = "Logs_20140328.FilterLogEvents";

pub struct CloudWatchLogsConnector {
    pub region: String,
    pub log_group: String,
    pub target_stream: String,
    /// What the requests are signed with; the reason when the settings give nothing to sign with.
    pub credentials: std::result::Result<AwsCredentialProvider, String>,
}

#[async_trait]
impl PullConnector for CloudWatchLogsConnector {
    async fn run_once(&self, ctx: &PullContext) -> Result<PullBatch> {
        if self.region.is_empty() || self.log_group.is_empty() {
            // 未配置完整 → 静默 no-op（CRUD 可先建、后补）。
            return Ok(PullBatch::default());
        }

        let now_ms = TimestampMicros::now().0 / 1_000;

        let start_ms = ctx
            .last_run_at_micros
            .map(|m| m / 1_000)
            .unwrap_or(now_ms - DEFAULT_LOOKBACK_MS);

        let client = reqwest::Client::new();
        let mut events = Vec::new();
        let mut next_token: Option<String> = None;

        for _ in 0..MAX_PAGES {
            let body = build_body(&self.log_group, start_ms, now_ms, next_token.as_deref());
            let parsed = self.filter_log_events(&client, body).await?;

            if let Some(arr) = parsed.get("events").and_then(|v| v.as_array()) {
                for e in arr {
                    events.push(event_to_raw(e, &self.log_group));
                }
            }

            next_token = parsed
                .get("nextToken")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from);

            if next_token.is_none() {
                break;
            }
        }

        Ok(PullBatch {
            events,
            ..PullBatch::default()
        })
    }
}

impl CloudWatchLogsConnector {
    /// One `FilterLogEvents` over the last minute, for at most one event: it takes the
    /// credentials and the `logs:FilterLogEvents` permission a pull takes, and finds out whether
    /// the log group is there, without moving the connector's cursor.
    pub async fn check(&self) -> Result<()> {
        let now_ms = TimestampMicros::now().0 / 1_000;
        let mut body: Value =
            serde_json::from_str(&build_body(&self.log_group, now_ms - 60_000, now_ms, None))
                .map_err(|e| Error::internal(format!("cloudwatch body: {e}")))?;

        body["limit"] = Value::from(1);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| Error::internal(format!("cloudwatch client: {e}")))?;

        self.filter_log_events(&client, body.to_string()).await?;

        Ok(())
    }

    async fn filter_log_events(&self, client: &reqwest::Client, body: String) -> Result<Value> {
        let credentials = self
            .credentials
            .as_ref()
            .map_err(|reason| Error::invalid(reason.clone()))?;

        let credential = credentials
            .get_credential()
            .await
            .map_err(|e| Error::unauthorized(format!("aws credentials: {e}")))?;

        let host = aws::logs_host(&self.region);
        let headers = [
            ("content-type", "application/x-amz-json-1.1"),
            ("x-amz-target", TARGET),
        ];

        let signed = sigv4::sign_post(
            &SigningKey {
                access_key: &credential.key_id,
                secret_key: &credential.secret_key,
                session_token: credential.token.as_deref(),
            },
            &self.region,
            "logs",
            &host,
            &headers,
            body.as_bytes(),
            &sigv4::amz_date_now(),
        );

        let mut req = client
            .post(format!("https://{host}/"))
            .header("content-type", headers[0].1)
            .header("x-amz-target", TARGET)
            .header("x-amz-date", &signed.amz_date)
            .header("authorization", &signed.authorization);

        if let Some(tok) = credential.token.as_deref().filter(|t| !t.is_empty()) {
            req = req.header("x-amz-security-token", tok);
        }

        let resp = crate::shared::http_trace::send(
            client,
            req.body(body),
            crate::shared::http_trace::HttpTarget::ThirdParty,
        )
        .await
        .map_err(|e| Error::unavailable(format!("cloudwatch request: {e}")))?;

        let status = resp.status();

        let text = resp
            .text()
            .await
            .map_err(|e| Error::unavailable(format!("cloudwatch body: {e}")))?;

        if !status.is_success() {
            return Err(refusal(status.as_u16(), &text));
        }

        serde_json::from_str(&text).map_err(|e| Error::internal(format!("cloudwatch json: {e}")))
    }
}

/// A refused `FilterLogEvents` as an error, by the `__type` AWS gave.
fn refusal(status: u16, text: &str) -> Error {
    let message = format!("cloudwatch FilterLogEvents {status}: {text}");

    let kind = serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|body| {
            body.get("__type")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();

    if kind.ends_with("ResourceNotFoundException") {
        Error::not_found(message)
    } else if matches!(status, 400 | 403)
        && (kind.contains("AccessDenied")
            || kind.contains("UnrecognizedClient")
            || kind.contains("InvalidSignature")
            || kind.contains("ExpiredToken"))
    {
        Error::forbidden(message)
    } else {
        Error::unavailable(message)
    }
}

fn build_body(log_group: &str, start_ms: i64, end_ms: i64, next_token: Option<&str>) -> String {
    let mut o = serde_json::json!({
        "logGroupName": log_group,
        "startTime": start_ms,
        "endTime": end_ms,
        "limit": 10_000,
    });

    if let Some(t) = next_token {
        o["nextToken"] = Value::String(t.to_string());
    }

    o.to_string()
}

fn event_to_raw(e: &Value, log_group: &str) -> RawEvent {
    let ts = e
        .get("timestamp")
        .and_then(|v| v.as_i64())
        .map(|ms| TimestampMicros(ms * 1_000))
        .unwrap_or_else(TimestampMicros::now);

    let mut m = Map::new();

    if let Some(msg) = e.get("message").and_then(|v| v.as_str()) {
        m.insert("message".to_string(), Value::String(msg.to_string()));
    }

    if let Some(ls) = e.get("logStreamName").and_then(|v| v.as_str()) {
        m.insert("log_stream".to_string(), Value::String(ls.to_string()));
    }

    m.insert(
        "log_group".to_string(),
        Value::String(log_group.to_string()),
    );

    m.insert(
        "source".to_string(),
        Value::String("cloudwatch".to_string()),
    );

    RawEvent {
        timestamp: ts,
        fields: m,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_body_includes_window_and_optional_token() {
        let b = build_body("/aws/lambda/fn", 1000, 2000, None);
        assert!(b.contains("\"logGroupName\":\"/aws/lambda/fn\""));
        assert!(b.contains("\"startTime\":1000"));
        assert!(b.contains("\"endTime\":2000"));
        assert!(!b.contains("nextToken"));
        let b2 = build_body("g", 0, 1, Some("tok123"));
        assert!(b2.contains("\"nextToken\":\"tok123\""));
    }

    #[test]
    fn event_to_raw_maps_fields_and_ms_to_micros() {
        let e = serde_json::json!({
            "timestamp": 1_700_000_000_000_i64,
            "message": "hello",
            "logStreamName": "stream-a",
        });
        let ev = event_to_raw(&e, "/aws/lambda/fn");
        assert_eq!(ev.timestamp.0, 1_700_000_000_000_000); // ms → μs
        assert_eq!(
            ev.fields.get("message").and_then(|v| v.as_str()),
            Some("hello")
        );
        assert_eq!(
            ev.fields.get("log_stream").and_then(|v| v.as_str()),
            Some("stream-a")
        );
        assert_eq!(
            ev.fields.get("log_group").and_then(|v| v.as_str()),
            Some("/aws/lambda/fn")
        );
        assert_eq!(
            ev.fields.get("source").and_then(|v| v.as_str()),
            Some("cloudwatch")
        );
    }

    #[test]
    fn a_refusal_reads_by_the_type_aws_gave() {
        let missing = r#"{"__type":"ResourceNotFoundException","message":"The specified log group does not exist."}"#;
        let denied = r#"{"__type":"AccessDeniedException","Message":"User is not authorized"}"#;
        let unknown_key = r#"{"__type":"UnrecognizedClientException","message":"The security token included in the request is invalid."}"#;

        assert!(matches!(refusal(400, missing), Error::NotFound(_)));
        assert!(matches!(refusal(400, denied), Error::Forbidden(_)));
        assert!(matches!(refusal(400, unknown_key), Error::Forbidden(_)));
        assert!(matches!(refusal(503, "unavailable"), Error::Unavailable(_)));
    }
}
