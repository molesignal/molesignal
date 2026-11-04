// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What the steps that keep count hold between events on this node: the metrics being added up
//! ([`GeneratedMetrics`]) and the counts of quotas and throttles ([`Limits`]). A worker writes them
//! out every few seconds ([`StepState::tick`]); a node that stops writes the rest first
//! ([`StepState::finish`]).
use std::sync::Arc;

use tokio::sync::Mutex;

use super::{
    generated::{GeneratedMetrics, MetricBatch},
    limits::Limits,
};
use crate::{
    app::intake::IntakeService,
    domain::{intake::IntakeBatch, pipeline::limit::LimitUsageLedger, stream::StreamType},
    shared::{ids::Id, time::TimestampMicros},
};

/// A batch of samples whose write failed is tried this many times in all.
const WRITE_ATTEMPTS: u32 = 3;

pub struct StepState {
    pub(super) metrics: GeneratedMetrics,
    pub(super) limits: Limits,
    /// Samples whose write failed, with the attempts made; one write at a time.
    unwritten: Mutex<Vec<(MetricBatch, u32)>>,
}

impl StepState {
    pub fn new(ledger: Arc<dyn LimitUsageLedger>) -> Self {
        Self {
            metrics: GeneratedMetrics::new(),
            limits: Limits::new(ledger),
            unwritten: Mutex::new(Vec::new()),
        }
    }

    /// Where quotas and throttles keep their counts.
    pub fn ledger(&self) -> &Arc<dyn LimitUsageLedger> {
        self.limits.ledger()
    }

    /// Writes the samples of the intervals that are over and syncs the counts of the limits.
    pub async fn tick(&self, intake: &IntakeService, now: TimestampMicros) {
        let mut unwritten = self.unwritten.lock().await;

        let mut batches: Vec<(MetricBatch, u32)> = std::mem::take(&mut *unwritten);

        batches.extend(
            self.metrics
                .take_due(now)
                .into_iter()
                .map(|batch| (batch, 0)),
        );

        for (batch, attempts) in batches {
            if let Err(batch) = write(intake, batch, now).await
                && attempts + 1 < WRITE_ATTEMPTS
            {
                unwritten.push((batch, attempts + 1));
            }
        }

        // Still holding `unwritten`: syncs of the limits take turns too.
        if let Err(error) = self.limits.sync(now).await {
            tracing::warn!(%error, "pipeline limit counts not written; trying again with the next sync");
        }
    }

    /// Writes every sample and count this node holds, as it stops.
    pub async fn finish(&self, intake: &IntakeService) {
        let now = TimestampMicros::now();
        let mut unwritten = self.unwritten.lock().await;

        let mut batches: Vec<MetricBatch> = unwritten.drain(..).map(|(batch, _)| batch).collect();
        batches.extend(self.metrics.take_all(now));

        for batch in batches {
            let _ = write(intake, batch, now).await;
        }

        if let Err(error) = self.limits.sync(now).await {
            tracing::warn!(%error, "pipeline limit counts could not be written before exit");
        }
    }
}

/// Writes one stream's samples the way a pipeline writes its output: the stream is made when it is
/// missing and no pipeline of the metrics stream runs on them. The batch comes back when it failed.
async fn write(
    intake: &IntakeService,
    batch: MetricBatch,
    now: TimestampMicros,
) -> Result<(), MetricBatch> {
    let result = intake
        .intake_pipeline_output(IntakeBatch {
            batch_id: Id::new(),
            org_id: batch.org.clone(),
            stream: batch.stream.clone(),
            stream_type: StreamType::METRICS,
            events: batch.samples.clone(),
            received_at: now,
        })
        .await;

    match result {
        Ok(result) => {
            if let Some(first) = result.errors.first() {
                tracing::warn!(
                    org_id = %batch.org.0,
                    stream = %batch.stream,
                    refused = result.rejected,
                    reason = %first.reason,
                    "the metrics stream refused generated samples"
                );
            }

            Ok(())
        }
        Err(error) => {
            tracing::warn!(
                %error,
                org_id = %batch.org.0,
                stream = %batch.stream,
                samples = batch.samples.len(),
                "generated metric samples not written"
            );

            Err(batch)
        }
    }
}
