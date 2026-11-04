// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 不知道一条 Trace 在什么时候时找到它：每条 Trace 写下时有一行摘要，记着它的起止。摘要行比 Span
//! 少得多，`trace_id` 又有整值索引，在最长的查找窗口里找它的摘要行便宜；找到了就只读它起止之间的
//! Span。

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        query::{QueryLanguage, QueryRequest, StreamHint},
        storage::{DatasetTypeId, type_id::builtin},
        stream::StreamType,
    },
    infra::query::escape_sql_ident,
    shared::{
        Result,
        time::{TimeRange, TimestampMicros},
        trace::summary::{TRACE_SUMMARY_DURATION_NS_FIELD, TRACE_SUMMARY_START_NS_FIELD},
    },
};

/// 读出的起止两边各放宽这么多，单位微秒：纳秒折成微秒时的取整。
const SLACK_MICROS: i64 = 1_000_000;

/// `trace_id` 的 Span 开始的时间范围，从 `range` 里它的摘要行读出；没有摘要行时为 `None`。
pub(super) async fn trace_range(
    state: &AppState,
    ctx: &IamContext,
    stream: &str,
    trace_id: &str,
    range: TimeRange,
) -> Result<Option<TimeRange>> {
    let sql = format!(
        "SELECT \"{start}\" AS start_ns, \"{duration}\" AS duration_ns
         FROM \"{stream}\"
         WHERE trace_id = '{trace_id}'
         LIMIT 1",
        start = TRACE_SUMMARY_START_NS_FIELD,
        duration = TRACE_SUMMARY_DURATION_NS_FIELD,
        stream = escape_sql_ident(stream),
    );

    let req = QueryRequest {
        org_id: ctx.org_id.clone(),
        language: QueryLanguage::Sql,
        statement: sql,
        time_range: range,
        stream: Some(StreamHint {
            name: stream.to_owned(),
            stream_type: StreamType::TRACES,
        }),
        limit: Some(1),
        step: None,
        federation_clusters: Vec::new(),
    };

    let out = state
        .query
        .run_dataset(req, DatasetTypeId::builtin(builtin::DATASET_TRACE_SUMMARY))
        .await?;

    let Some(row) = out.rows.first() else {
        return Ok(None);
    };

    let nanos = |index: usize| row.get(index).and_then(serde_json::Value::as_i64);

    let Some(start_ns) = nanos(0) else {
        return Ok(None);
    };

    let end_ns = start_ns.saturating_add(nanos(1).unwrap_or(0).max(0));

    Ok(Some(TimeRange::new(
        TimestampMicros(start_ns / 1_000 - SLACK_MICROS),
        TimestampMicros(end_ns / 1_000 + SLACK_MICROS),
    )))
}
