// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 调模型服务的 HTTP 传输：连接超时、读空闲超时与有限重试。
//!
//! 超时按"多久没收到数据"算，而不是一次请求的总时长：流式回答可能持续几分钟，总超时会把
//! 正常的长回答截断。重试只发生在拿到可用响应之前（连接失败、限流、服务端暂时不可用），
//! 已经开始交给调用方的回答不会重来。

use std::{
    future::Future,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::{Response, StatusCode, header::HeaderMap};

/// 一次调用在第一次之外最多再试几次。
pub const DEFAULT_MAX_RETRIES: u32 = 2;

/// 模型服务没有配置超时时，多久没收到数据算失败。
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// 建立连接最多等这么久；配置的超时更短时取配置值。
const CONNECT_TIMEOUT_CAP: Duration = Duration::from_secs(10);

const BASE_BACKOFF: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(8);

/// 服务端要求的等待时间超过这个值时按这个值等，免得一轮回答卡在重试上。
const MAX_RETRY_AFTER: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportOptions {
    /// 多久没有收到任何数据就判定这次请求失败。
    pub idle_timeout: Duration,
    pub max_retries: u32,
}

impl Default for TransportOptions {
    fn default() -> Self {
        Self {
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }
}

impl TransportOptions {
    /// 按模型服务配置里的 `timeout_ms` 构造；不是正数时用默认值。
    pub fn from_timeout_ms(timeout_ms: i64) -> Self {
        let idle_timeout = u64::try_from(timeout_ms)
            .ok()
            .filter(|millis| *millis > 0)
            .map(Duration::from_millis)
            .unwrap_or(DEFAULT_IDLE_TIMEOUT);

        Self {
            idle_timeout,
            ..Self::default()
        }
    }

    pub fn connect_timeout(&self) -> Duration {
        self.idle_timeout.min(CONNECT_TIMEOUT_CAP)
    }

    /// 带连接超时与读空闲超时的 HTTP 客户端。
    pub fn client(&self) -> reqwest::Client {
        reqwest::Client::builder()
            .connect_timeout(self.connect_timeout())
            .read_timeout(self.idle_timeout)
            .build()
            .unwrap_or_else(|error| {
                tracing::warn!(%error, "model HTTP client fell back to defaults");
                reqwest::Client::new()
            })
    }
}

/// 这个状态码值得再试一次：请求超时、限流、服务端暂时不可用或过载。
pub fn retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 504 | 529)
}

/// 第 `attempt` 次重试（从 0 起）前等多久。
///
/// 服务端在 `retry-after-ms` 或 `retry-after`（秒）里给了时间就照办，但不超过上限；
/// 否则指数退避，再加最多 250 ms 的抖动，免得同时失败的请求一起重来。
pub fn retry_delay(attempt: u32, headers: Option<&HeaderMap>) -> Duration {
    if let Some(requested) = headers.and_then(requested_delay) {
        return requested.min(MAX_RETRY_AFTER);
    }

    let backoff = BASE_BACKOFF
        .saturating_mul(2u32.saturating_pow(attempt))
        .min(MAX_BACKOFF);

    backoff + jitter()
}

fn requested_delay(headers: &HeaderMap) -> Option<Duration> {
    let read = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value >= 0.0)
    };

    if let Some(millis) = read("retry-after-ms") {
        return Some(Duration::from_secs_f64(millis / 1000.0));
    }

    read("retry-after").map(Duration::from_secs_f64)
}

fn jitter() -> Duration {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos())
        .unwrap_or(0);

    Duration::from_millis(u64::from(nanos % 250))
}

/// 发出一次逻辑请求。
///
/// `send` 每次调用都重新构造并发出请求（请求体不能重放）。连接失败与可重试的状态码按
/// [`retry_delay`] 等待后重来；读超时不重试，同样的慢请求再来一遍多半还是慢。重试用完后
/// 原样返回最后一次的结果，由调用方决定怎么报错。
pub async fn send_with_retry<F, Fut>(
    options: TransportOptions,
    mut send: F,
) -> Result<Response, reqwest::Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Response, reqwest::Error>>,
{
    let mut attempt = 0u32;

    loop {
        let outcome = send().await;
        let may_retry = attempt < options.max_retries;

        let delay = match &outcome {
            Ok(response) if may_retry && retryable_status(response.status()) => {
                retry_delay(attempt, Some(response.headers()))
            }
            Err(error) if may_retry && error.is_connect() => retry_delay(attempt, None),
            _ => return outcome,
        };

        tracing::debug!(
            attempt = attempt + 1,
            delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
            status = outcome
                .as_ref()
                .ok()
                .map(|response| response.status().as_u16()),
            "retrying model request"
        );

        drop(outcome);
        tokio::time::sleep(delay).await;
        attempt += 1;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use reqwest::header::HeaderValue;
    use wiremock::{
        Mock, MockServer, Request, Respond, ResponseTemplate,
        matchers::{method, path},
    };

    use super::*;

    #[test]
    fn timeout_comes_from_the_provider_setting_or_the_default() {
        let configured = TransportOptions::from_timeout_ms(45_000);
        assert_eq!(configured.idle_timeout, Duration::from_secs(45));
        assert_eq!(configured.connect_timeout(), CONNECT_TIMEOUT_CAP);

        let short = TransportOptions::from_timeout_ms(3_000);
        assert_eq!(short.connect_timeout(), Duration::from_secs(3));

        assert_eq!(
            TransportOptions::from_timeout_ms(0).idle_timeout,
            DEFAULT_IDLE_TIMEOUT
        );
        assert_eq!(
            TransportOptions::from_timeout_ms(-5).idle_timeout,
            DEFAULT_IDLE_TIMEOUT
        );
    }

    #[test]
    fn only_transient_statuses_are_retried() {
        for code in [408, 429, 500, 502, 503, 504, 529] {
            assert!(
                retryable_status(StatusCode::from_u16(code).unwrap()),
                "{code}"
            );
        }

        for code in [200, 400, 401, 403, 404, 422] {
            assert!(
                !retryable_status(StatusCode::from_u16(code).unwrap()),
                "{code}"
            );
        }
    }

    #[test]
    fn retry_after_is_honoured_up_to_a_cap() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_static("3"));
        assert_eq!(retry_delay(0, Some(&headers)), Duration::from_secs(3));

        headers.insert("retry-after-ms", HeaderValue::from_static("1500"));
        assert_eq!(retry_delay(0, Some(&headers)), Duration::from_millis(1500));

        let mut long = HeaderMap::new();
        long.insert("retry-after", HeaderValue::from_static("600"));
        assert_eq!(retry_delay(0, Some(&long)), MAX_RETRY_AFTER);
    }

    #[test]
    fn backoff_grows_and_stays_bounded() {
        let first = retry_delay(0, None);
        assert!(first >= BASE_BACKOFF && first < BASE_BACKOFF + Duration::from_millis(250));

        let second = retry_delay(1, None);
        assert!(second >= BASE_BACKOFF * 2);

        let late = retry_delay(20, None);
        assert!(late <= MAX_BACKOFF + Duration::from_millis(250));
    }

    /// 前 `failures` 次回 `status`，之后回 200。
    struct FlakyResponder {
        status: u16,
        failures: usize,
        seen: Arc<AtomicUsize>,
    }

    impl Respond for FlakyResponder {
        fn respond(&self, _request: &Request) -> ResponseTemplate {
            let call = self.seen.fetch_add(1, Ordering::SeqCst);

            if call < self.failures {
                ResponseTemplate::new(self.status).insert_header("retry-after", "0")
            } else {
                ResponseTemplate::new(200).set_body_string("ok")
            }
        }
    }

    async fn run_against(status: u16, failures: usize, max_retries: u32) -> (u16, usize) {
        let server = MockServer::start().await;
        let seen = Arc::new(AtomicUsize::new(0));

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(FlakyResponder {
                status,
                failures,
                seen: seen.clone(),
            })
            .mount(&server)
            .await;

        let options = TransportOptions {
            idle_timeout: Duration::from_secs(5),
            max_retries,
        };

        let client = options.client();
        let url = format!("{}/v1/messages", server.uri());

        let response = send_with_retry(options, || client.post(&url).send())
            .await
            .unwrap();

        (response.status().as_u16(), seen.load(Ordering::SeqCst))
    }

    #[tokio::test]
    async fn rate_limited_requests_are_sent_again() {
        assert_eq!(run_against(429, 2, 2).await, (200, 3));
    }

    #[tokio::test]
    async fn retries_stop_at_the_limit_and_return_the_last_answer() {
        assert_eq!(run_against(503, 5, 2).await, (503, 3));
    }

    #[tokio::test]
    async fn client_errors_are_not_retried() {
        assert_eq!(run_against(400, 5, 2).await, (400, 1));
    }

    #[tokio::test]
    async fn connection_failures_are_retried() {
        let options = TransportOptions {
            idle_timeout: Duration::from_secs(1),
            max_retries: 1,
        };

        let client = options.client();
        let attempts = AtomicUsize::new(0);

        // 端口 9 (discard) 在本机通常没有监听，连接立刻被拒绝。
        let outcome = send_with_retry(options, || {
            attempts.fetch_add(1, Ordering::SeqCst);
            client.post("http://127.0.0.1:9/unreachable").send()
        })
        .await;

        assert!(outcome.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }
}
