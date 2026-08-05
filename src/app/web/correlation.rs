// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cross-signal correlation derive helpers（web-investigation-shell）。
//!
//! 纯函数：根据 (from_kind, to_kind) 把 `ctx` payload 翻译成下一信号的 filters / prefill。
//! handler 用这些函数 + service_graph 查询做 metric→trace 加成。

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Filter {
    pub field: String,
    pub op: String,
    pub value: Value,
}

#[derive(Debug, Serialize, Default, PartialEq, Eq)]
pub struct Prefill {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sql: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promql: Option<String>,
}

pub fn extract_str(payload: &Value, key: &str) -> Option<String> {
    payload.get(key).and_then(Value::as_str).map(String::from)
}

pub fn extract_services(payload: &Value) -> Vec<String> {
    payload
        .get("services")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

pub fn filters_for_services(svcs: &[String]) -> Vec<Filter> {
    if svcs.is_empty() {
        return Vec::new();
    }
    if svcs.len() == 1 {
        vec![Filter {
            field: "service".into(),
            op: "eq".into(),
            value: Value::String(svcs[0].clone()),
        }]
    } else {
        vec![Filter {
            field: "service".into(),
            op: "in".into(),
            value: Value::Array(svcs.iter().cloned().map(Value::String).collect()),
        }]
    }
}

pub fn derive_trace_to_log(payload: &Value) -> (Vec<Filter>, Prefill) {
    let svcs = extract_services(payload);
    let mut filters = filters_for_services(&svcs);
    if let Some(trace_id) = extract_str(payload, "trace_id") {
        filters.push(Filter {
            field: "trace_id".into(),
            op: "eq".into(),
            value: Value::String(trace_id),
        });
    }
    (filters, Prefill::default())
}

pub fn derive_trace_to_metric(payload: &Value) -> (Vec<Filter>, Prefill) {
    let svcs = extract_services(payload);
    let filters = filters_for_services(&svcs);
    let promql = svcs
        .first()
        .map(|s| format!("rate(http_requests_total{{service=\"{s}\"}}[1m])"));
    (filters, Prefill { sql: None, promql })
}

pub fn derive_trace_to_host(payload: &Value) -> (Vec<Filter>, Prefill) {
    let mut filters = Vec::new();
    if let Some(host) = extract_str(payload, "host") {
        filters.push(Filter {
            field: "host".into(),
            op: "eq".into(),
            value: Value::String(host),
        });
    }
    (filters, Prefill::default())
}

pub fn derive_metric_to_log(payload: &Value) -> (Vec<Filter>, Prefill) {
    let svcs = extract_services(payload);
    (filters_for_services(&svcs), Prefill::default())
}

pub fn derive_log_to_trace(payload: &Value) -> (Vec<Filter>, Prefill) {
    let mut filters = Vec::new();
    if let Some(trace_id) = extract_str(payload, "trace_id") {
        filters.push(Filter {
            field: "trace_id".into(),
            op: "eq".into(),
            value: Value::String(trace_id),
        });
    }
    if let Some(svc) = extract_str(payload, "service") {
        filters.push(Filter {
            field: "service".into(),
            op: "eq".into(),
            value: Value::String(svc),
        });
    }
    (filters, Prefill::default())
}

pub fn derive_log_to_metric(payload: &Value) -> (Vec<Filter>, Prefill) {
    let svcs = extract_services(payload);
    let svc_one = svcs
        .first()
        .cloned()
        .or_else(|| extract_str(payload, "service"));
    let promql = svc_one
        .as_ref()
        .map(|s| format!("rate(log_records_total{{service=\"{s}\"}}[1m])"));
    let mut filters = filters_for_services(&svcs);
    if let (Some(s), true) = (svc_one, svcs.is_empty()) {
        filters.push(Filter {
            field: "service".into(),
            op: "eq".into(),
            value: Value::String(s),
        });
    }
    (filters, Prefill { sql: None, promql })
}

pub fn derive_host_to_trace(payload: &Value) -> (Vec<Filter>, Prefill) {
    let mut filters = Vec::new();
    if let Some(host) = extract_str(payload, "host") {
        filters.push(Filter {
            field: "host.name".into(),
            op: "eq".into(),
            value: Value::String(host),
        });
    }
    (filters, Prefill::default())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn trace_to_log_picks_trace_id_and_services() {
        let (filters, _) = derive_trace_to_log(&json!({
            "trace_id": "abc",
            "services": ["web", "api"]
        }));
        assert!(filters.iter().any(|f| f.field == "trace_id"));
        let svc = filters.iter().find(|f| f.field == "service").unwrap();
        assert_eq!(svc.op, "in");
    }

    #[test]
    fn trace_to_metric_emits_rate_promql() {
        let (_, p) = derive_trace_to_metric(&json!({"services": ["web"]}));
        assert!(p.promql.unwrap().contains("rate("));
    }

    #[test]
    fn host_to_trace_passes_host_filter() {
        let (filters, _) = derive_host_to_trace(&json!({"host": "node-1"}));
        assert_eq!(filters[0].field, "host.name");
    }
}
