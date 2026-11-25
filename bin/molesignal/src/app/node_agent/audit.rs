// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use kernel::{ids::Id, time::TimestampMicros};
use postgres::repositories::audit_events::{AuditEvent, AuditEventRepository};
use serde_json::Value;

use super::Actor;

/// Audit records of the Node Agent control plane: enrollment, certificates, drain, revocation,
/// configuration, quota and tasks. Written whoever acts, a member over HTTP or a node over gRPC.
pub(super) struct Audit {
    events: Arc<dyn AuditEventRepository>,
}

impl Audit {
    pub(super) fn new(events: Arc<dyn AuditEventRepository>) -> Self {
        Self { events }
    }

    pub(super) async fn record(
        &self,
        org_id: &Id,
        actor: &Actor,
        action: &str,
        target: (&str, &str),
        payload: Value,
    ) {
        let event = AuditEvent {
            id: Id::new(),
            org_id: org_id.clone(),
            actor_kind: actor.kind.clone(),
            actor_id: actor.id.clone(),
            action: action.into(),
            target_kind: Some(target.0.into()),
            target_id: Some(target.1.into()),
            ip: None,
            user_agent: None,
            payload,
            ts: TimestampMicros::now(),
        };

        if let Err(error) = self.events.record(event).await {
            tracing::warn!(action, target_id = target.1, %error, "failed to record a node agent audit event");
        }
    }
}
