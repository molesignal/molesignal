// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Push 型 connector 接入端点：第三方平台把日志直接 POST 进来。
//!
//! - `POST /api/v1/_kinesis_firehose` — AWS Kinesis Data Firehose HTTP endpoint
//! - `POST /api/v1/_cloudflare` — Cloudflare Logpush（NDJSON）
//! - `POST /api/v1/_heroku` — Heroku log drain（文本，每行一条）
//!
//! 这些端点**不走 JWT/API token**（外部平台带不了 `Authorization: Bearer`），而是用
//! 一个 connector token 自鉴权：反查 `kind` + `config_json.push_token` 命中的 enabled
//! connector，取其 `org_id` 与 `config_json.target_stream`。故路由在鉴权中间件白名单内。
//!
//! token 的来源因平台而异，三种都接受：
//! - `X-Connector-Token` 头（Cloudflare 等可配自定义头的平台）
//! - `X-Amz-Firehose-Access-Key` 头（Kinesis Firehose 的固定头）
//! - `?token=` 查询参数（Heroku 等无法设自定义头的平台）
//!
//! body 若带 `Content-Encoding: gzip`（Cloudflare Logpush 默认）则先解压。解析后统一走
//! [`IngestService`]（计费门禁 + schema-on-write + pipeline + sink），与其它接入同源。

use std::io::Read as _;

use axum::{
    Json, Router,
    body::Bytes,
    extract::{RawQuery, State},
    http::{HeaderMap, StatusCode, header::CONTENT_ENCODING},
    response::{IntoResponse, Response},
    routing::post,
};
use flate2::read::GzDecoder;

use crate::{
    api::AppState,
    domain::{
        ingestion::{IngestBatch, RawEvent},
        stream::StreamType,
    },
    infra::connectors::{
        Connector, ConnectorKind,
        cloudflare_logpush::parse_ndjson,
        heroku_drain::parse_text,
        kinesis_firehose::{FirehoseRequest, parse_firehose_request},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/_kinesis_firehose", post(kinesis_firehose))
        .route("/_cloudflare", post(cloudflare))
        .route("/_heroku", post(heroku))
}

/// 从请求里取 connector token：自定义头 / Firehose 头 / `?token=` 查询参数（按此优先级）。
fn token_from(headers: &HeaderMap, raw_query: Option<&str>) -> Option<String> {
    let header_token = headers
        .get("x-connector-token")
        .or_else(|| headers.get("x-amz-firehose-access-key"))
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let query_token = raw_query.and_then(|q| {
        q.split('&')
            .find_map(|kv| kv.strip_prefix("token="))
            .map(str::trim)
            .filter(|s| !s.is_empty())
    });
    header_token.or(query_token).map(str::to_string)
}

/// 用 token 反查命中的 enabled push connector；缺 token / 不命中 → 401。
async fn resolve_connector(
    state: &AppState,
    kind: ConnectorKind,
    token: Option<String>,
) -> Result<Connector> {
    let token = token.ok_or_else(|| {
        Error::unauthorized("missing connector token (X-Connector-Token header or ?token=)")
    })?;
    state
        .storage
        .connectors
        .find_for_push(kind.as_str(), &token)
        .await?
        .ok_or_else(|| Error::unauthorized("connector token not recognized"))
}

/// 目标 stream：取 `config_json.target_stream`，缺省回退到 `default`。
fn target_stream(connector: &Connector, default: &str) -> String {
    connector
        .config_json
        .get("target_stream")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(default)
        .to_string()
}

/// body 解码：`Content-Encoding: gzip` 时解压，否则原样返回。
fn decode_body(headers: &HeaderMap, body: &Bytes) -> Result<Vec<u8>> {
    let gzipped = headers
        .get(CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_ascii_lowercase().contains("gzip"))
        .unwrap_or(false);
    if gzipped {
        let mut out = Vec::new();
        GzDecoder::new(body.as_ref())
            .read_to_end(&mut out)
            .map_err(|e| Error::invalid(format!("gzip decode: {e}")))?;
        Ok(out)
    } else {
        Ok(body.to_vec())
    }
}

/// 把解析出的事件喂进 ingest（连同计费门禁），并 best-effort 记 connector last_run。
async fn ingest_events(
    state: &AppState,
    connector: &Connector,
    stream: String,
    events: Vec<RawEvent>,
    body_len: usize,
) -> Result<usize> {
    state
        .iam
        .service
        .ensure_organization_access(&connector.org_id)
        .await?;
    // 计费门禁 + 计量：用 connector 所属 org（推送端无 IamContext）。
    crate::api::http::billing::ensure_ingest_allowed(
        state,
        &connector.org_id,
        body_len as u64,
        TimestampMicros::now().0,
    )
    .await?;
    let accepted = if events.is_empty() {
        0
    } else {
        let batch = IngestBatch {
            batch_id: Id::new(),
            org_id: connector.org_id.clone(),
            stream,
            stream_type: StreamType::Logs,
            events,
            received_at: TimestampMicros::now(),
        };
        state.ingestion.ingest(batch).await?.accepted
    };
    // last_run 仅观测用，失败不影响接入结果。
    let _ = state
        .storage
        .connectors
        .touch_last_run(&connector.id, TimestampMicros::now())
        .await;
    Ok(accepted)
}

/// AWS Kinesis Data Firehose HTTP endpoint：body 是 `{requestId, timestamp, records[]}`，
/// 每条 record 的 `data` 为 base64。Firehose 要求 200 + `{requestId, timestamp}` ACK。
async fn kinesis_firehose(
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    let token = token_from(&headers, raw_query.as_deref());
    let connector = resolve_connector(&state, ConnectorKind::AwsKinesisFirehose, token).await?;
    let stream = target_stream(&connector, "kinesis");
    let raw = decode_body(&headers, &body)?;
    let req: FirehoseRequest =
        serde_json::from_slice(&raw).map_err(|e| Error::invalid(format!("firehose body: {e}")))?;
    let request_id = req.request_id.clone();
    let events = parse_firehose_request(req)?;
    ingest_events(&state, &connector, stream, events, body.len()).await?;
    let resp = serde_json::json!({
        "requestId": request_id,
        "timestamp": TimestampMicros::now().0 / 1_000, // ms
    });
    Ok((StatusCode::OK, Json(resp)).into_response())
}

/// Cloudflare Logpush：NDJSON（每行一条 JSON），body 默认 gzip 压缩。
async fn cloudflare(
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode> {
    let token = token_from(&headers, raw_query.as_deref());
    let connector = resolve_connector(&state, ConnectorKind::CloudflareLogpush, token).await?;
    let stream = target_stream(&connector, "cloudflare");
    let raw = decode_body(&headers, &body)?;
    let text = String::from_utf8_lossy(&raw);
    let events = parse_ndjson(text.as_ref())?;
    ingest_events(&state, &connector, stream, events, body.len()).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Heroku log drain：纯文本，每行一条日志。Heroku 设不了自定义头 → token 走 `?token=`。
async fn heroku(
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode> {
    let token = token_from(&headers, raw_query.as_deref());
    let connector = resolve_connector(&state, ConnectorKind::HerokuDrain, token).await?;
    let stream = target_stream(&connector, "heroku");
    let raw = decode_body(&headers, &body)?;
    let text = String::from_utf8_lossy(&raw);
    let events = parse_text(text.as_ref())?;
    ingest_events(&state, &connector, stream, events, body.len()).await?;
    Ok(StatusCode::NO_CONTENT)
}
