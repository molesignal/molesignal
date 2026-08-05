// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! AWS CloudWatch Logs pull connector。
//!
//! 周期调 `FilterLogEvents`（AWS JSON 协议，SigV4 签名，不引 AWS SDK——见 [`super::sigv4`]）拉取
//! `[last_run, now]` 窗口内的日志，按 `nextToken` 翻页，转 [`RawEvent`] 交由 ConnectorRunner 写入。
//!
//! 配置（`config_json`）：`region` / `log_group` / `access_key` / `secret_key` /
//! 可选 `session_token` / `target_stream`。缺关键字段 → no-op（返回 0 条）。

use async_trait::async_trait;
use serde_json::{Map, Value};

use super::{PullConnector, PullContext, sigv4};
use crate::{
    domain::ingestion::RawEvent,
    shared::{Error, Result, time::TimestampMicros},
};

/// 默认回溯窗口（首次运行、无 last_run 时）：5 分钟。
const DEFAULT_LOOKBACK_MS: i64 = 5 * 60 * 1000;
/// 单次 run_once 的翻页上限（防异常时无限翻页）。
const MAX_PAGES: usize = 50;

pub struct CloudWatchLogsConnector {
    pub region: String,
    pub log_group: String,
    pub target_stream: String,
    pub access_key: String,
    pub secret_key: String,
    pub session_token: Option<String>,
}

#[async_trait]
impl PullConnector for CloudWatchLogsConnector {
    async fn run_once(&self, ctx: &PullContext) -> Result<Vec<RawEvent>> {
        if self.region.is_empty() || self.log_group.is_empty() || self.access_key.is_empty() {
            // 未配置完整 → 静默 no-op（CRUD 可先建、后补凭据）。
            return Ok(Vec::new());
        }
        let now_ms = TimestampMicros::now().0 / 1_000;
        let start_ms = ctx
            .last_run_at_micros
            .map(|m| m / 1_000)
            .unwrap_or(now_ms - DEFAULT_LOOKBACK_MS);

        let host = format!("logs.{}.amazonaws.com", self.region);
        let url = format!("https://{host}/");
        let client = reqwest::Client::new();

        let mut events = Vec::new();
        let mut next_token: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let body = build_body(&self.log_group, start_ms, now_ms, next_token.as_deref());
            let amz_date = utc_amz_date();
            let signed = sigv4::sign_post_json(
                &self.access_key,
                &self.secret_key,
                self.session_token.as_deref(),
                &self.region,
                "logs",
                &host,
                "Logs_20140328.FilterLogEvents",
                body.as_bytes(),
                &amz_date,
            );

            let mut req = client
                .post(&url)
                .header("content-type", "application/x-amz-json-1.1")
                .header("x-amz-target", "Logs_20140328.FilterLogEvents")
                .header("x-amz-date", &signed.amz_date)
                .header("authorization", &signed.authorization);
            if let Some(tok) = self.session_token.as_deref().filter(|t| !t.is_empty()) {
                req = req.header("x-amz-security-token", tok);
            }
            let resp = crate::shared::http_trace::send(
                &client,
                req.body(body),
                crate::shared::http_trace::HttpTarget::ThirdParty,
            )
            .await
            .map_err(|e| Error::internal(format!("cloudwatch request: {e}")))?;
            let status = resp.status();
            let text = resp
                .text()
                .await
                .map_err(|e| Error::internal(format!("cloudwatch body: {e}")))?;
            if !status.is_success() {
                return Err(Error::internal(format!(
                    "cloudwatch FilterLogEvents {status}: {text}"
                )));
            }
            let parsed: Value = serde_json::from_str(&text)
                .map_err(|e| Error::internal(format!("cloudwatch json: {e}")))?;
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
        Ok(events)
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

/// `YYYYMMDDTHHMMSSZ`（UTC）。
fn utc_amz_date() -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_micros(TimestampMicros::now().0)
        .map(|dt| dt.format("%Y%m%dT%H%M%SZ").to_string())
        .unwrap_or_else(|| "19700101T000000Z".to_string())
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
}
