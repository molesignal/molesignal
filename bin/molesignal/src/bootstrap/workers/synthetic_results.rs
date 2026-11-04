// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Writes the queued runs of synthetic checks to each organization's `_synthetic_results`
//! stream, in batches.
//!
//! One per node, at the other end of the node's [`crate::app::synthetics::ResultStreamQueue`].
//! A batch takes the runs queued at the time, up to 64, names their checks and locations, and
//! writes each organization's rows at once. A batch that cannot be written is logged and left:
//! the runs stay in Postgres.

use std::{collections::HashMap, sync::Arc};

use tokio::task::JoinHandle;

use crate::{
    app::{
        intake::IntakeService,
        synthetics::{RESULT_STREAM, ResultStreamReceiver, stream_fields},
    },
    domain::{
        intake::{IntakeBatch, RawEvent},
        stream::StreamType,
        synthetics::{SyntheticMonitor, SyntheticRepository, SyntheticResult},
    },
    shared::{ids::Id, time::TimestampMicros},
};

const BATCH: usize = 64;

pub struct SyntheticResultWriter {
    receiver: ResultStreamReceiver,
    repository: Arc<dyn SyntheticRepository>,
    intake: Arc<IntakeService>,
}

impl SyntheticResultWriter {
    pub fn new(
        receiver: ResultStreamReceiver,
        repository: Arc<dyn SyntheticRepository>,
        intake: Arc<IntakeService>,
    ) -> Self {
        Self {
            receiver,
            repository,
            intake,
        }
    }

    pub fn spawn(mut self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut pending = Vec::with_capacity(BATCH);

            while let Some(first) = self.receiver.recv().await {
                pending.push(first);

                while pending.len() < BATCH {
                    match self.receiver.try_recv() {
                        Ok(result) => pending.push(result),
                        Err(_) => break,
                    }
                }

                self.flush(std::mem::take(&mut pending)).await;
            }
        })
    }

    #[tracing::instrument(
        name = "worker.synthetic_results",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "synthetic_results")
    )]
    async fn flush(&self, results: Vec<SyntheticResult>) {
        let mut by_org: HashMap<Id, Vec<SyntheticResult>> = HashMap::new();

        for result in results {
            by_org
                .entry(result.organization_id.clone())
                .or_default()
                .push(result);
        }

        for (org_id, results) in by_org {
            let events = self.rows(&org_id, &results).await;

            let written = self
                .intake
                .intake_internal_telemetry(IntakeBatch {
                    batch_id: Id::new(),
                    org_id: org_id.clone(),
                    stream: RESULT_STREAM.into(),
                    stream_type: StreamType::LOGS,
                    events,
                    received_at: TimestampMicros::now(),
                })
                .await;

            if let Err(error) = written {
                tracing::debug!(org = %org_id.0, %error, "synthetic results were not written");
            }
        }
    }

    /// The rows of one organization's runs, each named after its check and location. A name
    /// that cannot be had is left out: the row still has the id.
    async fn rows(&self, org_id: &Id, results: &[SyntheticResult]) -> Vec<RawEvent> {
        let locations = match self.repository.list_locations(org_id).await {
            Ok(locations) => locations
                .into_iter()
                .map(|location| (location.id, location.name))
                .collect::<HashMap<_, _>>(),
            Err(error) => {
                tracing::debug!(org = %org_id.0, %error, "synthetic locations were not named");
                HashMap::new()
            }
        };

        let mut monitors: HashMap<Id, Option<SyntheticMonitor>> = HashMap::new();
        let mut events = Vec::with_capacity(results.len());

        for result in results {
            if !monitors.contains_key(&result.monitor_id) {
                let monitor = self
                    .repository
                    .get_monitor(org_id, &result.monitor_id)
                    .await
                    .ok();

                monitors.insert(result.monitor_id.clone(), monitor);
            }

            let monitor = monitors.get(&result.monitor_id).and_then(Option::as_ref);
            let location = locations.get(&result.location_id).map(String::as_str);

            events.push(RawEvent {
                // A run is placed at the moment it started.
                timestamp: result.started_at,
                fields: stream_fields(result, monitor, location),
            });
        }

        events
    }
}
