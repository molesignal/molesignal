// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 首次体验：一键加载关联好的样例 telemetry（logs + metrics + traces 共享 trace_id），
//! 让新实例 Home/activation 的「sample data」步骤从 backend-pending 变为可用——新用户
//! 无需先跑外部 seed 脚本就能在几秒内复现一次跨信号下钻。
//!
//! - `POST /onboarding/sample-data`：在当前 org ingest 一批内置样例（复用 IngestService
//!   的 schema-on-write 自动建流），返回写入的流与行数。
//! - `GET  /onboarding/sample-data`：探测样例流是否已存在，供前端 activation 判定 completed。
//!
//! 数据是确定性生成的（相对当前时刻铺开过去几小时），不依赖随机数，重复加载只是按
//! 新时间窗追加。模拟 `frontend → checkout → payment → db` 调用链：每个 trace 的 span
//! 与日志共享同一 trace_id，metrics 给出各 service 的时延曲线。

use axum::{Extension, Json, Router, extract::State, routing::post};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::permission,
        ingestion::{IngestBatch, RawEvent},
        stream::StreamType,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

const SAMPLE_LOGS: &str = "sample_app_logs";
const SAMPLE_METRICS: &str = "sample_app_metrics";
const SAMPLE_TRACES: &str = "sample_app_traces";

/// 调用链上的服务，按 span 父子顺序排列（frontend 为根）。
const SERVICES: [&str; 4] = ["frontend", "checkout", "payment", "db"];
/// 各 service 的基线时延（毫秒），叶子（db）最小。
const BASE_LATENCY_MS: [f64; 4] = [120.0, 90.0, 60.0, 25.0];

const SAMPLE_TRACES_COUNT: i64 = 6;
const MIN_US: i64 = 60 * 1_000_000;

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/onboarding/sample-data",
        post(load_sample_data).get(sample_data_status),
    )
}

#[derive(Serialize)]
struct SampleDataStatus {
    loaded: bool,
}

#[derive(Serialize)]
struct LoadedStream {
    stream: String,
    rows: usize,
}

#[derive(Serialize)]
struct LoadResult {
    loaded: bool,
    total_rows: usize,
    streams: Vec<LoadedStream>,
}

/// 探测样例流是否已存在 → 前端 activation 的 `sampleDataAvailable`。
#[permission(any("streams.query", "sys.telemetry.read"))]
async fn sample_data_status(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<SampleDataStatus>> {
    let loaded = state
        .telemetry
        .streams
        .get(&ctx.org_id, SAMPLE_LOGS, StreamType::Logs)
        .await
        .is_ok();
    Ok(Json(SampleDataStatus { loaded }))
}

/// 一键加载样例数据：三个流各 ingest 一批。计费门禁特意跳过——样例数据量小、属
/// onboarding 便利，不应占用租户配额，也避免新实例尚未配 billing 时被 402 挡住。
#[permission("streams.write")]
async fn load_sample_data(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<LoadResult>> {
    let now_us = TimestampMicros::now().0;

    let mut streams = Vec::new();
    let mut total_rows = 0;
    for (stream_type, name, events) in sample_batches(now_us) {
        let rows = events.len();
        let batch = IngestBatch {
            batch_id: Id::new(),
            org_id: ctx.org_id.clone(),
            stream: name.to_string(),
            stream_type,
            events,
            received_at: TimestampMicros::now(),
        };
        state.ingestion.ingest(batch).await?;
        total_rows += rows;
        streams.push(LoadedStream {
            stream: name.to_string(),
            rows,
        });
    }

    Ok(Json(LoadResult {
        loaded: true,
        total_rows,
        streams,
    }))
}

/// 三个流的样例事件（traces / logs / metrics），共享 trace_id 串成跨信号关联。
fn sample_batches(now_us: i64) -> Vec<(StreamType, &'static str, Vec<RawEvent>)> {
    vec![
        (StreamType::Traces, SAMPLE_TRACES, sample_traces(now_us)),
        (StreamType::Logs, SAMPLE_LOGS, sample_logs(now_us)),
        (StreamType::Metrics, SAMPLE_METRICS, sample_metrics(now_us)),
    ]
}

fn event(ts_us: i64, fields: Map<String, Value>) -> RawEvent {
    RawEvent {
        timestamp: TimestampMicros(ts_us),
        fields,
    }
}

/// 第 `t` 条 trace 的 trace_id（32 hex，确定性）。
fn trace_id(t: i64) -> String {
    format!(
        "{:032x}",
        0xA11CE_u64.wrapping_mul(1000).wrapping_add(t as u64)
    )
}

/// 第 `t` 条 trace 起始时刻：往过去铺开，最近的 trace ~18min 前、最早 ~108min 前。
fn trace_start_us(now_us: i64, t: i64) -> i64 {
    now_us - (t + 1) * 18 * MIN_US
}

/// 1/6 的 trace 标记为故障链（payment 报错）。
fn is_error_trace(t: i64) -> bool {
    t % SAMPLE_TRACES_COUNT == SAMPLE_TRACES_COUNT - 2
}

fn sample_traces(now_us: i64) -> Vec<RawEvent> {
    let mut out = Vec::new();
    for t in 0..SAMPLE_TRACES_COUNT {
        let tid = trace_id(t);
        let err = is_error_trace(t);
        let mut span_start = trace_start_us(now_us, t);
        let mut parent = String::new();
        for (i, svc) in SERVICES.iter().enumerate() {
            let span_id = format!("{:016x}", ((t as u64) << 16) | (i as u64 + 1));
            // 故障链的 payment span 时延翻倍并标 error，其余 OK。
            let payment_fault = err && *svc == "payment";
            let dur_ms = BASE_LATENCY_MS[i] * if payment_fault { 2.4 } else { 1.0 };
            let status_code = if payment_fault { 2 } else { 0 };
            let mut f = Map::new();
            f.insert("trace_id".into(), json!(tid));
            f.insert("span_id".into(), json!(span_id));
            f.insert("parent_span_id".into(), json!(parent));
            f.insert("service.name".into(), json!(svc));
            f.insert("span.name".into(), json!(format!("{svc}.handle")));
            f.insert("duration_ns".into(), json!((dur_ms * 1_000_000.0) as i64));
            f.insert("status_code".into(), json!(status_code));
            out.push(event(span_start, f));
            // 子 span 在父 span 内稍后开始；parent 链下移。
            parent = span_id;
            span_start += (dur_ms * 0.2 * 1_000_000.0) as i64;
        }
    }
    out
}

fn sample_logs(now_us: i64) -> Vec<RawEvent> {
    let mut out = Vec::new();
    for t in 0..SAMPLE_TRACES_COUNT {
        let tid = trace_id(t);
        let err = is_error_trace(t);
        let base = trace_start_us(now_us, t);
        for (i, svc) in SERVICES.iter().enumerate() {
            let payment_fault = err && *svc == "payment";
            let (level, message) = if payment_fault {
                (
                    "error",
                    "payment gateway timeout after 3 retries".to_string(),
                )
            } else {
                ("info", format!("{svc} handled request ok"))
            };
            let mut f = Map::new();
            f.insert("trace_id".into(), json!(tid));
            f.insert("service".into(), json!(svc));
            f.insert("level".into(), json!(level));
            f.insert("message".into(), json!(message));
            out.push(event(base + (i as i64) * 5 * 1_000_000, f));
        }
    }
    out
}

fn sample_metrics(now_us: i64) -> Vec<RawEvent> {
    // 每个 service 一条 latency 时序，过去 3 小时每 5 分钟一个点。
    const STEPS: i64 = 37;
    const STEP_US: i64 = 5 * MIN_US;
    let mut out = Vec::new();
    for (i, svc) in SERVICES.iter().enumerate() {
        let base = BASE_LATENCY_MS[i];
        for step in 0..STEPS {
            let ts = now_us - (STEPS - 1 - step) * STEP_US;
            // 确定性日内波动 + 轻微随服务相位偏移，给 Metrics 页一条可看的曲线。
            let phase = (step as f64) * 0.35 + (i as f64) * 0.8;
            let value = base + base * 0.12 * phase.sin() + (step % 7) as f64;
            let mut f = Map::new();
            f.insert("value".into(), json!(value));
            f.insert("service".into(), json!(svc));
            out.push(event(ts, f));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    const NOW: i64 = 1_700_000_000_000_000;

    fn trace_ids(events: &[RawEvent]) -> BTreeSet<String> {
        events
            .iter()
            .filter_map(|e| {
                e.fields
                    .get("trace_id")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .collect()
    }

    #[test]
    fn traces_and_logs_share_the_same_trace_ids() {
        // 跨信号关联的前提：logs 与 traces 必须落在同一组 trace_id 上。
        let traces = sample_traces(NOW);
        let logs = sample_logs(NOW);
        assert_eq!(
            traces.len() as i64,
            SAMPLE_TRACES_COUNT * SERVICES.len() as i64
        );
        assert_eq!(
            logs.len() as i64,
            SAMPLE_TRACES_COUNT * SERVICES.len() as i64
        );
        assert_eq!(trace_ids(&traces), trace_ids(&logs));
        assert_eq!(trace_ids(&traces).len() as i64, SAMPLE_TRACES_COUNT);
    }

    #[test]
    fn includes_a_failing_span_and_log() {
        let traces = sample_traces(NOW);
        let logs = sample_logs(NOW);
        let err_spans = traces
            .iter()
            .filter(|e| e.fields.get("status_code").and_then(|v| v.as_i64()) == Some(2))
            .count();
        let err_logs = logs
            .iter()
            .filter(|e| e.fields.get("level").and_then(|v| v.as_str()) == Some("error"))
            .count();
        assert!(err_spans >= 1, "demo must show at least one failing span");
        assert!(err_logs >= 1, "demo must show at least one error log");
    }

    #[test]
    fn metrics_carry_finite_value_and_service() {
        let metrics = sample_metrics(NOW);
        assert!(!metrics.is_empty());
        for e in &metrics {
            let v = e
                .fields
                .get("value")
                .and_then(|v| v.as_f64())
                .expect("value present");
            assert!(
                v.is_finite() && v > 0.0,
                "latency must be positive and finite"
            );
            assert!(e.fields.get("service").and_then(|v| v.as_str()).is_some());
        }
    }

    #[test]
    fn generation_is_deterministic() {
        // 无随机：同一时刻两次生成逐字段一致（重复加载只是时间窗平移，行为可预期）。
        let a = sample_metrics(NOW);
        let b = sample_metrics(NOW);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.timestamp.0, y.timestamp.0);
            assert_eq!(x.fields.get("value"), y.fields.get("value"));
        }
    }
}
