// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /api/v1/web/correlation/:from_kind/:to_kind?ctx=<base64>` ——
//! 服务端补强 metric ↔ trace ↔ log ↔ host 跨信号跳转的 filter / time / prefill。
//!
//! 客户端 400ms 内拿不到结果会 fallback 到本地 provider，所以 handler 优先快、
//! 弱依赖；具体策略：每 (from, to) 对一个 helper，缺数据时返空 filters。
//!
//! 支持的 8 对（其余对返空 prefill 让客户端兜底）：
//! - trace → log / metric / host
//! - metric → trace / log
//! - log → trace / metric
//! - host → trace

use axum::{
    Extension, Router,
    extract::{Path, Query, State},
    response::Json,
    routing::get,
};
use base64::engine::{Engine, general_purpose::URL_SAFE_NO_PAD};
use chrono::Duration as ChronoDuration;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{api::AppState, app::iam::IamContext, domain::iam::permission, shared::Error};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/correlation/providers", get(providers))
        .route("/correlation/{from_kind}/{to_kind}", get(correlation))
}

#[derive(Debug, Deserialize)]
pub struct CorrQuery {
    pub ctx: String, // base64-encoded JSON blob
}

#[derive(Debug, Serialize)]
pub struct TimeRange {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Serialize)]
pub struct Filter {
    pub field: String,
    pub op: String,
    pub value: Value,
}

#[derive(Debug, Serialize, Default)]
pub struct Prefill {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sql: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promql: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CorrelationResponse {
    pub time_range: TimeRange,
    pub filters: Vec<Filter>,
    pub prefill: Prefill,
}

#[derive(Debug, Serialize)]
pub struct CorrelationProvider {
    pub id: &'static str,
    pub from_kind: &'static str,
    pub to_kind: &'static str,
    pub label: &'static str,
    pub enabled: bool,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn providers(
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<CorrelationProvider>>, Error> {
    Ok(Json(vec![
        provider("trace-log", "trace", "log", "Trace to logs"),
        provider("trace-metric", "trace", "metric", "Trace to metrics"),
        provider("trace-host", "trace", "host", "Trace to host"),
        provider("metric-trace", "metric", "trace", "Metric to traces"),
        provider("metric-log", "metric", "log", "Metric to logs"),
        provider("log-trace", "log", "trace", "Log to traces"),
        provider("log-metric", "log", "metric", "Log to metrics"),
        provider("host-trace", "host", "trace", "Host to traces"),
    ]))
}

fn provider(
    id: &'static str,
    from_kind: &'static str,
    to_kind: &'static str,
    label: &'static str,
) -> CorrelationProvider {
    CorrelationProvider {
        id,
        from_kind,
        to_kind,
        label,
        enabled: true,
    }
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn correlation(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((from_kind, to_kind)): Path<(String, String)>,
    Query(q): Query<CorrQuery>,
) -> Result<Json<CorrelationResponse>, Error> {
    let decoded = URL_SAFE_NO_PAD
        .decode(q.ctx.as_bytes())
        .map_err(|e| Error::invalid(format!("invalid ctx base64: {e}")))?;
    let payload: Value = serde_json::from_slice(&decoded)
        .map_err(|e| Error::invalid(format!("invalid ctx json: {e}")))?;

    // 时间窗：payload 优先；没有则 fallback "过去 1 分钟"
    let (from, to) = derive_time_window(&payload);

    let (filters, prefill) = match (from_kind.as_str(), to_kind.as_str()) {
        ("trace", "log") => derive_trace_to_log(&payload),
        ("trace", "metric") => derive_trace_to_metric(&payload),
        ("trace", "host") => derive_trace_to_host(&payload),
        ("metric", "trace") => {
            derive_metric_to_trace(&state, &ctx.org_id, &payload, &from, &to).await
        }
        ("metric", "log") => derive_metric_to_log(&payload),
        ("log", "trace") => derive_log_to_trace(&payload),
        ("log", "metric") => derive_log_to_metric(&payload),
        ("host", "trace") => derive_host_to_trace(&payload),
        _ => (Vec::new(), Prefill::default()),
    };

    Ok(Json(CorrelationResponse {
        time_range: TimeRange { from, to },
        filters,
        prefill,
    }))
}

fn derive_time_window(payload: &Value) -> (String, String) {
    let now = chrono::Utc::now();
    let from = payload
        .get("time_range")
        .and_then(|t| t.get("from"))
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| (now - ChronoDuration::seconds(60)).to_rfc3339());
    let to = payload
        .get("time_range")
        .and_then(|t| t.get("to"))
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| now.to_rfc3339());
    (from, to)
}

fn extract_str(payload: &Value, key: &str) -> Option<String> {
    payload.get(key).and_then(Value::as_str).map(String::from)
}

fn extract_services(payload: &Value) -> Vec<String> {
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

fn filters_for_services(svcs: &[String]) -> Vec<Filter> {
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

// === provider helpers ===

fn derive_trace_to_log(payload: &Value) -> (Vec<Filter>, Prefill) {
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

fn derive_trace_to_metric(payload: &Value) -> (Vec<Filter>, Prefill) {
    let svcs = extract_services(payload);
    let filters = filters_for_services(&svcs);
    let promql = svcs
        .first()
        .map(|s| format!("rate(http_requests_total{{service=\"{s}\"}}[1m])"));
    (filters, Prefill { sql: None, promql })
}

fn derive_trace_to_host(payload: &Value) -> (Vec<Filter>, Prefill) {
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

async fn derive_metric_to_trace(
    state: &AppState,
    org_id: &crate::shared::ids::Id,
    payload: &Value,
    from: &str,
    to: &str,
) -> (Vec<Filter>, Prefill) {
    let svcs = extract_services(payload);
    let mut filters = filters_for_services(&svcs);

    // 若给了 service，去 service_graph_edges 找一个最近活跃调用方，
    // 给前端一个 "可点击的 trace 候选" hint。
    if let Some(svc) = svcs.first() {
        let from_us = chrono::DateTime::parse_from_rfc3339(from)
            .map(|d| d.timestamp_micros())
            .unwrap_or(0);
        let to_us = chrono::DateTime::parse_from_rfc3339(to)
            .map(|d| d.timestamp_micros())
            .unwrap_or(i64::MAX);
        if let Ok(edges) = state
            .telemetry
            .service_graph
            .query(org_id, from_us, to_us, Some(svc))
            .await
            && let Some(peer) = edges.iter().find_map(|e| {
                if &e.client_service != svc {
                    Some(e.client_service.clone())
                } else if &e.server_service != svc {
                    Some(e.server_service.clone())
                } else {
                    None
                }
            })
        {
            filters.push(Filter {
                field: "peer_service".into(),
                op: "eq".into(),
                value: Value::String(peer),
            });
        }
    }
    (filters, Prefill::default())
}

fn derive_metric_to_log(payload: &Value) -> (Vec<Filter>, Prefill) {
    let svcs = extract_services(payload);
    (filters_for_services(&svcs), Prefill::default())
}

fn derive_log_to_trace(payload: &Value) -> (Vec<Filter>, Prefill) {
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

fn derive_log_to_metric(payload: &Value) -> (Vec<Filter>, Prefill) {
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

fn derive_host_to_trace(payload: &Value) -> (Vec<Filter>, Prefill) {
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
        let (_filters, p) = derive_trace_to_metric(&json!({"services": ["web"]}));
        assert!(p.promql.unwrap().contains("rate("));
    }

    #[test]
    fn host_to_trace_passes_host_filter() {
        let (filters, _) = derive_host_to_trace(&json!({"host": "node-1"}));
        assert_eq!(filters[0].field, "host.name");
    }
}
