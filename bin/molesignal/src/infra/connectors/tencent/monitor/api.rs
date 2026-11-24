// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Tencent Cloud API 3.0 calls: a JSON body posted to `<service>.tencentcloudapi.com`, signed
//! with TC3-HMAC-SHA256 as the notification adapters sign theirs.

use chrono::{DateTime, FixedOffset};
use kernel::{Error, Result, time::TimestampMicros};
use serde_json::{Value, json};
use signals::http_trace::{self, HttpTarget};

use super::settings::{Instance, Settings};
use crate::infra::{connectors::cloud_metrics::refusal, notify::adapters::tencent_sign as sign};

const CONTENT_TYPE: &str = "application/json; charset=utf-8";

/// A Tencent Cloud API: its service, host and version.
#[derive(Debug, Clone, Copy)]
pub(super) struct Api {
    pub service: &'static str,
    pub version: &'static str,
}

pub(super) const MONITOR: Api = Api {
    service: "monitor",
    version: "2018-07-24",
};

pub(super) const CVM: Api = Api {
    service: "cvm",
    version: "2017-03-12",
};

/// Calls `action` with `payload` and returns its `Response`.
pub(super) async fn call(
    client: &reqwest::Client,
    settings: &Settings,
    api: Api,
    action: &str,
    payload: &Value,
) -> Result<Value> {
    let host = format!("{}.tencentcloudapi.com", api.service);
    let payload = payload.to_string();
    let timestamp = TimestampMicros::now().0 / 1_000_000;

    let authorization = sign::authorization(&sign::SignedCall {
        secret_id: &settings.secret_id,
        secret_key: &settings.secret_key,
        service: api.service,
        host: &host,
        action,
        content_type: CONTENT_TYPE,
        payload: &payload,
        timestamp,
    });

    let request = client
        .post(format!("https://{host}/"))
        .header("authorization", authorization)
        .header("content-type", CONTENT_TYPE)
        .header("x-tc-action", action)
        .header("x-tc-version", api.version)
        .header("x-tc-timestamp", timestamp.to_string())
        .header("x-tc-region", &settings.region)
        .body(payload);

    let what = format!("tencent cloud {action}");

    let response = http_trace::send(client, request, HttpTarget::ThirdParty)
        .await
        .map_err(|e| Error::unavailable(format!("{what} request: {e}")))?;

    let status = response.status().as_u16();

    let body = response
        .text()
        .await
        .map_err(|e| Error::unavailable(format!("{what} body: {e}")))?;

    answer(&what, status, &body)
}

/// The `Response` of an answer, or why the answer is a refusal.
pub(super) fn answer(what: &str, status: u16, body: &str) -> Result<Value> {
    if !(200..300).contains(&status) {
        return Err(refusal(what, status, body));
    }

    let mut answer: Value = serde_json::from_str(body)
        .map_err(|e| Error::unavailable(format!("{what} answered with no JSON: {e}")))?;

    let response = answer
        .get_mut("Response")
        .map(Value::take)
        .unwrap_or(Value::Null);

    let Some(error) = response.get("Error") else {
        return Ok(response);
    };

    let code = error
        .get("Code")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let message = error
        .get("Message")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let message = format!("{what} was refused: {code}: {message}");

    Err(
        if code.starts_with("AuthFailure.UnauthorizedOperation")
            || code.starts_with("UnauthorizedOperation")
            || code.starts_with("OperationDenied")
        {
            Error::forbidden(message)
        } else if code.starts_with("AuthFailure") {
            Error::unauthorized(message)
        } else if code.starts_with("RequestLimitExceeded") {
            Error::resource_exhausted(message)
        } else if code.starts_with("ResourceNotFound") {
            Error::not_found(message)
        } else if code.starts_with("InvalidParameter")
            || code.starts_with("MissingParameter")
            || code.starts_with("UnknownParameter")
            || code.starts_with("UnsupportedOperation")
            || code.starts_with("InvalidAction")
        {
            Error::invalid(message)
        } else {
            Error::unavailable(message)
        },
    )
}

/// A time as Tencent Cloud Monitor takes it: ISO 8601 with the offset, in China Standard Time as
/// the reference shows (`2018-09-22T19:51:23+08:00`).
pub(super) fn iso_time(seconds: i64) -> String {
    let china = FixedOffset::east_opt(8 * 3600).expect("a valid offset");

    DateTime::from_timestamp(seconds, 0)
        .unwrap_or_default()
        .with_timezone(&china)
        .to_rfc3339()
}

/// The `GetMonitorData` body for one metric of up to 50 instances over `[start, end)`.
pub(super) fn monitor_data_request(
    settings: &Settings,
    metric: &str,
    instances: &[Instance],
    start: i64,
    end: i64,
) -> Value {
    let instances: Vec<Value> = instances
        .iter()
        .map(|dimensions| {
            let dimensions: Vec<Value> = dimensions
                .iter()
                .map(|(name, value)| json!({ "Name": name, "Value": value }))
                .collect();

            json!({ "Dimensions": dimensions })
        })
        .collect();

    json!({
        "Namespace": settings.namespace,
        "MetricName": metric,
        "Period": settings.schedule.period_secs,
        "StartTime": iso_time(start),
        // The end is inclusive; a second back leaves out the period that starts at `end`.
        "EndTime": iso_time(end - 1),
        "Instances": instances,
        "SpecifyStatistics": settings.statistics_mask(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_carry_the_china_offset() {
        // 2023-11-14T22:13:20Z.
        assert_eq!(iso_time(1_700_000_000), "2023-11-15T06:13:20+08:00");
    }

    #[test]
    fn a_monitor_data_request_names_instances_and_statistics() {
        let settings = Settings::parse(&json!({
            "accessKey": "AKIDexample",
            "secretKey": "secret",
            "region": "ap-guangzhou",
            "namespace": "QCE/CVM",
            "metrics": ["CpuUsage"],
            "statistics": "avg,max",
            "period": 60,
        }))
        .unwrap();

        let instances = vec![vec![("InstanceId".to_string(), "ins-p2lgmb3s".to_string())]];
        let body = monitor_data_request(
            &settings,
            "CpuUsage",
            &instances,
            1_700_000_000,
            1_700_000_300,
        );

        assert_eq!(
            body,
            json!({
                "Namespace": "QCE/CVM",
                "MetricName": "CpuUsage",
                "Period": 60,
                "StartTime": "2023-11-15T06:13:20+08:00",
                "EndTime": "2023-11-15T06:18:19+08:00",
                "Instances": [{"Dimensions": [{"Name": "InstanceId", "Value": "ins-p2lgmb3s"}]}],
                "SpecifyStatistics": 3,
            })
        );
    }

    #[test]
    fn an_error_in_the_response_is_a_refusal_of_its_kind() {
        let body = |code: &str| {
            json!({"Response": {"Error": {"Code": code, "Message": "m"}, "RequestId": "r"}})
                .to_string()
        };

        assert!(matches!(
            answer("x", 200, &body("AuthFailure.SignatureFailure")),
            Err(Error::Unauthorized(_))
        ));
        assert!(matches!(
            answer("x", 200, &body("AuthFailure.UnauthorizedOperation")),
            Err(Error::Forbidden(_))
        ));
        assert!(matches!(
            answer("x", 200, &body("RequestLimitExceeded")),
            Err(Error::ResourceExhausted(_))
        ));
        assert!(matches!(
            answer("x", 200, &body("InvalidParameterValue")),
            Err(Error::InvalidArgument(_))
        ));
        assert!(matches!(
            answer("x", 200, &body("InternalError")),
            Err(Error::Unavailable(_))
        ));

        let ok = json!({"Response": {"RequestId": "r", "MetricSet": []}}).to_string();
        assert_eq!(answer("x", 200, &ok).unwrap()["RequestId"], "r");
    }
}
