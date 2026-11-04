// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! HTTP egress: a batch goes to the destination's address in one request, or in a few when it
//! is large, with the credential its `auth` says.
//!
//! With `auth` `hmac` each request carries `X-MoleSignal-Timestamp` (Unix seconds) and
//! `X-MoleSignal-Sign: sha256=<hex>`, the HMAC-SHA256 of `<timestamp>.<body>` under the
//! shared secret. A receiver recomputes it over the body it got and refuses an old timestamp, so a
//! captured request cannot be sent again later.
//!
//! The host is resolved before the request and the connection is pinned to what it resolved to,
//! none of which may be a cloud metadata address; redirects are not followed, since one could
//! lead where that check did not look.
//!
//! An alert pipeline's HTTP enrich step goes the same way ([`enrich`]), posts one event and reads
//! what the receiver answers.

use std::{net::SocketAddr, time::Duration};

use aws_lc_rs::hmac;
use reqwest::{
    Method,
    header::{CONTENT_TYPE, USER_AGENT},
    redirect::Policy,
};
use serde_json::{Map, Value, json};

use super::to_json_lines;
use crate::{
    infra::connectors::{
        config::http::{BodyFormat, HttpAuth, HttpMethod, HttpSettings, is_metadata_address},
        repo::Connector,
    },
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
        time::TimestampMicros,
    },
};

/// A larger batch is sent in several requests, each at most this large (unless one event alone
/// is larger), which most receivers accept.
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

const DELIVERY_TIMEOUT: Duration = Duration::from_secs(30);
const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a receiver has to answer an enrich call.
const ENRICH_TIMEOUT: Duration = Duration::from_secs(5);

/// The largest answer of an enrich call that is read.
const MAX_ENRICH_ANSWER_BYTES: usize = 1024 * 1024;

pub async fn dispatch(connector: &Connector, events: &[Value]) -> Result<()> {
    let settings = HttpSettings::parse(&connector.config_json)?;
    let client = client(&settings, DELIVERY_TIMEOUT).await?;

    for chunk in chunks(events, MAX_BODY_BYTES) {
        send(&client, &settings, body(chunk, settings.format)).await?;
    }

    Ok(())
}

/// Sends one event marked as a test: a receiver that takes it, with this address and credential,
/// takes the deliveries.
pub async fn check(connector: &Connector) -> Result<()> {
    let settings = HttpSettings::parse(&connector.config_json)?;
    let client = client(&settings, CHECK_TIMEOUT).await?;

    let event = json!({
        "message": "MoleSignal connection test",
        "molesignal_connection_test": true,
        "_timestamp": TimestampMicros::now().0,
    });

    send(&client, &settings, body(&[event], settings.format)).await
}

/// Posts one alert event as JSON and reads the answer: the JSON object it holds, `None` when it holds
/// nothing. Anything else is an error, and so is a refusal.
pub async fn enrich(connector: &Connector, event: &Value) -> Result<Option<Map<String, Value>>> {
    let settings = HttpSettings::parse(&connector.config_json)?;
    let client = pinned_client(&settings.url, ENRICH_TIMEOUT).await?;
    let body = serde_json::to_vec(event).map_err(|e| Error::internal(format!("event: {e}")))?;

    let request = client
        .post(settings.url.clone())
        .header(CONTENT_TYPE, "application/json")
        .header(USER_AGENT, "MoleSignal");

    let request = authorized(request, &settings.auth, &body);

    // The address may hold a credential, so no error names it.
    let mut response = http_trace::send(&client, request.body(body), HttpTarget::ThirdParty)
        .await
        .map_err(|e| request_error(e.without_url()))?;

    let status = response.status();

    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(refusal(status.as_u16(), &text));
    }

    let mut answer = Vec::new();

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| request_error(e.without_url()))?
    {
        answer.extend_from_slice(&chunk);

        if answer.len() > MAX_ENRICH_ANSWER_BYTES {
            return Err(Error::unavailable(format!(
                "the receiver answered with more than {MAX_ENRICH_ANSWER_BYTES} bytes"
            )));
        }
    }

    enrich_answer(&answer)
}

/// What an enrich call's answer holds.
fn enrich_answer(answer: &[u8]) -> Result<Option<Map<String, Value>>> {
    if answer.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }

    match serde_json::from_slice(answer) {
        Ok(Value::Object(fields)) => Ok(Some(fields)),
        Ok(_) => Err(Error::unavailable("the receiver's answer is not an object")),
        Err(e) => Err(Error::unavailable(format!(
            "the receiver's answer is not JSON: {e}"
        ))),
    }
}

/// A client that connects only where the host resolves to now, and follows no redirect.
async fn client(settings: &HttpSettings, timeout: Duration) -> Result<reqwest::Client> {
    pinned_client(&settings.url, timeout).await
}

/// A client for `url` that connects only where its host resolves to now, none of which may be a
/// cloud metadata address, and follows no redirect. Notification webhooks go out through it too.
pub(crate) async fn pinned_client(url: &url::Url, timeout: Duration) -> Result<reqwest::Client> {
    if let Some(url::Host::Ipv4(ip)) = url.host()
        && is_metadata_address(ip.into())
    {
        return Err(Error::forbidden(format!(
            "`{ip}` is where cloud metadata services answer"
        )));
    }

    if let Some(url::Host::Ipv6(ip)) = url.host()
        && is_metadata_address(ip.into())
    {
        return Err(Error::forbidden(format!(
            "`{ip}` is where cloud metadata services answer"
        )));
    }

    let mut builder = reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(Policy::none());

    if let Some(url::Host::Domain(domain)) = url.host() {
        let port = url.port_or_known_default().unwrap_or(443);

        let addresses: Vec<SocketAddr> = tokio::net::lookup_host((domain, port))
            .await
            .map_err(|e| Error::unavailable(format!("`{domain}` could not be resolved: {e}")))?
            .collect();

        if addresses.is_empty() {
            return Err(Error::unavailable(format!(
                "`{domain}` resolves to no address"
            )));
        }

        if let Some(address) = addresses.iter().find(|a| is_metadata_address(a.ip())) {
            return Err(Error::forbidden(format!(
                "`{domain}` resolves to `{}`, where cloud metadata services answer",
                address.ip()
            )));
        }

        builder = builder.resolve_to_addrs(domain, &addresses);
    }

    builder
        .build()
        .map_err(|e| Error::internal(format!("http client: {e}")))
}

async fn send(client: &reqwest::Client, settings: &HttpSettings, body: Vec<u8>) -> Result<()> {
    let method = match settings.method {
        HttpMethod::Post => Method::POST,
        HttpMethod::Put => Method::PUT,
    };

    let content_type = match settings.format {
        BodyFormat::Ndjson => "application/x-ndjson",
        BodyFormat::Json => "application/json",
    };

    let request = client
        .request(method, settings.url.clone())
        .header(CONTENT_TYPE, content_type)
        .header(USER_AGENT, "MoleSignal");

    let request = authorized(request, &settings.auth, &body);

    // The address may hold a credential, so no error names it.
    let response = http_trace::send(client, request.body(body), HttpTarget::ThirdParty)
        .await
        .map_err(|e| request_error(e.without_url()))?;

    let status = response.status();

    if status.is_success() {
        return Ok(());
    }

    let text = response.text().await.unwrap_or_default();
    Err(refusal(status.as_u16(), &text))
}

/// The request with the credential its destination's `auth` says.
fn authorized(
    request: reqwest::RequestBuilder,
    auth: &HttpAuth,
    body: &[u8],
) -> reqwest::RequestBuilder {
    match auth {
        HttpAuth::None => request,
        HttpAuth::Bearer(token) => request.bearer_auth(token),
        HttpAuth::Basic { username, password } => request.basic_auth(username, Some(password)),
        HttpAuth::Header { name, value } => request.header(name.as_str(), value.as_str()),
        HttpAuth::Hmac(secret) => {
            let timestamp = (TimestampMicros::now().0 / 1_000_000).to_string();

            request
                .header("x-molesignal-timestamp", &timestamp)
                .header("x-molesignal-sign", signature(secret, &timestamp, body))
        }
    }
}

/// `sha256=` and the hex HMAC-SHA256 of `<timestamp>.<body>` under `secret`. Notification webhooks
/// sign the same way.
pub(crate) fn signature(secret: &str, timestamp: &str, body: &[u8]) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let mut context = hmac::Context::with_key(&key);
    context.update(timestamp.as_bytes());
    context.update(b".");
    context.update(body);

    format!("sha256={}", hex::encode(context.sign().as_ref()))
}

/// The events split so that each request's body stays under `limit` bytes.
fn chunks(events: &[Value], limit: usize) -> Vec<&[Value]> {
    let mut chunks = Vec::new();
    let (mut start, mut size) = (0, 0);

    for (index, event) in events.iter().enumerate() {
        let event_size = serde_json::to_vec(event)
            .map(|bytes| bytes.len() + 1)
            .unwrap_or(0);

        if index > start && size + event_size > limit {
            chunks.push(&events[start..index]);
            (start, size) = (index, 0);
        }

        size += event_size;
    }

    if start < events.len() {
        chunks.push(&events[start..]);
    }

    chunks
}

fn body(events: &[Value], format: BodyFormat) -> Vec<u8> {
    match format {
        BodyFormat::Ndjson => to_json_lines(events).into_bytes(),
        BodyFormat::Json => serde_json::to_vec(events).unwrap_or_else(|_| b"[]".to_vec()),
    }
}

fn request_error(error: reqwest::Error) -> Error {
    if error.is_timeout() {
        Error::unavailable(format!("http request timed out: {error}"))
    } else if error.is_connect() {
        Error::unavailable(format!("http connection failed: {error}"))
    } else {
        Error::unavailable(format!("http request failed: {error}"))
    }
}

/// A non-2xx answer as an error, with the start of what the receiver said about it.
fn refusal(status: u16, text: &str) -> Error {
    let said: String = text
        .chars()
        .filter(|c| !c.is_control() || *c == ' ')
        .take(200)
        .collect();

    let reason = match said.trim() {
        "" => format!("HTTP {status}"),
        said => format!("HTTP {status}: {said}"),
    };

    match status {
        401 | 403 => Error::forbidden(format!("the receiver refused the credential: {reason}")),
        404 => Error::not_found(format!(
            "the receiver has nothing at this address: {reason}"
        )),
        300..=399 => Error::unavailable(format!(
            "the receiver redirects elsewhere, which deliveries do not follow: {reason}"
        )),
        _ => Error::unavailable(format!("the receiver did not take the events: {reason}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_enrich_answer_is_an_object_or_nothing() {
        assert_eq!(enrich_answer(b"").unwrap(), None);
        assert_eq!(enrich_answer(b" \n").unwrap(), None);
        assert_eq!(
            enrich_answer(b"{\"severity\":\"critical\"}")
                .unwrap()
                .unwrap()["severity"],
            "critical"
        );
        assert!(enrich_answer(b"[1]").is_err());
        assert!(enrich_answer(b"OK").is_err());
    }

    #[tokio::test]
    async fn an_enrich_call_never_reaches_a_metadata_address() {
        let connector = Connector {
            id: crate::shared::ids::Id::new(),
            org_id: crate::shared::ids::Id::new(),
            name: "cmdb".into(),
            kind: "http".into(),
            config_json: json!({ "url": "http://169.254.169.254/latest/meta-data/" }),
            enabled: true,
            last_run_at: None,
            read_state: Value::Null,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        };

        let error = enrich(&connector, &json!({ "summary": "x" }))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("cloud metadata"), "{error}");
    }

    #[test]
    fn a_body_is_ndjson_or_one_array() {
        let events = [json!({ "a": 1 }), json!({ "b": 2 })];

        assert_eq!(
            body(&events, BodyFormat::Ndjson),
            b"{\"a\":1}\n{\"b\":2}\n".to_vec()
        );
        assert_eq!(
            body(&events, BodyFormat::Json),
            b"[{\"a\":1},{\"b\":2}]".to_vec()
        );
    }

    #[test]
    fn a_large_batch_goes_in_requests_under_the_limit() {
        let events: Vec<Value> = (0..10).map(|n| json!({ "n": n })).collect();
        // Each event is 7 bytes and its line break.
        let chunks = chunks(&events, 20);

        assert_eq!(chunks.len(), 5);
        assert!(chunks.iter().all(|chunk| chunk.len() == 2));
        assert_eq!(chunks.concat(), events);

        // An event larger than the limit still goes, alone.
        let big = [json!({ "message": "x".repeat(64) }), json!({ "n": 1 })];
        assert_eq!(super::chunks(&big, 20).len(), 2);
        assert!(super::chunks(&[], 20).is_empty());
    }

    #[test]
    fn the_signature_covers_the_timestamp_and_the_body() {
        let signed = signature("secret-of-16-chars", "1700000000", b"{}");

        assert!(signed.starts_with("sha256="));
        assert_eq!(signed.len(), "sha256=".len() + 64);
        assert_eq!(signed, signature("secret-of-16-chars", "1700000000", b"{}"));
        assert_ne!(signed, signature("secret-of-16-chars", "1700000001", b"{}"));
        assert_ne!(signed, signature("secret-of-16-chars", "1700000000", b"[]"));
        assert_ne!(signed, signature("another-secret-16", "1700000000", b"{}"));
    }

    #[test]
    fn a_receiver_checks_it_with_any_hmac_library() {
        // Python: hmac.new(b"secret-of-16-chars", b'1700000000.{"a":1}\n', hashlib.sha256)
        let signed = signature("secret-of-16-chars", "1700000000", b"{\"a\":1}\n");

        assert_eq!(
            signed,
            "sha256=031b5d0bdacf18d51992ed27d4202b80a89441ccbf16bdab8561f67282db9e07"
        );
    }

    #[test]
    fn a_refusal_says_whether_it_was_the_credential() {
        assert!(matches!(refusal(401, ""), Error::Forbidden(_)));
        assert!(matches!(
            refusal(403, "invalid api key"),
            Error::Forbidden(_)
        ));
        assert!(matches!(refusal(404, ""), Error::NotFound(_)));
        assert!(matches!(refusal(302, ""), Error::Unavailable(_)));
        assert!(
            refusal(400, "bad\r\nrequest")
                .to_string()
                .contains("HTTP 400: badrequest")
        );
        assert_eq!(
            refusal(500, &"x".repeat(1000))
                .to_string()
                .matches('x')
                .count(),
            200
        );
    }
}
