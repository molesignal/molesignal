// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Every audit event, also as a row of the organization's `_audit` log stream
//! ([`AUDIT_STREAM`]), where it can be searched and delivered elsewhere — to a SIEM — by a
//! scheduled pipeline. Postgres keeps the record the audit pages read; reading the stream needs
//! `audit.read`, as they do.
//!
//! An event is queued once Postgres has it, cleaned the way Postgres keeps it, and written in
//! batches by a background worker (`bootstrap::workers::audit_stream`): an audited action does
//! not wait on intake. When the queue is full the row is dropped; the record stays in Postgres.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;

pub use crate::domain::stream::AUDIT_STREAM;
use crate::{
    infra::persistence::repositories::audit_events::{
        AuditEvent, AuditEventRepository, AuditQuery, AuditRangeSummary, sanitize_audit_event,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// Events waiting to be written, at most.
const QUEUE_CAPACITY: usize = 4096;

#[derive(Clone)]
pub struct AuditStreamQueue {
    sender: mpsc::Sender<AuditEvent>,
}

pub type AuditStreamReceiver = mpsc::Receiver<AuditEvent>;

/// Both ends of the queue: the sender for the audit trail, the receiver for the writer.
pub fn audit_stream_queue() -> (AuditStreamQueue, AuditStreamReceiver) {
    let (sender, receiver) = mpsc::channel(QUEUE_CAPACITY);
    (AuditStreamQueue { sender }, receiver)
}

impl AuditStreamQueue {
    pub fn push(&self, event: AuditEvent) {
        if let Err(error) = self.sender.try_send(event) {
            tracing::debug!(%error, "audit stream row dropped");
        }
    }
}

/// The audit trail: an event is recorded in Postgres, then queued for the `_audit` stream.
pub struct StreamedAuditEvents {
    inner: Arc<dyn AuditEventRepository>,
    queue: AuditStreamQueue,
}

impl StreamedAuditEvents {
    pub fn new(inner: Arc<dyn AuditEventRepository>, queue: AuditStreamQueue) -> Self {
        Self { inner, queue }
    }
}

#[async_trait]
impl AuditEventRepository for StreamedAuditEvents {
    async fn record(&self, event: AuditEvent) -> Result<()> {
        let mut row = event.clone();
        sanitize_audit_event(&mut row);

        self.inner.record(event).await?;
        self.queue.push(row);
        Ok(())
    }

    async fn list_recent(&self, org_id: &Id, limit: i64) -> Result<Vec<AuditEvent>> {
        self.inner.list_recent(org_id, limit).await
    }

    async fn query(&self, org_id: &Id, q: &AuditQuery) -> Result<Vec<AuditEvent>> {
        self.inner.query(org_id, q).await
    }

    async fn delete_before(&self, before: TimestampMicros, limit: i64) -> Result<u64> {
        self.inner.delete_before(before, limit).await
    }

    async fn summarize_range(
        &self,
        from: Option<TimestampMicros>,
        before: TimestampMicros,
    ) -> Result<AuditRangeSummary> {
        self.inner.summarize_range(from, before).await
    }
}

/// The row of an event. Field names carry no dots, which SQL reads as qualifiers; a value an event
/// does not have is left out rather than written as null.
pub fn stream_fields(event: &AuditEvent) -> Map<String, Value> {
    let mut fields = Map::new();

    fields.insert("audit_id".into(), json!(event.id.0));
    fields.insert("actor_kind".into(), json!(event.actor_kind));
    fields.insert("actor_id".into(), json!(event.actor_id));
    fields.insert("action".into(), json!(event.action));

    let optional = [
        ("target_kind", &event.target_kind),
        ("target_id", &event.target_id),
        ("ip", &event.ip),
        ("user_agent", &event.user_agent),
    ];

    for (name, value) in optional {
        if let Some(value) = value {
            fields.insert(name.into(), json!(value));
        }
    }

    if !event.payload.is_null() {
        fields.insert("payload".into(), event.payload.clone());
    }

    fields
}

#[cfg(test)]
mod tests;
