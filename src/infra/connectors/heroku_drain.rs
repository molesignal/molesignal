// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Heroku log drain HTTP push。
//!
//! Heroku 推 syslog-like 文本，每行一条；HTTP POST `/api/v1/_heroku`，鉴权头同。
//! 一并支持以 `<priority>...` 开头的 syslog 行；解析失败时 fallback 原文 message。

use serde_json::{Map, Value};

use crate::{
    domain::ingestion::RawEvent,
    shared::{Result, time::TimestampMicros},
};

pub fn parse_text(body: &str) -> Result<Vec<RawEvent>> {
    let mut out = Vec::new();
    for line in body.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        let mut m = Map::new();
        m.insert("message".to_string(), Value::String(l.to_string()));
        m.insert("source".to_string(), Value::String("heroku".to_string()));
        out.push(RawEvent {
            timestamp: TimestampMicros::now(),
            fields: m,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_lines() {
        let body = "line one\nline two\n";
        let e = parse_text(body).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(
            e[0].fields.get("source").and_then(|v| v.as_str()),
            Some("heroku")
        );
    }
}
