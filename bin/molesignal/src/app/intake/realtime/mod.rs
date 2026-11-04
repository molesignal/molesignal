// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! One pass through a stream's realtime pipeline before schema validation and persistence. No
//! recursive pipelines.
//!
//! - Logs, metrics and profile metadata are changed here, as a batch arrives; logs may be routed to
//!   other streams.
//! - Spans are changed where they are written ([`spans`]).
//! - Metrics, spans, profile rows and RUM rows keep the fields
//!   [`crate::domain::pipeline::protected`] names: a step that changes one fails the event, and so
//!   does a step that drops a RUM view or vital.
//! - Steps that keep count change no field: a metric step adds the event to its metric
//!   ([`generated`]); a quota or a throttle ([`limits`]) lets it on, or drops it, or sends it to
//!   another stream as it is, past its limit. What they hold between events is in [`StepState`].
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use serde_json::Value;

use super::{IntakeOrigin, IntakeService};
use crate::{
    domain::{
        function::{Function, FunctionExecutor, FunctionLanguage, FunctionOutcome},
        intake::{IntakeBatch, IntakeError, IntakeResult, RawEvent, usage::event_bytes},
        pipeline::{
            limit::OverAction,
            processing::{ProcessingKind, ProcessingStep},
            protected::{Guard, changed_protected, protected_of},
            realtime::{RealtimePipeline, RealtimePipelineRepository},
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub mod generated;
pub mod limits;
pub mod spans;
mod state;

use generated::Counted;
pub use state::StepState;

pub struct RealtimeEngine {
    pub repository: Arc<dyn RealtimePipelineRepository>,
    pub executor: Arc<dyn FunctionExecutor>,
    pub steps: Arc<StepState>,
}

/// What the steps that keep count know of an event besides its fields.
#[derive(Clone, Copy)]
struct Arrival {
    /// The event's own time, which a metric counts it by.
    at: TimestampMicros,
    /// Its size as it arrived, which a quota in bytes counts.
    bytes: u64,
    /// When its batch is processed, which quotas and throttles count by.
    now: TimestampMicros,
}

/// Where one event's way through a pipeline's steps ended.
enum Passage {
    /// The event as the steps left it.
    Kept(Value),
    /// A step dropped it on purpose.
    Dropped,
    /// Why a step failed on it.
    Failed(String),
}

impl RealtimeEngine {
    /// One event through the pipeline's steps, and the stream a routing step sent it to, if one did.
    /// `guard` says what of the event the steps may not change, and whether they may drop it.
    ///
    /// What a metric step counts of the event goes into `counted`, for the caller to add once the
    /// event is written.
    async fn pass<'p>(
        &self,
        pipeline: &'p RealtimePipeline,
        functions: &[Function],
        guard: Option<Guard>,
        value: Value,
        arrival: Arrival,
        counted: &mut Vec<Counted<'p>>,
    ) -> (Passage, Option<String>) {
        let kept = guard.and_then(|guard| {
            value
                .as_object()
                .map(|fields| (guard, protected_of(guard, fields)))
        });

        let mut value = value;
        let mut routed = None;

        for (step, function) in pipeline.steps.iter().zip(functions) {
            match step.kind {
                ProcessingKind::Vrl => {}
                ProcessingKind::Builtin => {
                    routed = Some(step.routing.destination(&value, &step.target));
                    continue;
                }
                ProcessingKind::GenerateMetric => {
                    counted.extend(count(pipeline, step, &value, arrival));
                    continue;
                }
                ProcessingKind::Quota | ProcessingKind::Throttle => {
                    if self.within_limit(pipeline, step, &value, arrival).await {
                        continue;
                    }

                    match over_action(step) {
                        OverAction::Pass => continue,
                        // It leaves the pipeline here, as it is, for the stream past the limit.
                        OverAction::Route => {
                            let target = step.over_limit_target().map(str::to_owned);
                            return (Passage::Kept(value), target.or(routed));
                        }
                        OverAction::Drop if guard.is_some_and(|guard| !guard.drops()) => {
                            let reason = format!(
                                "step `{}` held back a row of {}, which keeps every row",
                                step.transform_name,
                                guard.map_or("", Guard::name)
                            );

                            return (Passage::Failed(reason), routed);
                        }
                        OverAction::Drop => return (Passage::Dropped, routed),
                    }
                }
            }

            match self.executor.run(function, &mut value).await {
                Ok(FunctionOutcome::Kept) => {}
                Ok(FunctionOutcome::Dropped) if guard.is_some_and(|guard| !guard.drops()) => {
                    let reason = format!(
                        "realtime transform `{}` dropped a row of {}, which keeps every row",
                        function.name,
                        guard.map_or("", Guard::name)
                    );

                    return (Passage::Failed(reason), routed);
                }
                Ok(FunctionOutcome::Dropped) => return (Passage::Dropped, routed),
                Err(_) => {
                    let reason = format!("realtime transform `{}` failed", function.name);
                    return (Passage::Failed(reason), routed);
                }
            }

            if let Some((guard, before)) = &kept
                && let Some(field) = changed_protected(*guard, before, &value)
            {
                let reason = format!(
                    "realtime transform `{}` changed `{field}`, which a pipeline on {} keeps as it arrives",
                    function.name,
                    guard.name()
                );

                return (Passage::Failed(reason), routed);
            }
        }

        (Passage::Kept(value), routed)
    }

    /// Whether a quota or a throttle lets the event on: one its condition does not pick always goes.
    async fn within_limit(
        &self,
        pipeline: &RealtimePipeline,
        step: &ProcessingStep,
        value: &Value,
        arrival: Arrival,
    ) -> bool {
        if !step.condition.matches(value) {
            return true;
        }

        let (org, id, name) = (&pipeline.org_id, &pipeline.id, step.transform_name.trim());
        let limits = &self.steps.limits;

        match (step.kind, &step.quota, &step.throttle) {
            (ProcessingKind::Quota, Some(spec), _) => {
                limits
                    .quota(org, id, name, spec, value, arrival.bytes, arrival.now)
                    .await
            }
            (ProcessingKind::Throttle, _, Some(spec)) => {
                limits.throttle(org, id, name, spec, value, arrival.bytes, arrival.now)
            }
            _ => true,
        }
    }
}

/// What an event the step's condition picks adds to the step's metric.
fn count<'p>(
    pipeline: &'p RealtimePipeline,
    step: &'p ProcessingStep,
    value: &Value,
    arrival: Arrival,
) -> Option<Counted<'p>> {
    let spec = step.metric.as_ref()?;

    if !step.condition.matches(value) {
        return None;
    }

    Counted::of(
        &pipeline.org_id,
        &pipeline.id,
        step.transform_name.trim(),
        spec,
        value,
        arrival.at,
        arrival.now,
    )
}

/// What a quota or a throttle does with what goes past its limit.
fn over_action(step: &ProcessingStep) -> OverAction {
    match step.kind {
        ProcessingKind::Quota => step.quota.as_ref().map(|spec| spec.action),
        ProcessingKind::Throttle => step.throttle.as_ref().map(|spec| spec.action),
        _ => None,
    }
    .unwrap_or_default()
}

/// The pipeline's steps as functions the executor runs.
fn functions(pipeline: &RealtimePipeline, org: &Id, at: TimestampMicros) -> Vec<Function> {
    pipeline
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| Function {
            id: Id(format!("{}:{index}", pipeline.id)),
            org_id: org.clone(),
            name: step.transform_name.clone(),
            language: FunctionLanguage::Vrl,
            source: step.script.clone(),
            params_schema: Value::Null,
            created_at: at,
            updated_at: pipeline.updated_at,
        })
        .collect()
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
        let functions = functions(&pipeline, &batch.org_id, batch.received_at);

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
        let guard = Guard::of(pipeline.signal, &stream);
        let now = TimestampMicros::now();
        let mut counted = Vec::new();

        for (index, mut event) in events.into_iter().enumerate() {
            let size = event_bytes(&event)?;
            let original = pipeline.routing.retain_source.then(|| event.clone());
            let value = Value::Object(std::mem::take(&mut event.fields));

            let arrival = Arrival {
                at: event.timestamp,
                bytes: size,
                now,
            };

            let (passage, routed) = engine
                .pass(&pipeline, &functions, guard, value, arrival, &mut counted)
                .await;
            let mut destination = routed.clone().unwrap_or_else(|| pipeline.target.clone());

            let value = match passage {
                Passage::Kept(value) => value,
                Passage::Failed(reason) => {
                    errors.push(IntakeError { index, reason });
                    continue;
                }
                // A step dropped the event on purpose: nothing is routed. A retained copy of the
                // source still keeps the event as it arrived, since that is what retaining the
                // source means.
                Passage::Dropped => {
                    if let Some(original) = original
                        && destination != stream
                    {
                        outputs
                            .entry(stream.clone())
                            .or_default()
                            .push(original, size, index);
                    }

                    continue;
                }
            };

            // Preserve earlier configurations until they are saved as a built-in step.
            if routed.is_none() {
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

        let dataset_type = self.sink.primary_dataset_type(stream_type)?;

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

        // Every output is written: what the batch's events add to metrics counts now.
        engine.steps.metrics.record(counted);

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
