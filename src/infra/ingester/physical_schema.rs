// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 逻辑 stream schema 到内部物理数据集 schema 的投影。
//!
//! 派生摘要与原始事件共享逻辑 stream、保留策略和权限，但不能共享宽表物理 schema；
//! 否则一条 Trace 摘要仍会为所有 Span 字段写出空列，独立数据集只隔离了路径而没有
//! 降低扫描与 footer 成本。

use std::collections::HashSet;

use crate::{
    domain::{
        ingestion::EVENT_ID_FIELD,
        metrics::{METRIC_KIND_FIELD, METRIC_NAME_FIELD},
        storage::PhysicalDatasetKind,
        stream::StreamDefinition,
    },
    shared::trace::summary::{
        TRACE_SUMMARY_DURATION_NS_FIELD, TRACE_SUMMARY_ERROR_COUNT_FIELD,
        TRACE_SUMMARY_MARKER_FIELD, TRACE_SUMMARY_SPAN_COUNT_FIELD, TRACE_SUMMARY_START_NS_FIELD,
    },
};

const TRACE_SUMMARY_FIELDS: &[&str] = &[
    "trace_id",
    "span_id",
    "parent_span_id",
    "service.name",
    "name",
    "status_code",
    TRACE_SUMMARY_MARKER_FIELD,
    TRACE_SUMMARY_START_NS_FIELD,
    TRACE_SUMMARY_DURATION_NS_FIELD,
    TRACE_SUMMARY_SPAN_COUNT_FIELD,
    TRACE_SUMMARY_ERROR_COUNT_FIELD,
];

const RUM_SESSION_SUMMARY_FIELDS: &[&str] = &[
    EVENT_ID_FIELD,
    "session_id",
    "user_id",
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

const RUM_ERROR_SUMMARY_FIELDS: &[&str] = &[
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

const METRIC_CATALOG_FIELDS: &[&str] = &[METRIC_NAME_FIELD, METRIC_KIND_FIELD];

pub(crate) fn project(
    stream: &StreamDefinition,
    dataset_kind: PhysicalDatasetKind,
) -> StreamDefinition {
    let fields = match dataset_kind {
        PhysicalDatasetKind::TraceSummary => Some(TRACE_SUMMARY_FIELDS),
        PhysicalDatasetKind::RumSessionSummary => Some(RUM_SESSION_SUMMARY_FIELDS),
        PhysicalDatasetKind::RumErrorSummary => Some(RUM_ERROR_SUMMARY_FIELDS),
        PhysicalDatasetKind::MetricCatalog => Some(METRIC_CATALOG_FIELDS),
        _ => None,
    };
    let Some(fields) = fields else {
        return stream.clone();
    };
    let fields = fields.iter().copied().collect::<HashSet<_>>();
    let mut projected = stream.clone();
    projected
        .schema
        .fields
        .retain(|field| fields.contains(field.name.as_str()));
    projected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::stream::{FieldDef, FieldType, Schema, StreamType},
        shared::{ids::Id, time::TimestampMicros},
    };

    fn field(name: &str) -> FieldDef {
        FieldDef {
            name: name.to_string(),
            data_type: FieldType::Utf8,
            nullable: true,
            indexed: false,
            encrypted: false,
            exact: false,
        }
    }

    fn stream() -> StreamDefinition {
        StreamDefinition {
            id: Id::new(),
            org_id: Id::new(),
            name: "traces".to_string(),
            stream_type: StreamType::Traces,
            schema: Schema {
                fields: vec![
                    field("trace_id"),
                    field(TRACE_SUMMARY_START_NS_FIELD),
                    field("events"),
                    field("attributes"),
                ],
            },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn trace_summary_drops_wide_span_columns() {
        let projected = project(&stream(), PhysicalDatasetKind::TraceSummary);
        let names = projected
            .schema
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["trace_id", TRACE_SUMMARY_START_NS_FIELD]);
    }

    #[test]
    fn raw_keeps_authoritative_schema() {
        let original = stream();
        let projected = project(&original, PhysicalDatasetKind::Raw);
        assert_eq!(projected.schema.fields.len(), original.schema.fields.len());
    }

    #[test]
    fn rum_and_metric_read_models_drop_wide_payload_fields() {
        let mut original = stream();
        original.schema.fields.extend([
            field(EVENT_ID_FIELD),
            field("session_id"),
            field("fingerprint"),
            field(METRIC_NAME_FIELD),
            field("large_payload"),
        ]);

        let sessions = project(&original, PhysicalDatasetKind::RumSessionSummary);
        assert!(
            sessions
                .schema
                .fields
                .iter()
                .any(|field| field.name == "session_id")
        );
        assert!(
            !sessions
                .schema
                .fields
                .iter()
                .any(|field| field.name == "large_payload")
        );

        let errors = project(&original, PhysicalDatasetKind::RumErrorSummary);
        assert!(
            errors
                .schema
                .fields
                .iter()
                .any(|field| field.name == "fingerprint")
        );
        assert!(
            !errors
                .schema
                .fields
                .iter()
                .any(|field| field.name == "large_payload")
        );

        let catalog = project(&original, PhysicalDatasetKind::MetricCatalog);
        assert_eq!(
            catalog
                .schema
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            vec![METRIC_NAME_FIELD]
        );
    }
}
