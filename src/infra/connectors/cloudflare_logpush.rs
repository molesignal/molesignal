// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cloudflare Logpush HTTP push 协议。
//!
//! Cloudflare 推 NDJSON 流（每行一条事件），HTTP POST 到 `/api/v1/_cloudflare`，
//! 受 `X-Connector-Token` 头鉴权。本模块只提供解析；HTTP 层调用。

use serde_json::{Map, Value};

use crate::{
    domain::ingestion::RawEvent,
    shared::{Result, time::TimestampMicros},
};

pub fn parse_ndjson(body: &str) -> Result<Vec<RawEvent>> {
    let mut out = Vec::new();
    for line in body.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(l) {
            Ok(v) => v,
            Err(_) => {
                let mut m = Map::new();
                m.insert("message".to_string(), Value::String(l.to_string()));
                Value::Object(m)
            }
        };
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
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ndjson_lines() {
        let body = "{\"a\":1}\n{\"a\":2}\n";
        let events = parse_ndjson(body).unwrap();
        assert_eq!(events.len(), 2);
    }
}
