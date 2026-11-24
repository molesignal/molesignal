// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Push 型 connector 接入端点：第三方平台把数据直接 POST 进来。
//!
//! - `POST /api/v1/_kinesis_firehose` — Amazon Data Firehose HTTP endpoint：日志与 CloudWatch
//!   Metric Streams 的指标（见 [`firehose`]）
//! - `POST /api/v1/_gcp_pubsub` — Google Cloud Pub/Sub push 订阅：Cloud Logging 的日志（见
//!   [`pubsub`]）
//! - `POST /api/v1/_cloud_events` — 云事件总线的 HTTP 目标：CloudEvents、Amazon EventBridge、
//!   Azure Event Grid 的事件（见 [`cloud_events`]）
//! - `POST /api/v1/_cloudflare` — Cloudflare Logpush（NDJSON）
//! - `POST /api/v1/_heroku` — Heroku log drain（文本，每行一条）
//!
//! 这些端点**不走 JWT/API token**（外部平台带不了 `Authorization: Bearer`），而是用
//! 一个 connector token 自鉴权：按 `kind` + 令牌的 SHA-256 反查命中的 enabled connector
//! （令牌本身与其它密钥一起封存，见 `infra::connectors::secrets`），取其 `org_id` 与目标流。
//! 故路由在鉴权中间件白名单内。反查结果缓存 [`CONNECTOR_CACHE_SECS`] 秒：Pub/Sub 一条消息
//! 一个请求，不能每次都查库；停用或换 Token 最多这么久后生效。
//!
//! token 的来源因平台而异，都接受：
//! - `X-Connector-Token` 头（Cloudflare 等可配自定义头的平台）
//! - `X-Amz-Firehose-Access-Key` 头（Firehose 的固定头）
//! - `X-EventBridge-Signature-Token` 头（阿里云 EventBridge 的 HTTP 目标）
//! - `?token=` 查询参数（Heroku、Pub/Sub 等无法设自定义头的平台）
//! - `Authorization: Basic` 的密码（只能把凭据写进地址 `user:token@host` 的平台）
//!
//! 目标流取 `config_json.targetStream`（日志）与 `metricsStream`（指标），缺省按平台各有一个名字。

use std::{io::Read as _, sync::OnceLock, time::Duration};

use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, RawQuery, State},
    http::{HeaderMap, StatusCode, header::CONTENT_ENCODING},
    routing::post,
};
use domain::{
    intake::{IntakeBatch, RawEvent, usage::source::IntakeCredential},
    stream::StreamType,
};
use flate2::read::GzDecoder;
use kernel::{Error, Result, ids::Id, time::TimestampMicros};

use crate::{
    api::AppState,
    app::intake::usage::IntakeReceipt,
    infra::connectors::{
        Connector, ConnectorKind,
        cloudflare::parse_ndjson,
        heroku::parse_text,
        runner::{Refusals, note_warning},
        secrets::push_token_digest,
    },
};

mod cloud_events;
mod firehose;
mod pubsub;

/// 除 Firehose 外推送请求体的上限，gzip 解压前后都按它算。
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// 一次写入最多多少条：一个大请求读出来的事件分几批写。
const MAX_EVENTS_PER_WRITE: usize = 10_000;

/// 反查到的 connector 缓存多久。
const CONNECTOR_CACHE_SECS: u64 = 30;

pub fn routes() -> Router<AppState> {
    let limit = || DefaultBodyLimit::max(MAX_BODY_BYTES);

    Router::new()
        .route(
            "/_kinesis_firehose",
            post(firehose::receive).layer(DefaultBodyLimit::max(firehose::MAX_BODY_BYTES)),
        )
        .route("/_gcp_pubsub", post(pubsub::receive).layer(limit()))
        .route(
            "/_cloud_events",
            post(cloud_events::receive)
                .options(cloud_events::allow_origin)
                .layer(limit()),
        )
        .route("/_cloudflare", post(cloudflare).layer(limit()))
        .route("/_heroku", post(heroku).layer(limit()))
}

/// 从请求里取 connector token：自定义头 / Firehose 头 / 阿里云 EventBridge 头 / `?token=` 查询
/// 参数 / Basic 认证的密码（按此优先级）。
fn token_from(headers: &HeaderMap, raw_query: Option<&str>) -> Option<String> {
    let header_token = headers
        .get("x-connector-token")
        .or_else(|| headers.get("x-amz-firehose-access-key"))
        .or_else(|| headers.get("x-eventbridge-signature-token"))
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let query_token = raw_query.and_then(|q| {
        q.split('&')
            .find_map(|kv| kv.strip_prefix("token="))
            .map(str::trim)
            .filter(|s| !s.is_empty())
    });

    header_token
        .or(query_token)
        .map(str::to_string)
        .or_else(|| basic_password(headers))
}

/// The password of `Authorization: Basic` credentials.
fn basic_password(headers: &HeaderMap) -> Option<String> {
    use base64::Engine as _;

    let encoded = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Basic ")?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?;
    let credentials = String::from_utf8(decoded).ok()?;
    let (_, password) = credentials.split_once(':')?;

    Some(password.trim().to_string()).filter(|password| !password.is_empty())
}

fn connector_cache() -> &'static moka::future::Cache<(&'static str, String), Connector> {
    static CACHE: OnceLock<moka::future::Cache<(&'static str, String), Connector>> =
        OnceLock::new();

    CACHE.get_or_init(|| {
        moka::future::Cache::builder()
            .max_capacity(10_000)
            .time_to_live(Duration::from_secs(CONNECTOR_CACHE_SECS))
            .build()
    })
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

    let key = (kind.as_str(), push_token_digest(&token));

    if let Some(connector) = connector_cache().get(&key).await {
        return Ok(connector);
    }

    let connector = state
        .storage
        .connectors
        .find_for_push(kind.as_str(), &token)
        .await?
        .ok_or_else(|| Error::unauthorized("connector token not recognized"))?;

    connector_cache().insert(key, connector.clone()).await;
    Ok(connector)
}

/// 配置里 `key` 指定的目标流；缺省为 `default`。`targetStream` 也接受早先的 `target_stream`。
fn stream_of(connector: &Connector, key: &str, default: &str) -> String {
    let named = |key: &str| {
        connector
            .config_json
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };

    named(key)
        .or_else(|| {
            (key == "targetStream")
                .then(|| named("target_stream"))
                .flatten()
        })
        .unwrap_or_else(|| default.to_string())
}

/// body 解码：`Content-Encoding: gzip` 时解压，解压后不超过 `limit`；否则原样返回。
fn decode_body(headers: &HeaderMap, body: &Bytes, limit: usize) -> Result<Vec<u8>> {
    let gzipped = headers
        .get(CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().contains("gzip"));

    if !gzipped {
        return Ok(body.to_vec());
    }

    let mut out = Vec::new();

    GzDecoder::new(body.as_ref())
        .take(limit as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|e| Error::invalid(format!("gzip decode: {e}")))?;

    if out.len() > limit {
        return Err(Error::payload_too_large(format!(
            "the body exceeds {} MiB after gzip decoding",
            limit / (1024 * 1024)
        )));
    }

    Ok(out)
}

/// 把一种信号的事件分批写进 `stream`，记用量；返回流拒收的事件。
async fn intake_events(
    state: &AppState,
    connector: &Connector,
    stream_type: StreamType,
    stream: &str,
    events: Vec<RawEvent>,
    bytes: usize,
) -> Result<Refusals> {
    let mut refusals = Refusals::default();

    if events.is_empty() {
        return Ok(refusals);
    }

    state
        .iam
        .service
        .ensure_organization_access(&connector.org_id)
        .await?;

    let receipt = IntakeReceipt::of_events(
        IntakeCredential::connector(&connector.id),
        stream_type,
        stream,
        bytes as u64,
        &events,
    );

    let received_at = TimestampMicros::now();
    let mut accepted = 0;
    let mut events = events;

    while !events.is_empty() {
        let rest = events.split_off(events.len().min(MAX_EVENTS_PER_WRITE));

        let batch = IntakeBatch {
            batch_id: Id::new(),
            org_id: connector.org_id.clone(),
            stream: stream.to_string(),
            stream_type,
            events,
            received_at,
        };

        let result = state.intake.intake(batch).await?;

        accepted += result.accepted;
        refusals.add(&result);
        events = rest;
    }

    state
        .platform
        .intake_meter
        .record(&connector.org_id, receipt, accepted as u64);

    Ok(refusals)
}

/// 记一次推送：last_run 仅观测用，失败不影响接入结果。
///
/// 流拒收的事件（值与字段类型不符）记成 connector 最近跳过的内容，在连接器页面可见，不让推送方
/// 重试：平台会整包重发，已写入的事件因此重复，拒收的也仍然写不进去。
async fn touch(state: &AppState, connector: &Connector, refusals: Refusals) {
    let now = TimestampMicros::now();
    let connectors = &state.storage.connectors;

    let Some(warning) = refusals.warning() else {
        let _ = connectors.touch_last_run(&connector.id, now).await;
        return;
    };

    tracing::warn!(
        connector = %connector.id.0,
        rejected = refusals.count,
        reason = refusals.first.as_deref().unwrap_or_default(),
        "pushed events refused by their stream"
    );

    let read_state = note_warning(&connector.read_state, &warning, now);

    if let Err(error) = connectors
        .save_read_state(&connector.id, Some(now), &read_state)
        .await
    {
        tracing::warn!(connector = %connector.id.0, %error, "connector read state not saved");
    }
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
    let stream = stream_of(&connector, "targetStream", "cloudflare");
    let raw = decode_body(&headers, &body, MAX_BODY_BYTES)?;
    let text = String::from_utf8_lossy(&raw);
    let events = parse_ndjson(text.as_ref())?;
    let refusals = intake_events(
        &state,
        &connector,
        StreamType::LOGS,
        &stream,
        events,
        raw.len(),
    )
    .await?;

    touch(&state, &connector, refusals).await;
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
    let stream = stream_of(&connector, "targetStream", "heroku");
    let raw = decode_body(&headers, &body, MAX_BODY_BYTES)?;
    let text = String::from_utf8_lossy(&raw);
    let events = parse_text(text.as_ref())?;
    let refusals = intake_events(
        &state,
        &connector,
        StreamType::LOGS,
        &stream,
        events,
        raw.len(),
    )
    .await?;

    touch(&state, &connector, refusals).await;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;
    use serde_json::json;

    use super::*;

    fn connector(config: serde_json::Value) -> Connector {
        Connector {
            id: Id::new(),
            org_id: Id::new(),
            name: "c".into(),
            kind: "aws_kinesis_firehose".into(),
            config_json: config,
            enabled: true,
            last_run_at: None,
            read_state: serde_json::Value::Null,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn the_stream_is_the_one_the_form_saves_then_the_older_key_then_the_default() {
        assert_eq!(
            stream_of(
                &connector(json!({"targetStream": "aws"})),
                "targetStream",
                "kinesis"
            ),
            "aws"
        );
        assert_eq!(
            stream_of(
                &connector(json!({"target_stream": "old"})),
                "targetStream",
                "kinesis"
            ),
            "old"
        );
        assert_eq!(
            stream_of(&connector(json!({})), "targetStream", "kinesis"),
            "kinesis"
        );
        assert_eq!(
            stream_of(
                &connector(json!({"metricsStream": " "})),
                "metricsStream",
                "cloudwatch"
            ),
            "cloudwatch"
        );
    }

    #[test]
    fn the_token_comes_from_a_header_or_the_query() {
        let mut headers = HeaderMap::new();

        assert_eq!(
            token_from(&headers, Some("a=1&token=abc")),
            Some("abc".into())
        );

        headers.insert(
            "x-amz-firehose-access-key",
            HeaderValue::from_static("firehose"),
        );

        assert_eq!(
            token_from(&headers, Some("token=abc")),
            Some("firehose".into())
        );
        assert_eq!(token_from(&HeaderMap::new(), None), None);

        let mut aliyun = HeaderMap::new();
        aliyun.insert(
            "x-eventbridge-signature-token",
            HeaderValue::from_static("eb"),
        );

        assert_eq!(token_from(&aliyun, None), Some("eb".into()));

        let mut basic = HeaderMap::new();
        basic.insert(
            "authorization",
            HeaderValue::from_static("Basic dXNlcjpzZWNyZXQtdG9rZW4="),
        );

        assert_eq!(token_from(&basic, None), Some("secret-token".into()));
    }

    #[test]
    fn a_gzip_body_larger_than_the_limit_once_decoded_is_refused() {
        use std::io::Write as _;

        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&[b' '; 2048]).unwrap();
        let body = Bytes::from(encoder.finish().unwrap());

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static("gzip"));

        assert!(matches!(
            decode_body(&headers, &body, 1024),
            Err(Error::PayloadTooLarge(_))
        ));
        assert_eq!(decode_body(&headers, &body, 2048).unwrap().len(), 2048);
    }
}
