// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! One `DescribeMetricList` call: an RPC-style OpenAPI request with its parameters in the query
//! string, signed with the V3 signature (ACS3-HMAC-SHA256) the notification adapters use.

use aws_lc_rs::rand::{SecureRandom, SystemRandom};
use kernel::{Error, Result};
use serde_json::Value;
use signals::http_trace::{self, HttpTarget};

use super::settings::Settings;
use crate::infra::{connectors::cloud_metrics::refusal, notify::adapters::aliyun_sign as sign};

const API_VERSION: &str = "2019-01-01";
const ACTION: &str = "DescribeMetricList";
const API: &str = "cloudmonitor DescribeMetricList";

/// The most records CloudMonitor returns in one page.
pub(super) const MAX_PAGE_LENGTH: usize = 1440;

/// What one query asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Query<'a> {
    pub metric: &'a str,
    /// Unix milliseconds, exclusive: CloudMonitor reads `(start, end]`.
    pub start_ms: i64,
    /// Unix milliseconds, inclusive.
    pub end_ms: i64,
    pub length: usize,
    pub next_token: Option<&'a str>,
}

/// One page of the answer.
#[derive(Debug, Default)]
pub(super) struct Page {
    /// `Datapoints`: a JSON text holding an array of records.
    pub datapoints: Value,
    pub next_token: Option<String>,
}

/// The query parameters of `query`.
pub(super) fn params(settings: &Settings, query: &Query<'_>) -> Vec<(&'static str, String)> {
    let mut params = vec![
        ("Namespace", settings.namespace.clone()),
        ("MetricName", query.metric.to_string()),
        ("Period", settings.schedule.period_secs.to_string()),
        ("StartTime", query.start_ms.to_string()),
        ("EndTime", query.end_ms.to_string()),
        ("Length", query.length.to_string()),
    ];

    if let Some(dimensions) = &settings.dimensions {
        params.push(("Dimensions", dimensions.clone()));
    }

    if let Some(token) = query.next_token.filter(|token| !token.is_empty()) {
        params.push(("NextToken", token.to_string()));
    }

    params
}

/// Asks CloudMonitor for one page.
pub(super) async fn describe_metric_list(
    client: &reqwest::Client,
    settings: &Settings,
    query: &Query<'_>,
) -> Result<Page> {
    let host = settings.host();
    let query_string = sign::canonical_query(&params(settings, query));
    let payload_hash = sign::sha256_hex(b"");
    let date = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let nonce = nonce()?;

    let headers = [
        ("host", host.as_str()),
        ("x-acs-action", ACTION),
        ("x-acs-version", API_VERSION),
        ("x-acs-date", date.as_str()),
        ("x-acs-signature-nonce", nonce.as_str()),
        ("x-acs-content-sha256", payload_hash.as_str()),
    ];

    let authorization = sign::authorization(
        &settings.access_key,
        &settings.secret_key,
        &sign::Request {
            method: "POST",
            query: &query_string,
            headers: &headers,
            payload_hash: &payload_hash,
        },
    );

    let mut request = client
        .post(format!("https://{host}/?{query_string}"))
        .header("accept", "application/json")
        .header("authorization", authorization);

    // The client sends `host` from the address, as signed.
    for (name, value) in &headers[1..] {
        request = request.header(*name, *value);
    }

    let response = http_trace::send(client, request, HttpTarget::ThirdParty)
        .await
        .map_err(|e| Error::unavailable(format!("{API} request: {e}")))?;

    let status = response.status().as_u16();

    let body = response
        .text()
        .await
        .map_err(|e| Error::unavailable(format!("{API} body: {e}")))?;

    page(status, &body)
}

/// The page in an answer, or why the answer is a refusal.
pub(super) fn page(status: u16, body: &str) -> Result<Page> {
    let answer: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let code = answer.get("Code").map(code_text).unwrap_or_default();
    let refused = answer.get("Success").and_then(Value::as_bool) == Some(false);

    if !(200..300).contains(&status) || refused || !matches!(code.as_str(), "" | "200") {
        return Err(refused_with(status, &code, body));
    }

    if answer.is_null() {
        return Err(Error::unavailable(format!("{API} answered with no JSON")));
    }

    Ok(Page {
        datapoints: answer.get("Datapoints").cloned().unwrap_or(Value::Null),
        next_token: answer
            .get("NextToken")
            .and_then(Value::as_str)
            .filter(|token| !token.is_empty())
            .map(str::to_string),
    })
}

/// A code CloudMonitor gave as a string or a number.
fn code_text(code: &Value) -> String {
    match code {
        Value::String(code) => code.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// A refusal as an error, by the code Alibaba Cloud gave before the HTTP status.
fn refused_with(status: u16, code: &str, body: &str) -> Error {
    let body: String = body.chars().take(500).collect();
    let message = format!("{API} answered {status}: {body}");

    if code.starts_with("Throttling") {
        Error::resource_exhausted(message)
    } else if code.starts_with("InvalidAccessKeyId")
        || code.starts_with("SignatureDoesNotMatch")
        || code.starts_with("IncompleteSignature")
        || code.starts_with("InvalidSecurityToken")
    {
        Error::unauthorized(message)
    } else if code.starts_with("Forbidden") || code.contains("NoPermission") {
        Error::forbidden(message)
    } else if (200..300).contains(&status) {
        // Refused in a 200 answer: the request itself is wrong (a metric that is not there).
        Error::invalid(message)
    } else {
        refusal(API, status, &body)
    }
}

/// A random string per request, against replays.
fn nonce() -> Result<String> {
    let mut bytes = [0u8; 16];

    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| Error::internal("aliyun signature nonce could not be generated"))?;

    Ok(hex::encode(bytes))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn settings() -> Settings {
        Settings::parse(&json!({
            "accessKey": "id",
            "secretKey": "secret",
            "region": "cn-hangzhou",
            "namespace": "acs_ecs_dashboard",
            "metrics": ["CPUUtilization"],
            "dimensions": "{\"instanceId\":\"i-abc\"}",
        }))
        .unwrap()
    }

    #[test]
    fn a_query_names_the_metric_range_and_page() {
        let query = Query {
            metric: "CPUUtilization",
            start_ms: 1_700_000_099_999,
            end_ms: 1_700_000_399_999,
            length: MAX_PAGE_LENGTH,
            next_token: Some(
                "15761432850009dd70bb64cff1f0fff6c0b08ffff073be5fb1e785e2b020f7fed9b5e137bd810a6d6cff5ae",
            ),
        };

        let params = params(&settings(), &query);
        let value = |name: &str| {
            params
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.as_str())
        };

        assert_eq!(value("Namespace"), Some("acs_ecs_dashboard"));
        assert_eq!(value("MetricName"), Some("CPUUtilization"));
        assert_eq!(value("Period"), Some("300"));
        assert_eq!(value("StartTime"), Some("1700000099999"));
        assert_eq!(value("EndTime"), Some("1700000399999"));
        assert_eq!(value("Length"), Some("1440"));
        assert_eq!(value("Dimensions"), Some("{\"instanceId\":\"i-abc\"}"));
        assert!(value("NextToken").is_some());

        let first = params_without_token();
        assert!(!first.iter().any(|(key, _)| *key == "NextToken"));
    }

    fn params_without_token() -> Vec<(&'static str, String)> {
        params(
            &settings(),
            &Query {
                metric: "CPUUtilization",
                start_ms: 0,
                end_ms: 1,
                length: 1,
                next_token: None,
            },
        )
    }

    /// The answer in the DescribeMetricList reference.
    #[test]
    fn a_page_carries_its_datapoints_and_token() {
        let body = json!({
            "NextToken": "xxxxxx",
            "RequestId": "3121AE7D-4AFF-4C25-8F1D-C8226EBB1F42",
            "Success": true,
            "Datapoints": "[{\"timestamp\":1548777660000,\"userId\":\"120886317861****\",\"instanceId\":\"i-abc\",\"Minimum\":9.92,\"Average\":9.92,\"Maximum\":9.92}]",
            "Code": "200",
            "Message": "The Request is not authorization.",
            "Period": "60"
        })
        .to_string();

        let page = page(200, &body).unwrap();

        assert_eq!(page.next_token.as_deref(), Some("xxxxxx"));
        assert!(page.datapoints.as_str().unwrap().contains("i-abc"));
    }

    #[test]
    fn a_refusal_says_what_kind_it_is() {
        let throttled = json!({"Code": "Throttling.User", "Message": "Request was denied due to user flow control."}).to_string();
        assert!(matches!(
            page(400, &throttled),
            Err(Error::ResourceExhausted(_))
        ));

        let key = json!({"Code": "InvalidAccessKeyId.NotFound", "Message": "Specified access key is not found."}).to_string();
        assert!(matches!(page(404, &key), Err(Error::Unauthorized(_))));

        let ram = json!({"Code": "Forbidden.RAM", "Message": "User not authorized to operate on the specified resource."}).to_string();
        assert!(matches!(page(403, &ram), Err(Error::Forbidden(_))));

        let wrong =
            json!({"Code": "400", "Success": false, "Message": "metric not found"}).to_string();
        assert!(matches!(page(200, &wrong), Err(Error::InvalidArgument(_))));
    }
}
