// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What `to_change` steps hold between events on this node, what each Kubernetes object last
//! looked like, and where the changes they note are kept once their batch is written.

use std::sync::Mutex;

use domain::pipeline::{
    change::{NotedChange, WatchedObjects},
    processing::ProcessingStep,
    realtime::RealtimePipeline,
};
use kernel::{ids::Id, time::TimestampMicros};
use serde_json::Value;

use crate::app::changes::{ChangeRecorder, ReportedChange};

pub struct ChangeWatch {
    watched: Mutex<WatchedObjects>,
    /// Without it what the steps note is not kept: a node built for tests.
    recorder: Option<ChangeRecorder>,
}

impl ChangeWatch {
    pub fn new(recorder: Option<ChangeRecorder>) -> Self {
        Self {
            watched: Mutex::new(WatchedObjects::default()),
            recorder,
        }
    }

    /// The change an event tells to a step that picks it.
    pub(super) fn note(
        &self,
        pipeline: &RealtimePipeline,
        step: &ProcessingStep,
        value: &Value,
        at: TimestampMicros,
    ) -> Option<NotedChange> {
        if !step.condition.matches(value) {
            return None;
        }

        let spec = step.change.clone().unwrap_or_default();
        let mut watched = self
            .watched
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        watched.note(&pipeline.org_id, value, &spec, at)
    }

    /// Keeps what a batch noted, as reported by the pipeline. A change that cannot be kept is
    /// logged: the events it was read of are written all the same.
    pub(super) async fn keep(&self, pipeline: &RealtimePipeline, noted: Vec<NotedChange>) {
        let Some(recorder) = &self.recorder else {
            return;
        };

        if noted.is_empty() {
            return;
        }

        let changes = noted
            .into_iter()
            .map(|change| ReportedChange {
                kind: change.kind,
                service: change.service,
                namespace: change.namespace,
                environment: change.environment,
                version: change.version,
                description: Some(change.description),
                deployed_at: Some(change.at.0),
                ..ReportedChange::default()
            })
            .collect::<Vec<_>>();

        let count = changes.len();

        if let Err(error) = recorder
            .record(&pipeline.org_id, changes, &Id(pipeline.id.0.clone()))
            .await
        {
            tracing::warn!(%error, pipeline = %pipeline.id.0, count, "changes a pipeline noted were not kept");
        }
    }
}
