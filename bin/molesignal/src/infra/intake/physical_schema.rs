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
        storage::{DatasetTypeId, type_id::builtin},
        stream::StreamDefinition,
    },
    shared::trace::summary::{
        TRACE_SUMMARY_DURATION_NS_FIELD, TRACE_SUMMARY_ERROR_COUNT_FIELD,
        TRACE_SUMMARY_MARKER_FIELD, TRACE_SUMMARY_SPAN_COUNT_FIELD, TRACE_SUMMARY_START_NS_FIELD,
    },
};

/// A trace summary is the root Span's own row narrowed to these fields: its
/// identity, what it was, how it ended and the HTTP status it answered with,
/// with the trace's counts beside them.
const TRACE_SUMMARY_FIELDS: &[&str] = &[
    "trace_id",
    "span_id",
    "parent_span_id",
    "service.name",
    "name",
    "status_code",
    "http.response.status_code",
    "http.status_code",
    TRACE_SUMMARY_MARKER_FIELD,
    TRACE_SUMMARY_START_NS_FIELD,
    TRACE_SUMMARY_DURATION_NS_FIELD,
    TRACE_SUMMARY_SPAN_COUNT_FIELD,
    TRACE_SUMMARY_ERROR_COUNT_FIELD,
];

pub(crate) fn project(stream: &StreamDefinition, dataset_type: &DatasetTypeId) -> StreamDefinition {
    let fields = match dataset_type.as_str() {
        builtin::DATASET_TRACE_SUMMARY => Some(TRACE_SUMMARY_FIELDS),
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
            index_type: None,
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
            stream_type: StreamType::TRACES,
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
        let projected = project(
            &stream(),
            &DatasetTypeId::builtin(builtin::DATASET_TRACE_SUMMARY),
        );
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
        let projected = project(
            &original,
            &DatasetTypeId::builtin(builtin::DATASET_TRACE_SPANS),
        );
        assert_eq!(projected.schema.fields.len(), original.schema.fields.len());
    }
}
