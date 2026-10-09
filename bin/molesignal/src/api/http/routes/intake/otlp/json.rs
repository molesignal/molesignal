// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! OTLP/JSON 解码前的规范化。
//!
//! OTLP/JSON 规范把 int64 编码成十进制字符串（`"asInt": "340"`），而 `opentelemetry-proto`
//! 的 serde 类型只接受数字形式的 `asInt`。解码失败发生在 `#[serde(flatten)]` 的 oneof 里，
//! 会被静默吞掉，数据点于是丢失 `value`。解码前统一把 `asInt` 的字符串改写成数字。

use serde_json::Value;

/// 把所有 `asInt` 的字符串值改写成 JSON 数字；无法解析成 `i64` 的值保持原样，交给后续解码报错。
pub(super) fn normalize_as_int(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let parsed = match map.get("asInt") {
                Some(Value::String(text)) => text.trim().parse::<i64>().ok(),
                _ => None,
            };
            if let Some(int) = parsed {
                map.insert("asInt".to_owned(), Value::from(int));
            }
            for child in map.values_mut() {
                normalize_as_int(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_as_int(item);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Bytes;
    use opentelemetry_proto::tonic::collector::{
        logs::v1::ExportLogsServiceRequest, metrics::v1::ExportMetricsServiceRequest,
    };
    use serde_json::json;

    use super::{
        super::{OtlpEncoding, decode_otlp, metrics_to_events},
        *,
    };

    fn field<'a>(
        events: &'a [crate::domain::intake::RawEvent],
        metric: &str,
        key: &str,
    ) -> Option<&'a Value> {
        events
            .iter()
            .find(|event| event.fields.get("metric_name") == Some(&Value::from(metric)))
            .and_then(|event| event.fields.get(key))
    }

    #[test]
    fn normalizes_nested_string_int64_values_only() {
        let mut value = json!({
            "a": [{"asInt": "340"}, {"asInt": 7}, {"asInt": "not a number"}],
            "b": {"nested": {"asInt": " 12 "}},
            "asDouble": "3"
        });
        normalize_as_int(&mut value);
        assert_eq!(value["a"][0]["asInt"], json!(340));
        assert_eq!(value["a"][1]["asInt"], json!(7));
        assert_eq!(value["a"][2]["asInt"], json!("not a number"));
        assert_eq!(value["b"]["nested"]["asInt"], json!(12));
        assert_eq!(value["asDouble"], json!("3"));
    }

    /// 规范写法的 OTLP/JSON 指标：gauge 的 `asDouble`、sum 的字符串 `asInt`、histogram 的
    /// 字符串 `count` 都必须落成字段；此前它们全部静默丢失。
    #[test]
    fn metrics_json_keeps_values_and_histograms() {
        let body = json!({"resourceMetrics": [{
            "resource": {"attributes": [{"key": "service.name", "value": {"stringValue": "checkout"}}]},
            "scopeMetrics": [{"scope": {"name": "docs"}, "metrics": [
                {"name": "queue.depth", "gauge": {"dataPoints": [
                    {"timeUnixNano": "1791127631584409088", "asDouble": 12.5}]}},
                {"name": "http.requests", "sum": {
                    "aggregationTemporality": 2, "isMonotonic": true,
                    "dataPoints": [{"startTimeUnixNano": "1791127571584409088",
                                    "timeUnixNano": "1791127631584409088", "asInt": "340"}]}},
                {"name": "http.duration", "histogram": {
                    "aggregationTemporality": 1,
                    "dataPoints": [{"timeUnixNano": "1791127631584409088", "count": "4",
                                    "sum": 210.0, "min": 5.0, "max": 150.0,
                                    "bucketCounts": ["1", "2", "1"], "explicitBounds": [10.0, 100.0]}]}}
            ]}]
        }]})
        .to_string();

        let request: ExportMetricsServiceRequest =
            decode_otlp(OtlpEncoding::Json, &Bytes::from(body)).expect("decode metrics");
        let events = metrics_to_events(request);

        assert_eq!(events.len(), 3);
        assert_eq!(
            field(&events, "queue.depth", "value"),
            Some(&Value::from(12.5))
        );
        assert_eq!(
            field(&events, "http.requests", "value"),
            Some(&Value::from(340))
        );
        assert_eq!(
            field(&events, "http.duration", "count"),
            Some(&Value::from(4))
        );
        assert_eq!(
            field(&events, "http.duration", "sum"),
            Some(&Value::from(210.0))
        );
        assert_eq!(
            field(&events, "http.duration", "max"),
            Some(&Value::from(150.0))
        );
    }

    /// 回归保护：工作区一旦重新启用 `serde_json/arbitrary_precision`，带小数的 `doubleValue`
    /// 会在 `#[serde(flatten)]` 里解码失败（`invalid type: map, expected f64`）。
    #[test]
    fn fractional_double_attributes_decode() {
        let body = json!({"resourceLogs": [{"scopeLogs": [{"logRecords": [{
            "timeUnixNano": "1791127631584409088",
            "body": {"stringValue": "hello"},
            "attributes": [{"key": "ratio", "value": {"doubleValue": 0.25}}]
        }]}]}]})
        .to_string();

        let request: ExportLogsServiceRequest =
            decode_otlp(OtlpEncoding::Json, &Bytes::from(body)).expect("decode logs");
        let attribute = &request.resource_logs[0].scope_logs[0].log_records[0].attributes[0];
        assert_eq!(attribute.key, "ratio");
        assert!(attribute.value.is_some());
    }
}
