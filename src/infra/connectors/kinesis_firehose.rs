// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Kinesis Firehose 协议解析。
//!
//! Firehose 推送格式：
//! ```json
//! { "requestId": "...",
//!   "timestamp": 123,
//!   "records": [ { "data": "base64(...)" }, ... ] }
//! ```
//! 把每条 `data` base64 解 → 按 newline 切 → 每行作为一条事件（JSON or text）。
//!
//! HTTP receiver 在 `src/api/http/routes/ingest_kinesis.rs` 里调本模块的
//! [`parse_firehose_request`]。

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::{
    domain::ingestion::RawEvent,
    shared::{Error, Result, time::TimestampMicros},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirehoseRequest {
    /// Firehose 发的是 `requestId`（camelCase）；HTTP 响应需原样回显。
    #[serde(default)]
    pub request_id: String,
    #[serde(default)]
    pub timestamp: i64,
    pub records: Vec<FirehoseRecord>,
}

#[derive(Debug, Deserialize)]
pub struct FirehoseRecord {
    pub data: String,
}

pub fn parse_firehose_request(req: FirehoseRequest) -> Result<Vec<RawEvent>> {
    use base64::Engine as _;
    let mut out = Vec::with_capacity(req.records.len());
    for r in req.records {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(r.data.as_bytes())
            .map_err(|e| Error::invalid(format!("firehose record b64: {e}")))?;
        let s = String::from_utf8_lossy(&bytes);
        for line in s.lines() {
            let l = line.trim();
            if l.is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(l).unwrap_or_else(|_| {
                let mut m = Map::new();
                m.insert("message".to_string(), Value::String(l.to_string()));
                Value::Object(m)
            });
            let map = match v {
                Value::Object(m) => m,
                other => {
                    let mut m = Map::new();
                    m.insert("payload".to_string(), other);
                    m
                }
            };
            out.push(RawEvent {
                timestamp: TimestampMicros::now(),
                fields: map,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;

    use super::*;

    #[test]
    fn parses_json_and_text_lines() {
        let json_data = base64::engine::general_purpose::STANDARD
            .encode("{\"level\":\"info\",\"msg\":\"hi\"}\nplain text line\n");
        let req = FirehoseRequest {
            request_id: "r".into(),
            timestamp: 0,
            records: vec![FirehoseRecord { data: json_data }],
        };
        let events = parse_firehose_request(req).unwrap();
        assert_eq!(events.len(), 2);
        assert!(events[0].fields.contains_key("level"));
        assert!(events[1].fields.contains_key("message"));
    }
}
