// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Mutex;

use super::*;
use crate::domain::stream::is_platform_stream;

/// The events as they reach Postgres.
#[derive(Default)]
struct Recorded(Mutex<Vec<AuditEvent>>);

#[async_trait]
impl AuditEventRepository for Recorded {
    async fn record(&self, event: AuditEvent) -> Result<()> {
        self.0.lock().unwrap().push(event);
        Ok(())
    }

    async fn list_recent(&self, _: &Id, _: i64) -> Result<Vec<AuditEvent>> {
        Ok(Vec::new())
    }

    async fn query(&self, _: &Id, _: &AuditQuery) -> Result<Vec<AuditEvent>> {
        Ok(Vec::new())
    }

    async fn delete_before(&self, _: TimestampMicros, _: i64) -> Result<u64> {
        Ok(0)
    }

    async fn summarize_range(
        &self,
        _: Option<TimestampMicros>,
        _: TimestampMicros,
    ) -> Result<AuditRangeSummary> {
        Ok(AuditRangeSummary {
            rows: 7,
            earliest: Some(TimestampMicros(3)),
        })
    }
}

fn event() -> AuditEvent {
    AuditEvent {
        id: Id("audit-1".into()),
        org_id: Id("org-a".into()),
        actor_kind: "user".into(),
        actor_id: "u-1".into(),
        action: "connector.update".into(),
        target_kind: Some("connector".into()),
        target_id: None,
        ip: Some("10.0.0.7".into()),
        user_agent: None,
        payload: json!({ "name": "Archive", "password": "hunter2" }),
        ts: TimestampMicros(1_000),
    }
}

#[tokio::test]
async fn an_event_goes_to_postgres_then_to_the_stream_cleaned() {
    let recorded = Arc::new(Recorded::default());
    let (queue, mut receiver) = audit_stream_queue();
    let trail = StreamedAuditEvents::new(recorded.clone(), queue);

    trail.record(event()).await.unwrap();

    // Postgres cleans what it keeps itself; the stream gets what it would keep.
    assert_eq!(recorded.0.lock().unwrap()[0].payload["password"], "hunter2");

    let row = receiver.try_recv().unwrap();
    assert_eq!(row.payload, json!({ "name": "Archive" }));
}

#[tokio::test]
async fn what_a_retention_change_would_delete_is_read_from_postgres() {
    let (queue, _receiver) = audit_stream_queue();
    let trail = StreamedAuditEvents::new(Arc::new(Recorded::default()), queue);

    let summary = trail
        .summarize_range(None, TimestampMicros(10))
        .await
        .unwrap();

    assert_eq!(summary.rows, 7);
    assert_eq!(summary.earliest, Some(TimestampMicros(3)));
}

#[test]
fn a_row_holds_what_the_event_has() {
    assert_eq!(
        Value::Object(stream_fields(&event())),
        json!({
            "audit_id": "audit-1",
            "actor_kind": "user",
            "actor_id": "u-1",
            "action": "connector.update",
            "target_kind": "connector",
            "ip": "10.0.0.7",
            "payload": { "name": "Archive", "password": "hunter2" },
        })
    );
}

#[test]
fn the_streams_the_platform_writes_are_its_own() {
    for stream in [
        AUDIT_STREAM,
        crate::app::synthetics::RESULT_STREAM,
        crate::agent::telemetry::AGENT_STREAM,
        crate::agent::scoring::SCORE_STREAM,
    ] {
        assert!(is_platform_stream(stream), "{stream}");
    }
}
