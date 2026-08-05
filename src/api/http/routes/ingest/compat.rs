// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 兼容协议接入：Elasticsearch `_bulk` 与 Loki push。
//!
//! - `POST /api/v1/_bulk` — Elasticsearch bulk NDJSON（Filebeat / Logstash / Vector ES sink）
//! - `POST /api/v1/loki/api/v1/push` — Loki push JSON（Promtail / Vector Loki sink）
//!
//! 与原生接入一致用 `Authorization: Bearer`（JWT 或 API token），经鉴权中间件拿
//! `IamContext`。body 带 `Content-Encoding: gzip` 时自动解压。解析出的事件统一走
//! [`IngestService`]（计费门禁 + schema-on-write + pipeline + sink）。

use std::{collections::BTreeMap, io::Read as _};

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header::CONTENT_ENCODING},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Map, Value};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::permission,
        ingestion::{IngestBatch, RawEvent},
        stream::StreamType,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/_bulk", post(es_bulk))
        .route("/loki/api/v1/push", post(loki_push))
}

/// body 解码：`Content-Encoding: gzip` 时解压，否则原样。
fn decode_body(headers: &HeaderMap, body: &Bytes) -> Result<Vec<u8>> {
    let gzipped = headers
        .get(CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_ascii_lowercase().contains("gzip"))
        .unwrap_or(false);
    if gzipped {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(body.as_ref())
            .read_to_end(&mut out)
            .map_err(|e| Error::invalid(format!("gzip decode: {e}")))?;
        Ok(out)
    } else {
        Ok(body.to_vec())
    }
}

/// `stream-name` 头，缺省回退。
fn stream_name(headers: &HeaderMap, default: &str) -> String {
    headers
        .get("stream-name")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(default)
        .to_string()
}

/// 把一批事件写入指定 stream（计费门禁 + ingest）。
async fn ingest_into(
    state: &AppState,
    org_id: &Id,
    stream: String,
    events: Vec<RawEvent>,
    body_len: usize,
) -> Result<usize> {
    if events.is_empty() {
        return Ok(0);
    }
    crate::api::http::billing::ensure_ingest_allowed(
        state,
        org_id,
        body_len as u64,
        TimestampMicros::now().0,
    )
    .await?;
    let batch = IngestBatch {
        batch_id: Id::new(),
        org_id: org_id.clone(),
        stream,
        stream_type: StreamType::Logs,
        events,
        received_at: TimestampMicros::now(),
    };
    Ok(state.ingestion.ingest(batch).await?.accepted)
}

// ===== Elasticsearch _bulk =====

/// 解析 ES bulk NDJSON：action 行 + 可选 doc 行交替，按目标 stream 分组事件。
///
/// 支持 `index` / `create`（带 doc 行）；`delete` 无 doc 行、跳过；`update` 取其 `doc` 字段。
/// `_index` 决定目标 stream，缺省回退到 `default_stream`。
fn parse_es_bulk(text: &str, default_stream: &str) -> Result<BTreeMap<String, Vec<RawEvent>>> {
    let mut by_stream: BTreeMap<String, Vec<RawEvent>> = BTreeMap::new();
    let mut lines = text.lines();
    while let Some(action_line) = lines.next() {
        let action_line = action_line.trim();
        if action_line.is_empty() {
            continue;
        }
        let Ok(Value::Object(action)) = serde_json::from_str::<Value>(action_line) else {
            return Err(Error::invalid("es bulk: action line is not a JSON object"));
        };
        // action 形如 {"index":{"_index":"x"}} / {"create":{...}} / {"delete":{...}} / {"update":{...}}
        let (op, meta) = action.into_iter().next().unwrap_or_default();
        let target = meta
            .get("_index")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| default_stream.to_string());
        match op.as_str() {
            "delete" => continue, // 无 doc 行、无操作
            "index" | "create" | "update" => {
                let Some(doc_line) = lines.next() else { break };
                let Ok(Value::Object(mut doc)) = serde_json::from_str::<Value>(doc_line.trim())
                else {
                    return Err(Error::invalid(
                        "es bulk: document line is not a JSON object",
                    ));
                };
                // update 的载荷在 `doc` 字段里。
                if op == "update"
                    && let Some(Value::Object(inner)) = doc.remove("doc")
                {
                    doc = inner;
                }
                let timestamp = doc
                    .remove("_timestamp")
                    .and_then(|v| v.as_i64())
                    .map(TimestampMicros)
                    .unwrap_or_else(TimestampMicros::now);
                by_stream.entry(target).or_default().push(RawEvent {
                    timestamp,
                    fields: doc,
                });
            }
            other => return Err(Error::invalid(format!("es bulk: unknown action `{other}`"))),
        }
    }
    Ok(by_stream)
}

#[permission("streams.write")]
async fn es_bulk(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    let raw = decode_body(&headers, &body)?;
    let text = String::from_utf8_lossy(&raw);
    let default_stream = stream_name(&headers, "default");
    let by_stream = parse_es_bulk(&text, &default_stream)?;

    let accepted: usize = by_stream.values().map(Vec::len).sum();
    for (stream, events) in by_stream {
        ingest_into(&state, &ctx.org_id, stream, events, body.len()).await?;
    }

    // ES 客户端（Filebeat 等）按 `errors` + `items[].status` 判定成功。
    let items: Vec<Value> = (0..accepted)
        .map(|_| serde_json::json!({"index": {"status": 201}}))
        .collect();
    Ok(Json(serde_json::json!({"took": 0, "errors": false, "items": items})).into_response())
}

// ===== Loki push =====

/// 解析 Loki push JSON：`{"streams":[{"stream":{labels},"values":[["<unix_nano>","<line>"], ...]}]}`。
/// 每个 value 成为一条事件（`_timestamp` 由 ns→μs，`message` 为行内容，labels 作字段）。
fn parse_loki(payload: &Value) -> Result<Vec<RawEvent>> {
    let streams = payload
        .get("streams")
        .and_then(|v| v.as_array())
        .ok_or_else(|| Error::invalid("loki: missing `streams` array"))?;
    let mut events: Vec<RawEvent> = Vec::new();
    for s in streams {
        // labels：作为公共字段附加到该 stream 的每条事件。
        let mut base = Map::new();
        if let Some(Value::Object(labels)) = s.get("stream") {
            for (k, v) in labels {
                base.insert(k.clone(), v.clone());
            }
        }
        let Some(values) = s.get("values").and_then(|v| v.as_array()) else {
            continue;
        };
        for entry in values {
            let Some(pair) = entry.as_array() else {
                continue;
            };
            let ts = pair
                .first()
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<i64>().ok())
                .map(|ns| TimestampMicros(ns / 1_000))
                .unwrap_or_else(TimestampMicros::now);
            let line = pair.get(1).and_then(|v| v.as_str()).unwrap_or_default();
            let mut fields = base.clone();
            fields.insert("message".to_string(), Value::String(line.to_string()));
            events.push(RawEvent {
                timestamp: ts,
                fields,
            });
        }
    }
    Ok(events)
}

#[permission("streams.write")]
async fn loki_push(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode> {
    let raw = decode_body(&headers, &body)?;
    let payload: Value =
        serde_json::from_slice(&raw).map_err(|e| Error::invalid(format!("loki body: {e}")))?;
    let events = parse_loki(&payload)?;
    let stream = stream_name(&headers, "default");
    ingest_into(&state, &ctx.org_id, stream, events, body.len()).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn es_bulk_groups_by_index_and_handles_ops() {
        let body = "\
{\"index\":{\"_index\":\"app\"}}
{\"level\":\"info\",\"msg\":\"a\"}
{\"create\":{\"_index\":\"app\"}}
{\"msg\":\"b\"}
{\"delete\":{\"_index\":\"app\",\"_id\":\"1\"}}
{\"index\":{\"_index\":\"web\"}}
{\"msg\":\"c\"}
{\"update\":{\"_index\":\"web\"}}
{\"doc\":{\"msg\":\"d\"}}
";
        let grouped = parse_es_bulk(body, "default").unwrap();
        assert_eq!(
            grouped.get("app").map(Vec::len),
            Some(2),
            "index+create, delete skipped"
        );
        assert_eq!(
            grouped.get("web").map(Vec::len),
            Some(2),
            "index + update(doc)"
        );
        // update 的 doc 字段被解包
        let web = &grouped["web"];
        assert_eq!(web[1].fields.get("msg").and_then(|v| v.as_str()), Some("d"));
    }

    #[test]
    fn es_bulk_falls_back_to_default_stream_and_timestamp() {
        let body = "{\"index\":{}}\n{\"_timestamp\":1700000000000000,\"msg\":\"x\"}\n";
        let grouped = parse_es_bulk(body, "fallback").unwrap();
        let ev = &grouped["fallback"][0];
        assert_eq!(
            ev.timestamp.0, 1700000000000000,
            "_timestamp honored + removed from fields"
        );
        assert!(ev.fields.get("_timestamp").is_none());
        assert_eq!(ev.fields.get("msg").and_then(|v| v.as_str()), Some("x"));
    }

    #[test]
    fn loki_maps_values_with_labels_and_ns_timestamp() {
        let payload = json!({
            "streams": [
                {
                    "stream": {"service": "checkout", "level": "error"},
                    "values": [
                        ["1700000000000000000", "boom"],
                        ["1700000001000000000", "again"]
                    ]
                }
            ]
        });
        let events = parse_loki(&payload).unwrap();
        assert_eq!(events.len(), 2);
        // 1.7e18 ns → 1.7e15 μs
        assert_eq!(events[0].timestamp.0, 1_700_000_000_000_000);
        assert_eq!(
            events[0].fields.get("message").and_then(|v| v.as_str()),
            Some("boom")
        );
        assert_eq!(
            events[0].fields.get("service").and_then(|v| v.as_str()),
            Some("checkout")
        );
        assert_eq!(
            events[1].fields.get("level").and_then(|v| v.as_str()),
            Some("error")
        );
    }

    #[test]
    fn loki_rejects_missing_streams() {
        assert!(parse_loki(&json!({})).is_err());
    }
}
