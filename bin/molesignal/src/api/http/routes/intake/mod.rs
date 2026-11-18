// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::io::Read as _;

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{
        HeaderMap, HeaderValue,
        header::{CONTENT_ENCODING, RETRY_AFTER},
    },
    response::{IntoResponse, Response},
    routing::post,
};
use domain::{
    iam::{IamContext, permission},
    intake::{IntakeResult, usage::source::IntakeCredential},
    stream::StreamType,
};
use kernel::{Error, Result};

use crate::{api::AppState, app::intake::usage::IntakeReceipt};

mod compat;
mod connectors;
pub(crate) mod otlp;
mod prometheus;

/// 没有 `stream-name` 头时 OTLP 与 Prometheus remote_write 写入的流：与前端 explore /
/// send-test-event 相同的 `default` 流，不存在时 intake 自动建流。
const DEFAULT_STREAM: &str = "default";

/// 原生 intake 请求体的上限，gzip 解压前后都按它算：平均大小的日志一次能发两万多条。
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// intake 缓冲已满（`429`）时让客户端等待的秒数：缓冲在一次 flush 完成时释放。
const RETRY_AFTER_SECONDS: &str = "1";

/// 请求头 `stream-name` 指定的目标流，缺省为 `default`。
fn stream_name(headers: &HeaderMap, default: &str) -> String {
    headers
        .get("stream-name")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(default)
        .to_string()
}

pub fn routes() -> Router<AppState> {
    let limit = || DefaultBodyLimit::max(MAX_BODY_BYTES);

    Router::new()
        .route("/intake/logs/{stream}", post(intake_logs).layer(limit()))
        .route(
            "/intake/metrics/{stream}",
            post(intake_metrics).layer(limit()),
        )
        .route(
            "/intake/traces/{stream}",
            post(intake_traces).layer(limit()),
        )
        .merge(otlp::routes())
        .merge(prometheus::routes())
        .merge(connectors::routes())
        .merge(compat::routes())
}

#[permission("logs.write")]
async fn intake_logs(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(stream): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> std::result::Result<Json<IntakeResult>, Rejection> {
    intake(state, ctx, stream, StreamType::LOGS, &headers, body)
        .await
        .map_err(Rejection)
}

#[permission("metrics.write")]
async fn intake_metrics(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(stream): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> std::result::Result<Json<IntakeResult>, Rejection> {
    intake(state, ctx, stream, StreamType::METRICS, &headers, body)
        .await
        .map_err(Rejection)
}

#[permission("traces.write")]
async fn intake_traces(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(stream): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> std::result::Result<Json<IntakeResult>, Rejection> {
    intake(state, ctx, stream, StreamType::TRACES, &headers, body)
        .await
        .map_err(Rejection)
}

async fn intake(
    state: AppState,
    ctx: IamContext,
    stream: String,
    stream_type: StreamType,
    headers: &HeaderMap,
    body: Bytes,
) -> Result<Json<IntakeResult>> {
    // 解压一个大的请求体要几十毫秒：放在 blocking 线程上，不占住处理请求的 worker。
    let body = if gzipped(headers) {
        tokio::task::spawn_blocking(move || decoded(true, body))
            .await
            .map_err(|error| Error::internal(format!("intake body join: {error}")))??
    } else {
        body
    };

    let size = body.len() as u64;

    let written = state
        .intake
        .intake_json(
            ctx.org_id.clone(),
            stream.clone(),
            stream_type,
            Vec::from(body),
        )
        .await?;

    let receipt = IntakeReceipt::of_services(
        IntakeCredential::of(&ctx),
        stream_type,
        &stream,
        size,
        written.services,
    );

    state
        .platform
        .intake_meter
        .record(&ctx.org_id, receipt, written.result.accepted as u64);

    Ok(Json(written.result))
}

/// 请求带 `Content-Encoding: gzip`。
fn gzipped(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("gzip"))
}

/// 请求体：`gzipped` 时解压，解压后同样不超过 [`MAX_BODY_BYTES`]。
fn decoded(gzipped: bool, body: Bytes) -> Result<Bytes> {
    if !gzipped {
        return Ok(body);
    }

    let mut out = Vec::new();

    flate2::read::GzDecoder::new(body.as_ref())
        .take(MAX_BODY_BYTES as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|error| Error::invalid(format!("gzip decode: {error}")))?;

    if out.len() > MAX_BODY_BYTES {
        return Err(Error::payload_too_large(format!(
            "intake body exceeds {} MiB after gzip decoding",
            MAX_BODY_BYTES / (1024 * 1024)
        )));
    }

    Ok(Bytes::from(out))
}

/// intake 失败时的响应：缓冲已满的 `429` 带上 `Retry-After`，客户端不必自己猜等多久。
struct Rejection(Error);

impl From<Error> for Rejection {
    fn from(error: Error) -> Self {
        Self(error)
    }
}

impl IntoResponse for Rejection {
    fn into_response(self) -> Response {
        let retry = matches!(self.0, Error::ResourceExhausted(_));
        let mut response = self.0.into_response();

        if retry {
            response
                .headers_mut()
                .insert(RETRY_AFTER, HeaderValue::from_static(RETRY_AFTER_SECONDS));
        }

        response
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use axum::http::StatusCode;
    use flate2::{Compression, write::GzEncoder};

    use super::*;

    fn gzip(bytes: &[u8]) -> Bytes {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(bytes).unwrap();
        Bytes::from(encoder.finish().unwrap())
    }

    fn encoded(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_ENCODING, HeaderValue::from_str(value).unwrap());
        headers
    }

    #[test]
    fn a_gzip_body_is_decoded_and_a_plain_one_kept() {
        let body = br#"[{"message":"hi"}]"#;

        assert_eq!(
            decoded(gzipped(&encoded("gzip")), gzip(body)).unwrap(),
            Bytes::from_static(body)
        );
        assert_eq!(
            decoded(gzipped(&HeaderMap::new()), Bytes::from_static(body)).unwrap(),
            Bytes::from_static(body)
        );
        assert!(matches!(
            decoded(gzipped(&encoded("gzip")), Bytes::from_static(body)),
            Err(Error::InvalidArgument(_))
        ));
    }

    #[test]
    fn a_gzip_body_larger_than_the_limit_once_decoded_is_refused() {
        let body = gzip(&vec![b' '; MAX_BODY_BYTES + 1]);

        assert!(body.len() < MAX_BODY_BYTES);
        assert!(matches!(
            decoded(gzipped(&encoded("gzip")), body),
            Err(Error::PayloadTooLarge(_))
        ));
    }

    #[test]
    fn full_buffers_tell_the_client_when_to_retry() {
        let full = Rejection(Error::resource_exhausted(
            "intake buffer memory limit exceeded",
        ))
        .into_response();

        assert_eq!(full.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(full.headers()[RETRY_AFTER], RETRY_AFTER_SECONDS);

        let invalid = Rejection(Error::invalid("intake body")).into_response();

        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
        assert!(invalid.headers().get(RETRY_AFTER).is_none());
    }
}
