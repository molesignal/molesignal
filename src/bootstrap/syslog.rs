// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 裸 syslog 接入（RFC3164 / RFC5424）：UDP datagram 与 TCP（换行分帧）监听器。
//!
//! syslog **无鉴权、无 org 上下文** —— 与 OTLP/native（带 Bearer）不同，必须在 `[syslog]`
//! 配置里显式绑定到一个 org（slug）+ 目标 stream。监听器把每行解析成 [`RawEvent`]
//! （提取 PRI→facility/severity、hostname、app_name、message、structured_data），整批走
//! [`IngestService`]（schema-on-write + pipeline + sink）。计费门禁不适用（非 HTTP 路径）。

use std::sync::Arc;

use serde_json::{Map, Value};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::{TcpListener, UdpSocket},
    task::JoinHandle,
};

use crate::{
    app::ingestion::IngestService,
    domain::{
        ingestion::{IngestBatch, RawEvent},
        stream::StreamType,
    },
    shared::{ids::Id, time::TimestampMicros},
};

/// syslog 监听器：按配置起 UDP / TCP，把解析出的事件写入绑定的 org+stream。
pub struct SyslogListener {
    ingest: Arc<IngestService>,
    org_id: Id,
    stream: String,
    udp_bind: String,
    tcp_bind: String,
}

impl SyslogListener {
    pub fn new(
        ingest: Arc<IngestService>,
        org_id: Id,
        stream: String,
        udp_bind: String,
        tcp_bind: String,
    ) -> Self {
        let stream = if stream.trim().is_empty() {
            "syslog".to_string()
        } else {
            stream
        };
        Self {
            ingest,
            org_id,
            stream,
            udp_bind,
            tcp_bind,
        }
    }

    /// 起监听任务（配置了哪个就起哪个）。返回 JoinHandle（tokio task 与 handle 解耦，drop 不取消）。
    pub fn spawn(self) -> Vec<JoinHandle<()>> {
        let mut handles = Vec::new();
        if !self.udp_bind.trim().is_empty() {
            let (i, o, s, b) = (
                self.ingest.clone(),
                self.org_id.clone(),
                self.stream.clone(),
                self.udp_bind.clone(),
            );
            handles.push(tokio::spawn(async move { run_udp(i, o, s, b).await }));
        }
        if !self.tcp_bind.trim().is_empty() {
            let (i, o, s, b) = (
                self.ingest.clone(),
                self.org_id.clone(),
                self.stream.clone(),
                self.tcp_bind.clone(),
            );
            handles.push(tokio::spawn(async move { run_tcp(i, o, s, b).await }));
        }
        handles
    }
}

async fn ingest_events(ingest: &IngestService, org_id: &Id, stream: &str, events: Vec<RawEvent>) {
    if events.is_empty() {
        return;
    }
    let batch = IngestBatch {
        batch_id: Id::new(),
        org_id: org_id.clone(),
        stream: stream.to_string(),
        stream_type: StreamType::Logs,
        events,
        received_at: TimestampMicros::now(),
    };
    if let Err(e) = ingest.ingest(batch).await {
        tracing::warn!(error = %e, "syslog ingest failed");
    }
}

async fn run_udp(ingest: Arc<IngestService>, org_id: Id, stream: String, bind: String) {
    let sock = match UdpSocket::bind(&bind).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, %bind, "syslog udp bind failed");
            return;
        }
    };
    tracing::info!(%bind, org = %org_id.0, stream = %stream, "syslog udp listening");
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match sock.recv_from(&mut buf).await {
            Ok((n, _peer)) => {
                let text = String::from_utf8_lossy(&buf[..n]);
                let events: Vec<RawEvent> = text
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(parse_syslog_line)
                    .collect();
                ingest_events(&ingest, &org_id, &stream, events).await;
            }
            Err(e) => tracing::warn!(error = %e, "syslog udp recv"),
        }
    }
}

async fn run_tcp(ingest: Arc<IngestService>, org_id: Id, stream: String, bind: String) {
    let listener = match TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, %bind, "syslog tcp bind failed");
            return;
        }
    };
    tracing::info!(%bind, org = %org_id.0, stream = %stream, "syslog tcp listening");
    loop {
        match listener.accept().await {
            Ok((sock, _peer)) => {
                let (i, o, s) = (ingest.clone(), org_id.clone(), stream.clone());
                tokio::spawn(async move { handle_tcp_conn(i, o, s, sock).await });
            }
            Err(e) => tracing::warn!(error = %e, "syslog tcp accept"),
        }
    }
}

/// 单连接：换行分帧（RFC6587 octet-counting 不支持）。批量满 256 行 flush，EOF 时 flush 余量。
async fn handle_tcp_conn(
    ingest: Arc<IngestService>,
    org_id: Id,
    stream: String,
    sock: tokio::net::TcpStream,
) {
    let mut reader = BufReader::new(sock);
    let mut batch: Vec<RawEvent> = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF
            Ok(_) => {
                let l = line.trim_end_matches(['\r', '\n']);
                if !l.trim().is_empty() {
                    batch.push(parse_syslog_line(l));
                }
                if batch.len() >= 256 {
                    ingest_events(&ingest, &org_id, &stream, std::mem::take(&mut batch)).await;
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "syslog tcp read");
                break;
            }
        }
    }
    ingest_events(&ingest, &org_id, &stream, batch).await;
}

// ===== 解析 =====

fn severity_text(sev: u8) -> &'static str {
    match sev {
        0 => "emerg",
        1 => "alert",
        2 => "crit",
        3 => "err",
        4 => "warning",
        5 => "notice",
        6 => "info",
        _ => "debug",
    }
}

/// 解析一条 syslog 行为 [`RawEvent`]。识别 `<PRI>`，再尝试 RFC5424（`1 ...`），否则按
/// RFC3164 best-effort（BSD 时间戳 + host + tag），都不成则整行作 `message`。
pub fn parse_syslog_line(line: &str) -> RawEvent {
    let line = line.trim();
    let mut fields = Map::new();
    fields.insert("source".to_string(), Value::String("syslog".to_string()));
    let mut ts = TimestampMicros::now();
    let mut rest = line;

    // <PRI> = facility*8 + severity
    if let Some(after) = rest.strip_prefix('<')
        && let Some(gt) = after.find('>')
        && let Ok(pri) = after[..gt].parse::<u16>()
    {
        fields.insert("facility".to_string(), Value::from(pri / 8));
        let sev = (pri % 8) as u8;
        fields.insert("severity".to_string(), Value::from(sev));
        fields.insert(
            "severity_text".to_string(),
            Value::String(severity_text(sev).to_string()),
        );
        rest = after[gt + 1..].trim_start();
    }

    // RFC5424: "1 TIMESTAMP HOST APP PROCID MSGID SD MSG"
    if let Some(body) = rest.strip_prefix("1 ")
        && let Some(message) = parse_rfc5424(body, &mut fields, &mut ts)
    {
        fields.insert("message".to_string(), Value::String(message));
        return RawEvent {
            timestamp: ts,
            fields,
        };
    }

    // RFC3164 best-effort
    let message = parse_rfc3164(rest, &mut fields);
    fields.insert("message".to_string(), Value::String(message));
    RawEvent {
        timestamp: ts,
        fields,
    }
}

fn parse_iso(s: &str) -> Option<TimestampMicros> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| TimestampMicros(dt.timestamp_micros()))
}

fn parse_rfc5424(
    body: &str,
    fields: &mut Map<String, Value>,
    ts: &mut TimestampMicros,
) -> Option<String> {
    let mut it = body.splitn(6, ' ');
    let timestamp = it.next()?;
    let host = it.next()?;
    let app = it.next()?;
    let _procid = it.next()?;
    let _msgid = it.next()?;
    let remainder = it.next().unwrap_or("").trim_start();

    if timestamp != "-"
        && let Some(t) = parse_iso(timestamp)
    {
        *ts = t;
    }
    if host != "-" && !host.is_empty() {
        fields.insert("hostname".to_string(), Value::String(host.to_string()));
    }
    if app != "-" && !app.is_empty() {
        fields.insert("app_name".to_string(), Value::String(app.to_string()));
    }

    // STRUCTURED-DATA：`-`（无）或一串 `[...]`（值里的 `]` 用 `\]` 转义）。
    let message = if let Some(after) = remainder.strip_prefix('-') {
        after.trim_start().to_string()
    } else if remainder.starts_with('[') {
        let bytes = remainder.as_bytes();
        let mut pos = 0;
        while pos < bytes.len() && bytes[pos] == b'[' {
            let mut i = pos + 1;
            while i < bytes.len() {
                match bytes[i] {
                    b'\\' => i += 2,
                    b']' => break,
                    _ => i += 1,
                }
            }
            if i >= bytes.len() {
                break;
            }
            pos = i + 1;
        }
        if pos > 0 {
            fields.insert(
                "structured_data".to_string(),
                Value::String(remainder[..pos].to_string()),
            );
        }
        remainder[pos..].trim_start().to_string()
    } else {
        remainder.to_string()
    };
    Some(message)
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn parse_rfc3164(rest: &str, fields: &mut Map<String, Value>) -> String {
    let is_bsd = rest.len() >= 15
        && rest.is_char_boundary(15)
        && MONTHS.iter().any(|m| rest.starts_with(m))
        && rest.as_bytes().get(3) == Some(&b' ');
    if !is_bsd {
        return rest.to_string();
    }
    let (tsr, after) = rest.split_at(15);
    fields.insert(
        "timestamp_text".to_string(),
        Value::String(tsr.trim().to_string()),
    );
    let after = after.trim_start();
    let Some((host, remainder)) = after.split_once(' ') else {
        return after.to_string();
    };
    if !host.is_empty() {
        fields.insert("hostname".to_string(), Value::String(host.to_string()));
    }
    // TAG[: ]MSG —— TAG 可能是 "app[pid]"。
    if let Some((tag, msg)) = remainder.split_once(':') {
        let app = tag.split('[').next().unwrap_or(tag).trim();
        if !app.is_empty() && app.len() <= 48 && !app.contains(' ') {
            fields.insert("app_name".to_string(), Value::String(app.to_string()));
            return msg.trim_start().to_string();
        }
    }
    remainder.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rfc5424_with_pri_and_fields() {
        let ev = parse_syslog_line(
            "<34>1 2003-10-11T22:14:15.003Z mymachine.example.com su - ID47 - 'su root' failed",
        );
        let f = &ev.fields;
        assert_eq!(f.get("facility").and_then(|v| v.as_u64()), Some(4)); // 34/8
        assert_eq!(f.get("severity").and_then(|v| v.as_u64()), Some(2)); // 34%8
        assert_eq!(
            f.get("severity_text").and_then(|v| v.as_str()),
            Some("crit")
        );
        assert_eq!(
            f.get("hostname").and_then(|v| v.as_str()),
            Some("mymachine.example.com")
        );
        assert_eq!(f.get("app_name").and_then(|v| v.as_str()), Some("su"));
        assert_eq!(
            f.get("message").and_then(|v| v.as_str()),
            Some("'su root' failed")
        );
        // 2003-10-11T22:14:15.003Z → micros
        assert_eq!(ev.timestamp.0, 1_065_910_455_003_000);
    }

    #[test]
    fn parses_rfc5424_structured_data() {
        let ev = parse_syslog_line(
            r#"<165>1 2003-10-11T22:14:15.003Z host evntslog - ID47 [exampleSDID@32473 iut="3"] an application event"#,
        );
        let f = &ev.fields;
        assert_eq!(
            f.get("structured_data").and_then(|v| v.as_str()),
            Some(r#"[exampleSDID@32473 iut="3"]"#)
        );
        assert_eq!(
            f.get("message").and_then(|v| v.as_str()),
            Some("an application event")
        );
    }

    #[test]
    fn parses_rfc3164_bsd() {
        let ev = parse_syslog_line("<13>Oct 11 22:14:15 mymachine su: 'su root' failed");
        let f = &ev.fields;
        assert_eq!(f.get("severity").and_then(|v| v.as_u64()), Some(5)); // 13%8 notice
        assert_eq!(
            f.get("severity_text").and_then(|v| v.as_str()),
            Some("notice")
        );
        assert_eq!(
            f.get("timestamp_text").and_then(|v| v.as_str()),
            Some("Oct 11 22:14:15")
        );
        assert_eq!(
            f.get("hostname").and_then(|v| v.as_str()),
            Some("mymachine")
        );
        assert_eq!(f.get("app_name").and_then(|v| v.as_str()), Some("su"));
        assert_eq!(
            f.get("message").and_then(|v| v.as_str()),
            Some("'su root' failed")
        );
    }

    #[test]
    fn plain_line_without_pri_falls_back_to_message() {
        let ev = parse_syslog_line("just a plain log line");
        let f = &ev.fields;
        assert_eq!(
            f.get("message").and_then(|v| v.as_str()),
            Some("just a plain log line")
        );
        assert_eq!(f.get("source").and_then(|v| v.as_str()), Some("syslog"));
        assert!(f.get("severity").is_none());
    }
}
