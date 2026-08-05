// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 从已完成 pipeline、脱敏与类型校验的原始批次投影窄物理读模型。

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::{
    domain::{
        ingestion::{EVENT_ID_FIELD, IngestBatch, RawEvent},
        metrics::{METRIC_KIND_FIELD, METRIC_NAME_FIELD},
        storage::PhysicalDatasetKind,
        stream::StreamType,
    },
    shared::{ids::Id, time::TimestampMicros},
};

const RUM_SESSION_FIELDS: &[&str] = &[
    EVENT_ID_FIELD,
    "session_id",
    "user_id",
    "ip_address",
    "client_ip",
    "ip",
    "started_at_micros",
    "duration_ms",
    "application",
    "service",
    "environment",
    "version",
    "country",
    "browser",
    "device",
    "os",
    "landing_page",
    "last_page",
    "view_count",
    "action_count",
    "error_count",
    "trace_id",
];

const RUM_ERROR_FIELDS: &[&str] = &[
    EVENT_ID_FIELD,
    "fingerprint",
    "message",
    "user_id",
    "session_id",
    "page",
    "version",
    "error_type",
    "application",
    "service",
    "environment",
];

/// 自动派生只在权威 `raw` 批次落盘前调用。返回批次与原批次共享逻辑 stream，
/// 但使用独立的 WAL/buffer/object path。
pub(super) fn project(batch: &IngestBatch) -> Vec<(PhysicalDatasetKind, IngestBatch)> {
    match (batch.stream_type, batch.stream.as_str()) {
        (StreamType::Logs, "rum_sessions") => project_rows(
            batch,
            PhysicalDatasetKind::RumSessionSummary,
            RUM_SESSION_FIELDS,
            Some("session_id"),
            true,
        )
        .into_iter()
        .collect(),
        (StreamType::Logs, "rum_errors") => project_rows(
            batch,
            PhysicalDatasetKind::RumErrorSummary,
            RUM_ERROR_FIELDS,
            Some("fingerprint"),
            false,
        )
        .into_iter()
        .collect(),
        (StreamType::Metrics, _) => metric_catalog(batch).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn project_rows(
    batch: &IngestBatch,
    kind: PhysicalDatasetKind,
    fields: &[&str],
    required: Option<&str>,
    use_started_at: bool,
) -> Option<(PhysicalDatasetKind, IngestBatch)> {
    let events = batch
        .events
        .iter()
        .filter(|event| {
            required.is_none_or(|field| {
                event
                    .fields
                    .get(field)
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.is_empty())
            })
        })
        .map(|event| {
            let mut projected = RawEvent {
                timestamp: event.timestamp,
                fields: retain_fields(&event.fields, fields),
            };
            if use_started_at
                && let Some(timestamp) = projected
                    .fields
                    .get("started_at_micros")
                    .and_then(value_i64)
            {
                projected.timestamp = TimestampMicros(timestamp);
            }
            projected
        })
        .collect::<Vec<_>>();
    (!events.is_empty()).then(|| (kind, derived_batch(batch, events)))
}

/// `_molesignal` 与 OTLP 容器 stream 会在同一 stream 中承载多个逻辑指标。
/// 每批每个指标仅写一行目录项，避免目录查询扫描宽样本列。
fn metric_catalog(batch: &IngestBatch) -> Option<(PhysicalDatasetKind, IngestBatch)> {
    let mut entries = BTreeMap::<String, RawEvent>::new();
    for event in &batch.events {
        let Some(name) = event
            .fields
            .get(METRIC_NAME_FIELD)
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        entries.entry(name.to_string()).or_insert_with(|| {
            let mut fields = Map::new();
            fields.insert(METRIC_NAME_FIELD.into(), Value::String(name.to_string()));
            if let Some(kind) = event.fields.get(METRIC_KIND_FIELD) {
                fields.insert(METRIC_KIND_FIELD.into(), kind.clone());
            }
            RawEvent {
                timestamp: event.timestamp,
                fields,
            }
        });
    }
    let events = entries.into_values().collect::<Vec<_>>();
    (!events.is_empty()).then(|| {
        (
            PhysicalDatasetKind::MetricCatalog,
            derived_batch(batch, events),
        )
    })
}

fn derived_batch(source: &IngestBatch, events: Vec<RawEvent>) -> IngestBatch {
    IngestBatch {
        batch_id: Id::new(),
        org_id: source.org_id.clone(),
        stream: source.stream.clone(),
        stream_type: source.stream_type,
        events,
        received_at: source.received_at,
    }
}

fn retain_fields(source: &Map<String, Value>, fields: &[&str]) -> Map<String, Value> {
    fields
        .iter()
        .filter_map(|name| {
            source
                .get(*name)
                .cloned()
                .map(|value| ((*name).to_string(), value))
        })
        .collect()
}

fn value_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn batch(stream: &str, stream_type: StreamType, fields: Value) -> IngestBatch {
        IngestBatch {
            batch_id: Id::from_string("batch"),
            org_id: Id::from_string("org"),
            stream: stream.into(),
            stream_type,
            events: vec![RawEvent {
                timestamp: TimestampMicros(10),
                fields: fields.as_object().unwrap().clone(),
            }],
            received_at: TimestampMicros(20),
        }
    }

    #[test]
    fn rum_session_projection_is_narrow_and_uses_session_start() {
        let source = batch(
            "rum_sessions",
            StreamType::Logs,
            json!({
                "session_id": "s-1",
                "started_at_micros": 123,
                "ip_address": "203.0.113.42",
                "country": "CN",
                "large_payload": {"ignored": true}
            }),
        );
        let projections = project(&source);
        assert_eq!(projections.len(), 1);
        assert_eq!(projections[0].0, PhysicalDatasetKind::RumSessionSummary);
        assert_eq!(projections[0].1.events[0].timestamp, TimestampMicros(123));
        assert_eq!(
            projections[0].1.events[0].fields["ip_address"],
            "203.0.113.42"
        );
        assert!(
            !projections[0].1.events[0]
                .fields
                .contains_key("large_payload")
        );
    }

    #[test]
    fn metric_catalog_deduplicates_names_within_batch() {
        let mut source = batch(
            "_molesignal",
            StreamType::Metrics,
            json!({METRIC_NAME_FIELD: "requests_total", "value": 1}),
        );
        source.events.push(source.events[0].clone());
        let projections = project(&source);
        assert_eq!(projections[0].0, PhysicalDatasetKind::MetricCatalog);
        assert_eq!(projections[0].1.events.len(), 1);
        assert_eq!(
            projections[0].1.events[0].fields[METRIC_NAME_FIELD],
            "requests_total"
        );
    }
}
