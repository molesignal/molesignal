// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! One-pass log routing before schema validation and persistence. No recursive pipelines.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use serde_json::Value;

use super::{IntakeOrigin, IntakeService};
use crate::{
    domain::{
        function::{Function, FunctionExecutor, FunctionLanguage},
        intake::{IntakeBatch, IntakeError, IntakeResult, RawEvent, usage::event_bytes},
        pipeline::{
            processing::ProcessingKind,
            realtime::{RealtimePipeline, RealtimePipelineRepository},
        },
        stream::StreamType,
    },
    shared::{Error, Result, ids::Id},
};

pub struct RealtimeEngine {
    pub repository: Arc<dyn RealtimePipelineRepository>,
    pub executor: Arc<dyn FunctionExecutor>,
}

#[derive(Default)]
struct Output {
    events: Vec<RawEvent>,
    sizes: Vec<u64>,
    indices: Vec<usize>,
}
impl Output {
    fn push(&mut self, event: RawEvent, size: u64, index: usize) {
        self.events.push(event);
        self.sizes.push(size);
        self.indices.push(index);
    }
}

impl IntakeService {
    pub fn with_realtime(mut self, engine: RealtimeEngine) -> Self {
        self.realtime = Some(engine);
        self
    }

    pub(super) async fn route_realtime(
        &self,
        batch: IntakeBatch,
        pipeline: RealtimePipeline,
        engine: &RealtimeEngine,
    ) -> Result<IntakeResult> {
        if pipeline.org_id != batch.org_id {
            return Err(Error::forbidden("pipeline organization mismatch"));
        }
        let total = batch.events.len();
        let functions: Vec<_> = pipeline
            .steps
            .iter()
            .enumerate()
            .map(|(index, step)| Function {
                id: Id(format!("{}:{index}", pipeline.id)),
                org_id: batch.org_id.clone(),
                name: step.transform_name.clone(),
                language: FunctionLanguage::Vrl,
                source: step.script.clone(),
                params_schema: Value::Null,
                created_at: batch.received_at,
                updated_at: pipeline.updated_at,
            })
            .collect();
        let IntakeBatch {
            batch_id,
            org_id,
            stream,
            stream_type,
            events,
            received_at,
        } = batch;
        let mut outputs: BTreeMap<String, Output> = BTreeMap::new();
        let mut errors = Vec::new();
        for (index, mut event) in events.into_iter().enumerate() {
            let size = event_bytes(&event)?;
            let original = pipeline.routing.retain_source.then(|| event.clone());
            let mut value = Value::Object(std::mem::take(&mut event.fields));
            let mut failed = false;
            let mut destination = pipeline.target.clone();
            let mut routed = false;
            for (step, function) in pipeline.steps.iter().zip(&functions) {
                if step.kind == ProcessingKind::Builtin {
                    destination = step.routing.destination(&value, &step.target);
                    routed = true;
                    continue;
                }
                if engine.executor.run(function, &mut value).await.is_err() {
                    errors.push(IntakeError {
                        index,
                        reason: format!("realtime transform `{}` failed", function.name),
                    });
                    failed = true;
                    break;
                }
            }
            if failed {
                continue;
            }
            // Preserve earlier configurations until they are saved as a built-in step.
            if !routed {
                destination = pipeline.routing.destination(&value, &pipeline.target);
            }
            let Value::Object(fields) = value else {
                errors.push(IntakeError {
                    index,
                    reason: "realtime transform must produce an object".into(),
                });
                continue;
            };
            event.fields = fields;
            outputs
                .entry(destination.clone())
                .or_default()
                .push(event, size, index);
            // When destination equals source, keep just the processed event.
            if destination != stream
                && let Some(original) = original
            {
                outputs
                    .entry(stream.clone())
                    .or_default()
                    .push(original, size, index);
            }
        }
        let dataset_type = self.sink.primary_dataset_type(StreamType::LOGS)?;
        for (destination, output) in outputs {
            let result = self
                .intake_prepared(
                    IntakeBatch {
                        batch_id: batch_id.clone(),
                        org_id: org_id.clone(),
                        stream: destination,
                        stream_type,
                        events: output.events,
                        received_at,
                    },
                    IntakeOrigin::RoutedExternal,
                    dataset_type.clone(),
                    Some(output.sizes),
                )
                .await?;
            // An input is accepted once, even when both source and destination were written.
            if result.rejected > result.errors.len() {
                return Err(Error::internal(
                    "routed sink rejected rows without error details",
                ));
            }
            for error in result.errors {
                errors.push(IntakeError {
                    index: output.indices[error.index],
                    reason: error.reason,
                });
            }
        }
        let rejected = errors
            .iter()
            .map(|e| e.index)
            .collect::<BTreeSet<_>>()
            .len();
        Ok(IntakeResult {
            accepted: total - rejected,
            rejected,
            errors,
        })
    }
}

#[cfg(test)]
mod tests;
