// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 链路的实时流水线在写 span 的地方跑：尾采样之后的写入端（[`crate::app::trace`]），以及 JSON 的
//! 链路写入接口（[`IntakeService::intake_spans`]）。
//!
//! - 尾采样之后再处理，trace 的摘要就能按处理后的 span 算：丢掉的 span 不计数，整条 trace 都丢了
//!   就没有摘要。
//! - [`IntakeService::intake`] 不再处理链路，转发到写入节点的批次也就不会被处理第二遍。
//! - 一个 span 的属性既平铺在行上，又在 `attributes`、`resource.attributes` 两个 JSON 里各存一份。
//!   步骤改的是平铺的那份，这里把 JSON 里的跟上（[`follow_attributes`]），删掉的敏感属性在 JSON
//!   里也就没有了。

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::{Arrival, Passage, functions};
use crate::{
    app::intake::IntakeService,
    domain::{
        intake::{IntakeBatch, IntakeError, IntakeResult, RawEvent},
        pipeline::protected::Guard,
        stream::StreamType,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// span 行上不是属性的列。步骤新添的字段不在其中时算作 span 的属性。
const SPAN_COLUMNS: &[&str] = &[
    "trace_id",
    "span_id",
    "parent_span_id",
    "trace_flags",
    "trace_state",
    "name",
    "kind",
    "start_time_unix_nano",
    "end_time_unix_nano",
    "duration_ns",
    "status_code",
    "status_message",
    "service.name",
    "resource",
    "scope",
    "attributes",
    "events",
    "links",
    "dropped_attributes_count",
    "dropped_events_count",
    "dropped_links_count",
    "schema_version",
    "semantic_conventions_version",
    "sampling_reason",
    "partial",
    "partial_reasons",
    "late",
    "duplicate",
    "conflict",
];

/// 一个流的 span 经过它的实时流水线之后。
#[derive(Debug, Default)]
pub struct ProcessedSpans {
    /// 留下的 span，按进来的顺序。
    pub kept: Vec<RawEvent>,
    /// 留下的 span 在进来的那批里的位置。
    pub places: Vec<usize>,
    /// 处理失败的 span：位置和原因。
    pub failed: Vec<IntakeError>,
    /// 步骤有意丢掉的 span 数。
    pub dropped: usize,
}

impl IntakeService {
    /// 组织的流 `stream` 上的 span 经过它的实时流水线；流上没挂流水线时原样留下。
    pub(crate) async fn process_spans(
        &self,
        org: &Id,
        stream: &str,
        spans: Vec<RawEvent>,
    ) -> Result<ProcessedSpans> {
        let pipeline = match &self.realtime {
            Some(engine) => engine
                .repository
                .for_source(org, StreamType::TRACES, stream)
                .await?
                .map(|pipeline| (engine, pipeline)),
            None => None,
        };

        let Some((engine, pipeline)) = pipeline else {
            return Ok(ProcessedSpans {
                places: (0..spans.len()).collect(),
                kept: spans,
                ..ProcessedSpans::default()
            });
        };

        let now = TimestampMicros::now();
        let functions = functions(&pipeline, org, now);
        let mut processed = ProcessedSpans::default();
        let mut counted = Vec::new();

        for (index, mut span) in spans.into_iter().enumerate() {
            let before = std::mem::take(&mut span.fields);
            let value = Value::Object(before.clone());

            // A pipeline on spans has no quota or throttle (`validate_config`): no size is needed.
            let arrival = Arrival {
                at: span.timestamp,
                bytes: 0,
                now,
            };

            match engine
                .pass(
                    &pipeline,
                    &functions,
                    Some(Guard::Spans),
                    value,
                    arrival,
                    &mut counted,
                )
                .await
                .0
            {
                Passage::Kept(Value::Object(mut fields)) => {
                    follow_attributes(&before, &mut fields);
                    span.fields = fields;
                    processed.kept.push(span);
                    processed.places.push(index);
                }
                Passage::Kept(_) => processed.failed.push(IntakeError {
                    index,
                    reason: "realtime transform must produce an object".into(),
                }),
                Passage::Dropped => processed.dropped += 1,
                Passage::Failed(reason) => processed.failed.push(IntakeError { index, reason }),
            }
        }

        // The spans are written after this, by a sink that does not send them through again.
        engine.steps.metrics.record(counted);
        Ok(processed)
    }

    /// JSON 的链路写入：先过流的实时流水线，再照常写入。失败的 span 按它在请求里的位置报出来。
    pub async fn intake_spans(&self, batch: IntakeBatch) -> Result<IntakeResult> {
        let total = batch.events.len();

        let IntakeBatch {
            batch_id,
            org_id,
            stream,
            stream_type,
            events,
            received_at,
        } = batch;

        let processed = self.process_spans(&org_id, &stream, events).await?;
        let mut errors = processed.failed;

        if !processed.kept.is_empty() {
            let result = self
                .intake(IntakeBatch {
                    batch_id,
                    org_id,
                    stream,
                    stream_type,
                    events: processed.kept,
                    received_at,
                })
                .await?;

            errors.extend(result.errors.into_iter().map(|error| IntakeError {
                index: processed.places[error.index],
                reason: error.reason,
            }));
        }

        let rejected = errors
            .iter()
            .map(|error| error.index)
            .collect::<BTreeSet<_>>()
            .len();

        Ok(IntakeResult {
            accepted: total - rejected,
            rejected,
            errors,
        })
    }
}

/// 步骤改了、删了平铺的属性，`attributes` 和 `resource.attributes` 里的同名项跟着改、跟着删；步骤
/// 新添的属性写进 `attributes`。步骤直接改 JSON 里的项照它改的算。
fn follow_attributes(before: &Map<String, Value>, after: &mut Map<String, Value>) {
    let changed: Vec<(String, Option<Value>)> = before
        .iter()
        .filter(|(name, value)| after.get(name.as_str()) != Some(value))
        .map(|(name, _)| (name.clone(), after.get(name.as_str()).cloned()))
        .collect();

    let added: Vec<(String, Value)> = after
        .iter()
        .filter(|(name, _)| !before.contains_key(name.as_str()))
        .filter(|(name, _)| !SPAN_COLUMNS.contains(&name.as_str()))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();

    for (name, value) in &changed {
        for attributes in attribute_maps(after) {
            if !attributes.contains_key(name) {
                continue;
            }

            match value {
                Some(value) => attributes.insert(name.clone(), value.clone()),
                None => attributes.remove(name),
            };
        }
    }

    if let Some(attributes) = after.get_mut("attributes").and_then(Value::as_object_mut) {
        for (name, value) in added {
            attributes.insert(name, value);
        }
    }
}

/// span 行上存属性的两个 JSON：`attributes` 和 `resource.attributes`。
fn attribute_maps(fields: &mut Map<String, Value>) -> Vec<&mut Map<String, Value>> {
    let mut maps = Vec::new();
    let mut resource = None;
    let mut span = None;

    for (name, value) in fields.iter_mut() {
        match name.as_str() {
            "attributes" => span = value.as_object_mut(),
            "resource" => {
                resource = value.get_mut("attributes").and_then(Value::as_object_mut);
            }
            _ => {}
        }
    }

    maps.extend(span);
    maps.extend(resource);
    maps
}

#[cfg(test)]
mod tests;
