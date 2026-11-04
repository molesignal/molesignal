// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Writes the queued audit events to each organization's `_audit` stream, in batches.
//!
//! One per node, at the other end of the node's [`crate::app::audit_stream::AuditStreamQueue`].
//! A batch takes the events queued at the time, up to 64, and writes each organization's rows at
//! once. A batch that cannot be written is logged and left: the events stay in Postgres.

use std::{collections::HashMap, sync::Arc};

use tokio::task::JoinHandle;

use crate::{
    app::{
        audit_stream::{AUDIT_STREAM, AuditStreamReceiver, stream_fields},
        intake::IntakeService,
    },
    domain::{
        intake::{IntakeBatch, RawEvent},
        stream::StreamType,
    },
    infra::persistence::repositories::audit_events::AuditEvent,
    shared::{ids::Id, time::TimestampMicros},
};

const BATCH: usize = 64;

pub struct AuditStreamWriter {
    receiver: AuditStreamReceiver,
    intake: Arc<IntakeService>,
}

impl AuditStreamWriter {
    pub fn new(receiver: AuditStreamReceiver, intake: Arc<IntakeService>) -> Self {
        Self { receiver, intake }
    }

    pub fn spawn(mut self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut pending = Vec::with_capacity(BATCH);

            while let Some(first) = self.receiver.recv().await {
                pending.push(first);

                while pending.len() < BATCH {
                    match self.receiver.try_recv() {
                        Ok(event) => pending.push(event),
                        Err(_) => break,
                    }
                }

                self.flush(std::mem::take(&mut pending)).await;
            }
        })
    }

    #[tracing::instrument(
        name = "worker.audit_stream",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "audit_stream")
    )]
    async fn flush(&self, events: Vec<AuditEvent>) {
        let mut by_org: HashMap<Id, Vec<RawEvent>> = HashMap::new();

        for event in events {
            by_org
                .entry(event.org_id.clone())
                .or_default()
                .push(RawEvent {
                    timestamp: event.ts,
                    fields: stream_fields(&event),
                });
        }

        for (org_id, events) in by_org {
            let written = self
                .intake
                .intake_internal_telemetry(IntakeBatch {
                    batch_id: Id::new(),
                    org_id: org_id.clone(),
                    stream: AUDIT_STREAM.into(),
                    stream_type: StreamType::LOGS,
                    events,
                    received_at: TimestampMicros::now(),
                })
                .await;

            if let Err(error) = written {
                tracing::debug!(org = %org_id.0, %error, "audit events were not written");
            }
        }
    }
}
