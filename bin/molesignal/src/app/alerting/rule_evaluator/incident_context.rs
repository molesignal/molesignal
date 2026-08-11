// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Incident 跨信号上下文派生：把触发查询的 `QueryResult` 提炼成 incident 的
//! labels / trace_ids / host_ids / affected_services / sample_values。
//!
//! 纯函数层，不触碰 evaluator 状态；[`super::RuleEvaluator`] 在开 incident 时
//! 通过 [`derive_incident_context`] 调用一次。

use std::collections::BTreeMap;

use crate::{
    domain::{
        alerting::{incident::TriggeringSample, rule::AlertRule},
        query::QueryResult,
    },
    shared::time::TimestampMicros,
};

/// list/detail 暴露的 trace_id 最多保留多少条；过多会让 drawer 拥挤、payload 膨胀。
const MAX_TRACE_IDS_PER_INCIDENT: usize = 10;
/// 触发查询样本最多保留多少行，控制 detail payload 体积。
const MAX_SAMPLE_VALUES_PER_INCIDENT: usize = 20;

/// 从规则 + 查询结果派生 incident 的跨信号上下文。
///
/// 行为：
/// - `labels` 以规则 labels 为基底，被查询结果中带 `service` / `host` / `env` / `region` /
///   `trace_id` 等列的首行覆盖（结果列名作 label key，值取首行的字符串化）。
/// - `trace_ids` / `host_ids` / `affected_services` 扫描所有行的对应列，去重保留顺序，分别裁到上限。
/// - `sample_values`: 取前 N 行，时间戳列尝试解析为 micros，数值列取第一个 numeric column。
pub(super) struct IncidentContext {
    pub(super) labels: BTreeMap<String, String>,
    pub(super) trace_ids: Vec<String>,
    pub(super) host_ids: Vec<String>,
    pub(super) affected_services: Vec<String>,
    pub(super) sample_values: Vec<TriggeringSample>,
}

pub(super) fn derive_incident_context(rule: &AlertRule, q: &QueryResult) -> IncidentContext {
    let mut labels: BTreeMap<String, String> = rule.labels.clone();
    let mut trace_ids: Vec<String> = Vec::new();
    let mut host_ids: Vec<String> = Vec::new();
    let mut affected_services: Vec<String> = Vec::new();

    let trace_col = find_col(&q.columns, &["trace_id", "traceid", "trace.id"]);
    let host_col = find_col(&q.columns, &["host", "host_id", "host.name", "instance"]);
    let service_col = find_col(&q.columns, &["service", "service_name", "service.name"]);
    let ts_col = find_col(&q.columns, &["_timestamp", "ts", "time", "timestamp"]);
    let value_col_idx = first_numeric_col(q);

    for row in &q.rows {
        if let Some(idx) = trace_col {
            push_unique_string(&mut trace_ids, row.get(idx));
        }
        if let Some(idx) = host_col {
            push_unique_string(&mut host_ids, row.get(idx));
        }
        if let Some(idx) = service_col {
            push_unique_string(&mut affected_services, row.get(idx));
        }
    }

    trace_ids.truncate(MAX_TRACE_IDS_PER_INCIDENT);
    host_ids.truncate(MAX_TRACE_IDS_PER_INCIDENT);
    affected_services.truncate(MAX_TRACE_IDS_PER_INCIDENT);

    // 用首行的 service / host 覆盖到 labels（前端 SignalReference 直接能用）。
    if let Some(first) = q.rows.first() {
        if let Some(idx) = service_col
            && let Some(v) = first.get(idx).and_then(json_as_string)
        {
            labels.insert("service".to_string(), v);
        }
        if let Some(idx) = host_col
            && let Some(v) = first.get(idx).and_then(json_as_string)
        {
            labels.insert("host".to_string(), v);
        }
    }

    let sample_values = q
        .rows
        .iter()
        .take(MAX_SAMPLE_VALUES_PER_INCIDENT)
        .map(|row| TriggeringSample {
            ts: ts_col
                .and_then(|idx| row.get(idx))
                .and_then(json_as_i64)
                .map(TimestampMicros)
                .unwrap_or(TimestampMicros(0)),
            value: value_col_idx
                .and_then(|idx| row.get(idx))
                .and_then(json_as_f64)
                .unwrap_or(f64::NAN),
            labels: row_labels(&q.columns, row),
        })
        .collect();

    IncidentContext {
        labels,
        trace_ids,
        host_ids,
        affected_services,
        sample_values,
    }
}

fn find_col(columns: &[String], candidates: &[&str]) -> Option<usize> {
    for cand in candidates {
        if let Some(idx) = columns.iter().position(|c| c.eq_ignore_ascii_case(cand)) {
            return Some(idx);
        }
    }
    None
}

fn first_numeric_col(q: &QueryResult) -> Option<usize> {
    let row = q.rows.first()?;
    // 跳过时间戳列：触发查询往往以 `_timestamp` 作为首列，把它当作数值会让
    // sample_values.value 变成微秒时间戳。
    let ts_names = ["_timestamp", "ts", "time", "timestamp"];
    row.iter().enumerate().find_map(|(idx, v)| {
        let is_ts = q
            .columns
            .get(idx)
            .map(|c| ts_names.iter().any(|t| c.eq_ignore_ascii_case(t)))
            .unwrap_or(false);
        if !is_ts && (v.is_f64() || v.is_i64() || v.is_u64()) {
            Some(idx)
        } else {
            None
        }
    })
}

fn push_unique_string(dst: &mut Vec<String>, v: Option<&serde_json::Value>) {
    if let Some(s) = v.and_then(json_as_string)
        && !s.is_empty()
        && !dst.iter().any(|existing| existing == &s)
    {
        dst.push(s);
    }
}

fn json_as_string(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn json_as_f64(v: &serde_json::Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_i64().map(|i| i as f64))
        .or_else(|| v.as_u64().map(|u| u as f64))
}

fn json_as_i64(v: &serde_json::Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_u64().and_then(|u| i64::try_from(u).ok()))
        .or_else(|| v.as_f64().map(|f| f as i64))
}

fn row_labels(columns: &[String], row: &[serde_json::Value]) -> BTreeMap<String, String> {
    columns
        .iter()
        .zip(row.iter())
        .filter_map(|(c, v)| json_as_string(v).map(|s| (c.clone(), s)))
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        domain::{
            alerting::rule::{AlertQuery, AlertRuleKind, AlertTrigger, ComparisonOp, RuleState},
            query::{QueryLanguage, StreamHint},
            stream::StreamType,
        },
        shared::ids::Id,
    };

    fn sample_rule() -> AlertRule {
        AlertRule {
            id: Id::from_string("rule-1"),
            org_id: Id::from_string("org-1"),
            name: "errors".into(),
            description: String::new(),
            enabled: true,
            kind: AlertRuleKind::Scheduled,
            query: AlertQuery {
                language: QueryLanguage::Sql,
                statement: "SELECT count(*) FROM logs".into(),
                period_secs: 60,
                stream: Some(StreamHint {
                    name: "logs".into(),
                    stream_type: StreamType::Logs,
                }),
            },
            anomaly_params: None,
            trigger: AlertTrigger {
                operator: ComparisonOp::Gt,
                threshold: 5.0,
                for_periods: 1,
                silence_secs: 0,
            },
            thresholds: vec![],
            severity: None,
            escalation_policy_id: Id::from_string("pol-1"),
            labels: [("env".to_string(), "prod".to_string())].into(),
            annotations: [("runbook".to_string(), "https://wiki/r".to_string())].into(),
            last_eval_at: None,
            last_state: RuleState::Healthy,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn derives_trace_host_service_from_query_columns() {
        let rule = sample_rule();
        let q = QueryResult {
            columns: vec![
                "_timestamp".into(),
                "trace_id".into(),
                "service".into(),
                "host".into(),
                "value".into(),
            ],
            rows: vec![
                vec![
                    json!(1_700_000_000_000_000_i64),
                    json!("t-1"),
                    json!("api"),
                    json!("h-1"),
                    json!(42.0),
                ],
                vec![
                    json!(1_700_000_001_000_000_i64),
                    json!("t-2"),
                    json!("api"),
                    json!("h-2"),
                    json!(43.0),
                ],
            ],
            scanned_rows: 2,
            took_ms: 1,
            federation: None,
        };
        let ctx = derive_incident_context(&rule, &q);
        assert_eq!(ctx.trace_ids, vec!["t-1", "t-2"]);
        assert_eq!(ctx.host_ids, vec!["h-1", "h-2"]);
        assert_eq!(ctx.affected_services, vec!["api"]);
        // rule.labels (env=prod) 应保留，并被首行的 service/host 覆盖到 labels。
        assert_eq!(ctx.labels.get("env").map(|s| s.as_str()), Some("prod"));
        assert_eq!(ctx.labels.get("service").map(|s| s.as_str()), Some("api"));
        assert_eq!(ctx.labels.get("host").map(|s| s.as_str()), Some("h-1"));
        // sample 第一行：value=42, ts=1.7e15
        let s0 = &ctx.sample_values[0];
        assert_eq!(s0.ts.0, 1_700_000_000_000_000);
        assert!((s0.value - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn caps_trace_ids_at_max() {
        let rule = sample_rule();
        let rows: Vec<Vec<serde_json::Value>> = (0..30)
            .map(|i| vec![json!(format!("t-{i}")), json!(1.0)])
            .collect();
        let q = QueryResult {
            columns: vec!["trace_id".into(), "value".into()],
            rows,
            scanned_rows: 30,
            took_ms: 1,
            federation: None,
        };
        let ctx = derive_incident_context(&rule, &q);
        assert_eq!(ctx.trace_ids.len(), MAX_TRACE_IDS_PER_INCIDENT);
        assert_eq!(ctx.sample_values.len(), MAX_SAMPLE_VALUES_PER_INCIDENT);
    }
}
