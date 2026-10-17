// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Writing a run's events to its streams through the standard intake: every event to the stream
//! it was routed to (the pipeline's first stream unless a routing step chose another) and to every
//! further stream of the pipeline.
//!
//! 目标流不存在时按本批事件建流，输出里的新字段照常演化目标流的 schema（见
//! `IntakeService::intake_pipeline_output`）。与已有字段类型冲突的事件被拒收：只丢这些事件，
//! 它们算失败、进死信，其余照常写入。

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{
    DeadLetter, Ledger, PipelineOutput, PipelineSpec, WriteFailure, is_configuration_error,
    summaries,
};
use crate::{
    domain::intake::{IntakeBatch, RawEvent},
    infra::pipeline::exec::processing::ProcessedEvent,
    shared::{ids::Id, time::TimestampMicros},
};

/// Writes `rows` to their streams. A stream that already took a source's rows in an earlier attempt
/// is left out for that source. Returns what each stream that took none of its events said.
pub(super) async fn write(
    output: &PipelineOutput,
    pipeline: &PipelineSpec<'_>,
    rows: &[ProcessedEvent],
    ledger: &mut Ledger,
) -> Vec<String> {
    let copies = &pipeline.config.sinks[1..];
    let mut batches: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    let mut streams: BTreeSet<&str> = pipeline.config.sinks.iter().map(String::as_str).collect();

    for (index, row) in rows.iter().enumerate() {
        streams.insert(row.destination.as_str());
        let done = &ledger.sources[row.source].done;
        let mut to: Vec<&str> = vec![row.destination.as_str()];

        for copy in copies {
            if !to.contains(&copy.as_str()) {
                to.push(copy.as_str());
            }
        }

        for stream in to {
            if !done.contains(stream) {
                batches.entry(stream).or_default().push(index);
            }
        }
    }

    let now = TimestampMicros::now();
    let mut broken = Vec::new();
    let mut failed_writes: BTreeSet<(usize, &str)> = BTreeSet::new();

    for (stream, indices) in batches {
        let events: Vec<RawEvent> = indices
            .iter()
            .map(|&index| to_raw_event(&rows[index].event, now))
            .collect();

        let batch = IntakeBatch {
            batch_id: Id::new(),
            org_id: pipeline.org_id.clone(),
            stream: stream.to_owned(),
            stream_type: pipeline.stream_type,
            events,
            received_at: now,
        };

        let result = match output.intake.intake_pipeline_output(batch).await {
            Ok(result) => result,
            Err(e) => {
                ledger
                    .errors
                    .push(format!("writing to stream `{stream}` failed: {e}"));

                let failure = WriteFailure {
                    message: format!("write to `{stream}`: {e}"),
                    permanent: is_configuration_error(&e),
                };

                for &index in &indices {
                    let source = rows[index].source;
                    ledger.failed[index] = true;
                    failed_writes.insert((source, stream));
                    ledger.sources[source]
                        .failure
                        .get_or_insert_with(|| failure.clone());
                }

                continue;
            }
        };

        for error in &result.errors {
            let Some(&index) = indices.get(error.index) else {
                continue;
            };

            let row = &rows[index];
            let source = ledger.names[row.source].clone();
            ledger.failed[index] = true;
            ledger.dead_letter(|| {
                DeadLetter::rejected(source, row.event.clone(), stream, &error.reason)
            });
        }

        let lines = summaries(
            result.errors.iter().map(|error| error.reason.as_str()),
            |count, reason| format!("stream `{stream}` rejected {count} event(s): {reason}"),
        );

        if result.accepted == 0 && result.rejected > 0 {
            broken.extend(lines.iter().cloned());
        }

        ledger.errors.extend(lines);
    }

    // A stream took a source's rows unless writing them there failed; a stream none of them were
    // meant for has nothing to take.
    for (index, source) in ledger.sources.iter_mut().enumerate() {
        for stream in &streams {
            if !failed_writes.contains(&(index, *stream)) {
                source.done.insert((*stream).to_owned());
            }
        }
    }

    broken
}

/// 一行变换后的 JSON 对象 → `RawEvent`：从常见时间字段取微秒时间戳，缺失则用 `fallback`。
///
/// 两处与 HTTP 写入对齐，因为目标流的 schema 由这些字段推断：
/// - `_timestamp` 是隐式时间列，取出后不再留作数据字段，否则会被当成新字段写进 schema，和隐式列同名；
/// - 值为 null 的字段丢掉。源流 `SELECT *` 会给事件没有的列补 null，这些 null 不携带信息，却会让
///   首次建流时把整列推断成 JSON，也会让非空字段的缺失检查失效。
pub(super) fn to_raw_event(value: &Value, fallback: TimestampMicros) -> RawEvent {
    let mut fields = match value {
        Value::Object(map) => map.clone(),
        other => {
            let mut m = serde_json::Map::new();
            m.insert("value".to_string(), other.clone());
            m
        }
    };

    let system_timestamp = fields.remove("_timestamp");
    fields.retain(|_, value| !value.is_null());

    let timestamp = system_timestamp
        .as_ref()
        .or_else(|| fields.get("timestamp"))
        .and_then(Value::as_i64)
        .map(TimestampMicros)
        .unwrap_or(fallback);

    RawEvent { timestamp, fields }
}
