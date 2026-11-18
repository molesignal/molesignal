// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! HTTP transport for model service calls: connection timeout, read idle timeout, and limited
//! retries.
//!
//! The timeout measures "how long no data has arrived", rather than the total request duration:
//! streaming responses may last several minutes, and a total timeout would cut off normal long
//! responses. Retries only happen before a usable response arrives (connection failure, rate
//! limiting, or temporary server unavailability). Responses already being delivered to the caller
//! are not restarted.

use std::{
    future::Future,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::{Response, StatusCode, header::HeaderMap};

/// The maximum number of retries after the initial attempt for one call.
pub const DEFAULT_MAX_RETRIES: u32 = 2;

/// How long without receiving data counts as failure when the model service has no timeout setting.
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// The maximum wait for a connection; uses the configured timeout if shorter.
const CONNECT_TIMEOUT_CAP: Duration = Duration::from_secs(10);

const BASE_BACKOFF: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(8);

/// Caps the server-requested wait at this value so that a response turn does not get stuck
/// retrying.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportOptions {
    /// How long without receiving any data causes the request to be considered failed.
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
    /// Constructs from `timeout_ms` in the model service configuration; uses the default if not
    /// positive.
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

    /// An HTTP client with connection and read idle timeouts.
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

/// This status code warrants a retry: request timeout, rate limiting, temporary server
/// unavailability, or overload.
pub fn retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 504 | 529)
}

/// How long to wait before retry `attempt` (starting at 0).
///
/// Uses the server-provided time in `retry-after-ms` or `retry-after` (seconds), capped at the
/// limit; otherwise uses exponential backoff plus up to 250 ms of jitter so that requests failing
/// simultaneously do not all retry together.
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

/// Sends one logical request.
///
/// Each `send` call reconstructs and sends the request (the request body cannot be replayed).
/// Connection failures and retryable status codes trigger retries after waiting for
/// [`retry_delay`]; read timeouts are not retried, since the same slow request would likely be
/// slow again. After retries are exhausted, returns the last result unchanged, leaving error
/// reporting to the caller.
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

    /// Returns `status` for the first `failures` attempts, then returns 200.
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

        // Port 9 (discard) usually has no local listener, so the connection is refused immediately.
        let outcome = send_with_retry(options, || {
            attempts.fetch_add(1, Ordering::SeqCst);
            client.post("http://127.0.0.1:9/unreachable").send()
        })
        .await;

        assert!(outcome.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }
}
