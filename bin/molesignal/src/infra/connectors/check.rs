// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A connection test: whether a connector's settings reach the other side and are accepted
//! there, tried before they are saved.
//!
//! Each kind does the least that proves what a delivery or a pull needs: S3 writes and removes a
//! small object, Kafka asks for the topic's metadata, an HTTP destination is sent one event
//! marked as a test, CloudWatch Logs is asked for one event of the last minute, an S3 source lists
//! one object. A push connector has nothing to connect to.
//!
//! The outcome names what went wrong by a [`CheckCode`] the interface words in its own language,
//! with the other side's own words as `detail`.

use std::time::{Duration, Instant};

use serde::Serialize;

use super::{
    ConnectorKind,
    aws::CredentialSource,
    dispatcher::{http, kafka, s3},
    repo::Connector,
    runner, sources,
};
use crate::shared::{Error, Result};

/// Longer than any one step takes before giving up on its own.
const TIMEOUT: Duration = Duration::from_secs(25);

/// The longest `detail` shown.
const MAX_DETAIL_CHARS: usize = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckCode {
    Ok,
    /// The settings cannot work as they are.
    Invalid,
    /// No credentials could be had: the node has no identity, or none of the kind asked for.
    NoCredentials,
    /// The other side refused the credentials, or what they are allowed to do.
    Denied,
    /// The bucket, topic, log group or address is not there.
    NotFound,
    /// The TLS handshake failed: a certificate not trusted, or not presented.
    Tls,
    /// The host could not be resolved or connected to.
    Unreachable,
    Timeout,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckOutcome {
    pub ok: bool,
    pub code: CheckCode,
    pub detail: Option<String>,
    pub elapsed_ms: u64,
}

pub async fn check(connector: &Connector) -> CheckOutcome {
    let started = Instant::now();

    let result = match tokio::time::timeout(TIMEOUT, run(connector)).await {
        Ok(result) => result,
        Err(_) => Err(Error::unavailable(format!(
            "no answer within {} seconds: timed out",
            TIMEOUT.as_secs()
        ))),
    };

    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

    match result {
        Ok(()) => CheckOutcome {
            ok: true,
            code: CheckCode::Ok,
            detail: None,
            elapsed_ms,
        },
        Err(error) => CheckOutcome {
            ok: false,
            code: code(&error),
            detail: Some(detail(&error)),
            elapsed_ms,
        },
    }
}

async fn run(connector: &Connector) -> Result<()> {
    match connector.kind.parse::<ConnectorKind>()? {
        ConnectorKind::S3 => s3::check(connector).await,
        ConnectorKind::Kafka | ConnectorKind::KafkaSource => kafka::check(connector).await,
        ConnectorKind::Http => http::check(connector).await,
        ConnectorKind::AwsCloudwatchLogs => {
            runner::cloudwatch(connector, CredentialSource::Fresh)
                .check()
                .await
        }
        ConnectorKind::S3Source => sources::s3::check(connector).await,
        ConnectorKind::AwsKinesisFirehose
        | ConnectorKind::CloudflareLogpush
        | ConnectorKind::HerokuDrain => Err(Error::invalid(
            "a push connector is sent to; there is nothing for it to connect to",
        )),
    }
}

const TLS: &[&str] = &[
    "SSL handshake failed",
    "certificate verify failed",
    "invalid peer certificate",
    "UnknownIssuer",
    "CertificateRequired",
    "handshake failure",
];

const DENIED: &[&str] = &[
    "AccessDenied",
    "InvalidAccessKeyId",
    "SignatureDoesNotMatch",
    "ExpiredToken",
    "InvalidToken",
    "InvalidClientTokenId",
    "UnrecognizedClient",
    "Authentication failure",
    "Authentication failed",
    "SASL authentication error",
    "authorization failed",
    "Authorization failed",
    "403 Forbidden",
    "401 Unauthorized",
];

const NOT_FOUND: &[&str] = &[
    "NoSuchBucket",
    "ResourceNotFoundException",
    "Unknown topic",
    "UNKNOWN_TOPIC",
];

const UNREACHABLE: &[&str] = &[
    "could not be resolved",
    "resolves to no address",
    "dns error",
    "failed to lookup address",
    "onnection refused",
    "connection failed",
    "error trying to connect",
    "tcp connect error",
    "Broker transport failure",
    "All broker connections are down",
    "No route to host",
    "Network is unreachable",
];

const TIMED_OUT: &[&str] = &["timed out", "Timed out", "TimedOut", "deadline has elapsed"];

/// What went wrong, read from the kind of error first and its words second: S3, librdkafka and
/// the HTTP client say it only in their messages.
fn code(error: &Error) -> CheckCode {
    let text = error.to_string();
    let says = |markers: &[&str]| markers.iter().any(|marker| text.contains(marker));

    if matches!(error, Error::InvalidArgument(_) | Error::Validation { .. }) {
        CheckCode::Invalid
    } else if says(TLS) {
        CheckCode::Tls
    } else if says(DENIED) || matches!(error, Error::Forbidden(_)) {
        CheckCode::Denied
    } else if text.contains("aws credentials:") || matches!(error, Error::Unauthorized(_)) {
        CheckCode::NoCredentials
    } else if says(NOT_FOUND) || matches!(error, Error::NotFound(_)) {
        CheckCode::NotFound
    } else if says(UNREACHABLE) {
        CheckCode::Unreachable
    } else if says(TIMED_OUT) {
        CheckCode::Timeout
    } else {
        CheckCode::Failed
    }
}

/// The error's own words, without the kind of error it is put in front of them.
fn detail(error: &Error) -> String {
    let words = match error {
        Error::NotFound(text)
        | Error::Conflict(text)
        | Error::InvalidArgument(text)
        | Error::Unauthorized(text)
        | Error::Forbidden(text)
        | Error::Internal(text)
        | Error::Unavailable(text) => text.clone(),
        other => other.to_string(),
    };

    let mut detail: String = words.chars().take(MAX_DETAIL_CHARS).collect();

    if words.chars().count() > MAX_DETAIL_CHARS {
        detail.push('…');
    }

    detail
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::shared::{ids::Id, time::TimestampMicros};

    fn connector(kind: &str, config: serde_json::Value) -> Connector {
        Connector {
            id: Id::new(),
            org_id: Id::new(),
            name: "test".into(),
            kind: kind.into(),
            config_json: config,
            enabled: true,
            last_run_at: None,
            read_state: serde_json::Value::Null,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn a_failure_is_named_by_what_it_means() {
        let cases = [
            (Error::invalid("`bucket` is required"), CheckCode::Invalid),
            (
                Error::unavailable("s3 put: Generic S3 error: ... <Code>AccessDenied</Code>"),
                CheckCode::Denied,
            ),
            (
                Error::unavailable("s3 put: Generic S3 error: <Code>NoSuchBucket</Code>"),
                CheckCode::NotFound,
            ),
            (
                Error::unauthorized("aws credentials: Generic S3 error: error sending request"),
                CheckCode::NoCredentials,
            ),
            (
                Error::unauthorized(
                    "aws credentials: forbidden: sts AssumeRole refused: AccessDenied: not authorized",
                ),
                CheckCode::Denied,
            ),
            (
                Error::unavailable(
                    "kafka metadata: Meta data fetch error: OperationTimedOut (Local: Timed out) \
                     (Local: Authentication failure: SASL authentication error: Authentication failed)",
                ),
                CheckCode::Denied,
            ),
            (
                Error::unavailable(
                    "kafka metadata: OperationTimedOut (SSL handshake failed: certificate verify failed)",
                ),
                CheckCode::Tls,
            ),
            (
                Error::unavailable(
                    "kafka metadata: OperationTimedOut (Local: Broker transport failure: Connection refused)",
                ),
                CheckCode::Unreachable,
            ),
            (
                Error::unavailable("`in.example.com` could not be resolved: dns error"),
                CheckCode::Unreachable,
            ),
            (
                Error::unavailable("http request timed out: operation timed out"),
                CheckCode::Timeout,
            ),
            (
                Error::forbidden("the receiver refused the credential: HTTP 401"),
                CheckCode::Denied,
            ),
            (Error::not_found("kafka topic `logs`"), CheckCode::NotFound),
            (
                Error::unavailable("the receiver did not take the events: HTTP 500"),
                CheckCode::Failed,
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(code(&error), expected, "{error}");
        }
    }

    #[test]
    fn the_detail_is_the_errors_own_words_and_not_too_many() {
        assert_eq!(
            detail(&Error::unavailable(
                "the receiver did not take the events: HTTP 500"
            )),
            "the receiver did not take the events: HTTP 500"
        );

        let long = detail(&Error::unavailable("x".repeat(2000)));
        assert_eq!(long.chars().count(), MAX_DETAIL_CHARS + 1);
        assert!(long.ends_with('…'));
    }

    #[tokio::test]
    async fn a_push_connector_has_nothing_to_test() {
        let outcome = check(&connector("heroku_drain", json!({}))).await;

        assert!(!outcome.ok);
        assert_eq!(outcome.code, CheckCode::Invalid);
    }

    #[tokio::test]
    async fn settings_that_cannot_work_say_so_without_connecting() {
        let outcome = check(&connector(
            "http",
            json!({ "url": "https://in.example.com", "auth": "bearer" }),
        ))
        .await;

        assert_eq!(outcome.code, CheckCode::Invalid);
        assert!(outcome.detail.unwrap().contains("bearerToken"));
    }
}
