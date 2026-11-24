// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 兼容协议接入：Elasticsearch `_bulk` 与 Loki push。
//!
//! - `POST /api/v1/_bulk` — Elasticsearch bulk NDJSON（Filebeat / Logstash / Vector ES sink）
//! - `POST /api/v1/loki/api/v1/push` — Loki push JSON（Promtail / Vector Loki sink）
//!
//! 与原生接入一致用 `Authorization: Bearer`（JWT 或 API token），经鉴权中间件拿
//! `IamContext`。body 带 `Content-Encoding: gzip` 时自动解压。解析出的事件统一走

use std::{collections::BTreeMap, io::Read as _};

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header::CONTENT_ENCODING},
    response::{IntoResponse, Response},
    routing::post,
};
use domain::{
    iam::{IamContext, permission},
    intake::{IntakeBatch, IntakeResult, RawEvent, usage::source::IntakeCredential},
    stream::StreamType,
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde_json::{Map, Value};

use crate::{
    api::AppState,
    app::intake::usage::{IntakeReceipt, split_by_events},
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

/// One body's events by the stream each goes to, each with its position in the body.
pub(super) type ByStream = BTreeMap<String, Vec<(usize, RawEvent)>>;

/// An event of a body that its stream refused: a value that does not fit its field's type.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Refused {
    /// The event's position in the body.
    pub position: usize,
    pub reason: String,
}

pub(super) async fn intake_into(
    state: &AppState,
    ctx: &IamContext,
    stream: String,
    events: Vec<RawEvent>,
    bytes: u64,
) -> Result<IntakeResult> {
    if events.is_empty() {
        return Ok(IntakeResult {
            accepted: 0,
            rejected: 0,
            errors: Vec::new(),
        });
    }

    let receipt = IntakeReceipt::of_events(
        IntakeCredential::of(ctx),
        StreamType::LOGS,
        &stream,
        bytes,
        &events,
    );

    let batch = IntakeBatch {
        batch_id: Id::new(),
        org_id: ctx.org_id.clone(),
        stream,
        stream_type: StreamType::LOGS,
        events,
        received_at: TimestampMicros::now(),
    };

    let result = state.intake.intake(batch).await?;

    state
        .platform
        .intake_meter
        .record(&ctx.org_id, receipt, result.accepted as u64);

    Ok(result)
}

/// Writes each stream's events, the body's `bytes` shared among the streams by how many events
/// each has, and returns the events refused, in body order.
pub(super) async fn write_streams(
    state: &AppState,
    ctx: &IamContext,
    by_stream: ByStream,
    bytes: u64,
) -> Result<Vec<Refused>> {
    let counts = by_stream
        .values()
        .map(|events| events.len() as u64)
        .collect::<Vec<_>>();

    let shares = split_by_events(bytes, &counts);
    let mut refused = Vec::new();

    for ((stream, placed), bytes) in by_stream.into_iter().zip(shares) {
        let (positions, events): (Vec<usize>, Vec<RawEvent>) = placed.into_iter().unzip();
        let result = intake_into(state, ctx, stream, events, bytes).await?;

        refused.extend(result.errors.into_iter().map(|error| Refused {
            position: positions[error.index],
            reason: error.reason,
        }));
    }

    refused.sort_by_key(|refused| refused.position);
    Ok(refused)
}

/// What a sender is told when some of the `total` `records` of its body were refused: how many,
/// and the first and why.
pub(super) fn refusal_message(refused: &[Refused], total: usize, records: &str) -> String {
    match refused.first() {
        Some(first) => format!(
            "{} of {total} {records} rejected; the first, at position {}: {}",
            refused.len(),
            first.position,
            first.reason
        ),
        None => format!("0 of {total} {records} rejected"),
    }
}

// ===== Elasticsearch _bulk =====

/// 解析 ES bulk NDJSON：action 行 + 可选 doc 行交替，按目标 stream 分组事件。
///
/// 支持 `index` / `create`（带 doc 行）；`delete` 无 doc 行、跳过；`update` 取其 `doc` 字段。
/// `_index` 决定目标 stream，缺省回退到 `default_stream`。事件的位置是它在带文档的 action 中的
/// 序号，也就是它在应答 `items` 里的位置。
fn parse_es_bulk(text: &str, default_stream: &str) -> Result<ByStream> {
    let mut by_stream = ByStream::new();
    let mut documents = 0;
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

                by_stream.entry(target).or_default().push((
                    documents,
                    RawEvent {
                        timestamp,
                        fields: doc,
                    },
                ));
                documents += 1;
            }
            other => return Err(Error::invalid(format!("es bulk: unknown action `{other}`"))),
        }
    }

    Ok(by_stream)
}

#[permission("logs.write")]
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

    let documents: usize = by_stream.values().map(Vec::len).sum();
    let refused = write_streams(&state, &ctx, by_stream, raw.len() as u64).await?;

    Ok(Json(bulk_answer(documents, &refused)).into_response())
}

/// The `_bulk` answer for `documents` documents. ES clients (Filebeat and the like) read `errors`
/// and each item's `status`: a refused document is a 400 with its reason, which they do not retry
/// and report as lost.
fn bulk_answer(documents: usize, refused: &[Refused]) -> Value {
    let mut refusals = refused.iter().peekable();

    let items: Vec<Value> = (0..documents)
        .map(
            |position| match refusals.next_if(|refused| refused.position == position) {
                Some(refused) => serde_json::json!({"index": {
                    "status": 400,
                    "error": {"type": "mapper_parsing_exception", "reason": refused.reason},
                }}),
                None => serde_json::json!({"index": {"status": 201}}),
            },
        )
        .collect();

    serde_json::json!({"took": 0, "errors": !refused.is_empty(), "items": items})
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

#[permission("logs.write")]
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
    let total = events.len();
    let stream = stream_name(&headers, "default");
    let by_stream = ByStream::from([(stream, events.into_iter().enumerate().collect())]);
    let refused = write_streams(&state, &ctx, by_stream, raw.len() as u64).await?;

    // Lines refused for good are reported: Loki clients do not retry a 400.
    if !refused.is_empty() {
        return Err(Error::invalid(refusal_message(
            &refused,
            total,
            "log lines",
        )));
    }

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
        assert_eq!(
            web[1].1.fields.get("msg").and_then(|v| v.as_str()),
            Some("d")
        );

        // 位置按带文档的 action 计，跨 stream 连续，delete 不占位置。
        let positions: Vec<usize> = grouped
            .values()
            .flatten()
            .map(|(position, _)| *position)
            .collect();
        assert_eq!(positions, [0, 1, 2, 3]);
    }

    #[test]
    fn es_bulk_falls_back_to_default_stream_and_timestamp() {
        let body = "{\"index\":{}}\n{\"_timestamp\":1700000000000000,\"msg\":\"x\"}\n";
        let grouped = parse_es_bulk(body, "fallback").unwrap();
        let (_, ev) = &grouped["fallback"][0];
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

    #[test]
    fn a_refusal_says_how_many_and_the_first() {
        let refused = [
            Refused {
                position: 2,
                reason: "type mismatch on field `code`: expected Int64, got String".into(),
            },
            Refused {
                position: 5,
                reason: "type mismatch on field `code`: expected Int64, got Bool".into(),
            },
        ];

        assert_eq!(
            refusal_message(&refused, 7, "log lines"),
            "2 of 7 log lines rejected; the first, at position 2: type mismatch on field `code`: \
             expected Int64, got String"
        );
    }

    #[test]
    fn a_bulk_answer_marks_the_refused_documents() {
        let refused = [Refused {
            position: 1,
            reason: "type mismatch on field `code`: expected Int64, got String".into(),
        }];

        let answer = bulk_answer(3, &refused);

        assert_eq!(answer["errors"], true);
        assert_eq!(answer["items"][0]["index"]["status"], 201);
        assert_eq!(answer["items"][1]["index"]["status"], 400);
        assert_eq!(
            answer["items"][1]["index"]["error"]["reason"],
            refused[0].reason
        );
        assert_eq!(answer["items"][2]["index"]["status"], 201);
        assert_eq!(bulk_answer(2, &[])["errors"], false);
    }
}
