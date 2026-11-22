// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The sink that writes kept traces into this deployment's own storage.
//!
//! Its worker retries a failed batch until it is written, so a write that fails halfway must
//! leave behind only what is still unwritten: the traces of every (org, stream) group written in
//! full leave the batch, and a group whose spans were written but whose summary rows were not
//! keeps just those rows, which go first on the next attempt.

use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

use async_trait::async_trait;
use domain::{
    intake::{IntakeBatch, RawEvent},
    storage::{DatasetTypeId, type_id::builtin},
    stream::{MOLESIGNAL_SYSTEM_STREAM, StreamType},
};
use kernel::{ids::Id, time::TimestampMicros};
use parking_lot::Mutex;
use signals::{self_telemetry::with_suppression, tail_sampling::DecidedTrace};

use super::{TraceSink, processing, summary};
use crate::app::intake::IntakeService;

/// Writes the traces that were kept into this deployment's own storage: tenants' always, the
/// process's own only with self telemetry on.
pub struct SelfIntakeTraceSink {
    intake: Arc<IntakeService>,
    system_org_id: Id,
    stores_own_traces: bool,
    /// Summary rows of groups whose spans are already written.
    unwritten_summaries: Mutex<Vec<Rows>>,
}

/// Rows bound for one stream.
struct Rows {
    org_id: String,
    stream: String,
    internal: bool,
    events: Vec<RawEvent>,
}

/// One stream's share of a batch: the traces it came from, their spans and summary rows.
#[derive(Default)]
struct Group {
    traces: Vec<usize>,
    spans: Vec<RawEvent>,
    summaries: Vec<RawEvent>,
}

impl SelfIntakeTraceSink {
    pub fn new(intake: Arc<IntakeService>, system_org_id: Id, stores_own_traces: bool) -> Self {
        Self {
            intake,
            system_org_id,
            stores_own_traces,
            unwritten_summaries: Mutex::new(Vec::new()),
        }
    }

    async fn groups(
        &self,
        traces: &[DecidedTrace],
    ) -> Result<BTreeMap<(String, String, bool), Group>, String> {
        let mut groups: BTreeMap<(String, String, bool), Group> = BTreeMap::new();

        for (index, trace) in traces.iter().enumerate() {
            let (org_id, stream, internal) = match &trace.stream {
                Some(stream) => (trace.org_id.clone(), stream.clone(), false),
                None => (
                    self.system_org_id.0.clone(),
                    MOLESIGNAL_SYSTEM_STREAM.into(),
                    true,
                ),
            };

            let spans = summary::span_events(trace);

            let spans = if internal {
                spans
            } else {
                processing::written_spans(&self.intake, &org_id, &stream, spans).await?
            };

            let group = groups.entry((org_id, stream, internal)).or_default();

            if summary::summarized(trace) {
                group.summaries.extend(summary::summary_event(&spans));
            }

            group.traces.push(index);
            group.spans.extend(spans);
        }

        Ok(groups)
    }

    /// Writes summary rows left over from an earlier attempt; what fails stays for the next one.
    async fn write_unwritten_summaries(&self) -> Result<(), String> {
        let pending = std::mem::take(&mut *self.unwritten_summaries.lock());
        let mut pending = pending.into_iter();

        while let Some(rows) = pending.next() {
            if let Err(error) = self.write_summaries(&rows).await {
                let mut unwritten = self.unwritten_summaries.lock();
                unwritten.push(rows);
                unwritten.extend(pending);
                return Err(error);
            }
        }

        Ok(())
    }

    async fn write_spans(&self, rows: Rows) -> Result<(), String> {
        let batch = IntakeBatch {
            batch_id: Id::new(),
            org_id: Id(rows.org_id),
            stream: rows.stream,
            stream_type: StreamType::TRACES,
            events: rows.events,
            received_at: TimestampMicros::now(),
        };

        let result = if rows.internal {
            with_suppression(self.intake.intake_self_telemetry(batch)).await
        } else {
            with_suppression(self.intake.intake(batch)).await
        }
        .map_err(|error| error.to_string())?;

        if result.rejected != 0 {
            return Err(format!("Trace storage rejected {} spans", result.rejected));
        }

        Ok(())
    }

    async fn write_summaries(&self, rows: &Rows) -> Result<(), String> {
        let batch = IntakeBatch {
            batch_id: Id::new(),
            org_id: Id(rows.org_id.clone()),
            stream: rows.stream.clone(),
            stream_type: StreamType::TRACES,
            events: rows.events.clone(),
            received_at: TimestampMicros::now(),
        };

        let dataset = DatasetTypeId::builtin(builtin::DATASET_TRACE_SUMMARY);

        let result = if rows.internal {
            with_suppression(self.intake.intake_self_telemetry_dataset(batch, dataset)).await
        } else {
            with_suppression(self.intake.intake_derived_dataset(batch, dataset)).await
        }
        .map_err(|error| error.to_string())?;

        if result.rejected != 0 {
            return Err(format!(
                "Trace summary storage rejected {} rows",
                result.rejected
            ));
        }

        Ok(())
    }
}

#[async_trait]
impl TraceSink for SelfIntakeTraceSink {
    fn name(&self) -> &'static str {
        "self_intake"
    }

    fn takes(&self, trace: &DecidedTrace) -> bool {
        self.stores_own_traces || trace.stream.is_some()
    }

    async fn export(&self, traces: &mut Vec<DecidedTrace>) -> Result<(), String> {
        self.write_unwritten_summaries().await?;

        let groups = self.groups(traces).await?;
        let mut written = HashSet::new();
        let mut failure = None;

        for ((org_id, stream, internal), group) in groups {
            if !group.spans.is_empty() {
                let spans = Rows {
                    org_id: org_id.clone(),
                    stream: stream.clone(),
                    internal,
                    events: group.spans,
                };

                if let Err(error) = self.write_spans(spans).await {
                    failure.get_or_insert(error);
                    continue;
                }
            }

            written.extend(group.traces);

            if group.summaries.is_empty() {
                continue;
            }

            let summaries = Rows {
                org_id,
                stream,
                internal,
                events: group.summaries,
            };

            if let Err(error) = self.write_summaries(&summaries).await {
                self.unwritten_summaries.lock().push(summaries);
                failure.get_or_insert(error);
            }
        }

        let Some(error) = failure else {
            traces.clear();
            return Ok(());
        };

        let mut index = 0;

        traces.retain(|_| {
            let keep = !written.contains(&index);
            index += 1;
            keep
        });

        Err(error)
    }
}
