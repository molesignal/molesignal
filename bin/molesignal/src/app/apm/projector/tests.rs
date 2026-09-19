// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use parking_lot::Mutex as ParkingMutex;

use super::*;
use crate::{
    domain::apm::{
        APM_FACT_SCHEMA_VERSION, ApmOutcome, ApmSpanKind, ErrorGroupRecord, ErrorSample,
        InstrumentationMetadata, OwnerSnapshot, ProjectionGap, ProjectionState, ServiceIdentity,
        ServiceObservation, SnapshotWriteStats, VersionObservation,
    },
    shared::{Result, ids::Id},
};

#[derive(Default)]
struct MemoryWriter {
    fail: AtomicBool,
    snapshots: ParkingMutex<Vec<OwnerSnapshot>>,
    gaps: ParkingMutex<Vec<ProjectionGap>>,
}

#[async_trait]
impl ApmWriteRepository for MemoryWriter {
    async fn upsert_catalog(
        &self,
        _services: &[ServiceObservation],
        _versions: &[VersionObservation],
    ) -> Result<()> {
        if self.fail.load(Ordering::Relaxed) {
            return Err(crate::shared::Error::internal("unavailable"));
        }
        Ok(())
    }

    async fn replace_owner_snapshots(
        &self,
        snapshots: &[OwnerSnapshot],
    ) -> Result<SnapshotWriteStats> {
        self.snapshots.lock().extend_from_slice(snapshots);
        Ok(SnapshotWriteStats {
            attempted: snapshots.len() as u64,
            applied: snapshots.len() as u64,
            stale: 0,
        })
    }

    async fn upsert_error_groups(
        &self,
        _groups: &[ErrorGroupRecord],
        _samples: &[ErrorSample],
        _max_samples_per_group: usize,
    ) -> Result<()> {
        Ok(())
    }

    async fn record_projection_gaps(&self, gaps: &[ProjectionGap]) -> Result<()> {
        self.gaps.lock().extend_from_slice(gaps);
        Ok(())
    }

    async fn ensure_projection_started(
        &self,
        org_id: &Id,
        started_at: TimestampMicros,
    ) -> Result<ProjectionState> {
        Ok(ProjectionState {
            org_id: org_id.clone(),
            projection_started_at: started_at,
            last_complete_bucket_at: None,
            last_rollup_bucket_at: None,
        })
    }

    async fn advance_projection_complete(
        &self,
        _org_id: &Id,
        _bucket_at: TimestampMicros,
    ) -> Result<()> {
        Ok(())
    }
}

fn config() -> ApmProjectorConfig {
    ApmProjectorConfig {
        queue_capacity: 8,
        flush_interval: Duration::from_millis(5),
        flush_max_snapshots: 100,
        shutdown_timeout: Duration::from_secs(1),
        late_grace: Duration::from_secs(60),
        max_exemplars_per_bucket: 2,
        max_error_samples_per_group: 2,
        histogram: HistogramSchema::v1(),
        cardinality: crate::config::ApmCardinalitySettings::default(),
    }
}

fn fact(span_id: &str) -> ApmSpanFact {
    ApmSpanFact {
        schema_version: APM_FACT_SCHEMA_VERSION,
        org_id: Id::from_string("org-1"),
        service: ServiceIdentity::new(None, Some("api"), None, None),
        service_version: None,
        service_instance_id: None,
        instrumentation: InstrumentationMetadata::default(),
        trace_id: "trace-1".into(),
        span_id: span_id.into(),
        parent_span_id: None,
        event_time: TimestampMicros::now(),
        duration_micros: 10,
        span_kind: ApmSpanKind::Server,
        outcome: ApmOutcome::Success,
        transaction: None,
        dependency: None,
        error: None,
        exception: None,
    }
}

#[tokio::test]
async fn accepted_sampled_out_fact_flushes_and_duplicate_does_not_double_count() {
    let writer = Arc::new(MemoryWriter::default());
    let projector =
        BufferedApmProjector::start("owner".into(), writer.clone(), config()).expect("start");
    projector.project(fact("one"), CandidateDisposition::Accepted);
    projector.project(fact("one"), CandidateDisposition::IdenticalDuplicate);
    projector.project(fact("one"), CandidateDisposition::ConflictingDuplicate);
    tokio::time::sleep(Duration::from_millis(20)).await;
    projector.shutdown().await;
    let snapshots = writer.snapshots.lock();
    let latest = snapshots.last().expect("snapshot");
    assert_eq!(latest.measurements.request_count, 1);
    assert!(!latest.measurements.exemplars[0].trace_available);
    assert_eq!(projector.health().duplicate_skips, 2);
}

#[tokio::test]
async fn repository_failure_degrades_only_apm_and_project_calls_stay_non_blocking() {
    let writer = Arc::new(MemoryWriter::default());
    writer.fail.store(true, Ordering::Relaxed);
    let mut projector_config = config();
    projector_config.queue_capacity = 1;
    let projector =
        BufferedApmProjector::start("owner".into(), writer, projector_config).expect("start");
    let started = std::time::Instant::now();
    for index in 0..100 {
        projector.project(
            fact(&format!("span-{index}")),
            CandidateDisposition::Accepted,
        );
    }
    assert!(started.elapsed() < Duration::from_millis(50));
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(projector.health().degraded);
    projector.shutdown().await;
}

#[tokio::test]
async fn facts_older_than_late_grace_become_explicit_gaps() {
    let writer = Arc::new(MemoryWriter::default());
    let projector =
        BufferedApmProjector::start("owner".into(), writer.clone(), config()).expect("start");
    let mut old = fact("old");
    old.event_time = TimestampMicros::from_secs(1);
    projector.project(old, CandidateDisposition::LateDropped);
    tokio::time::sleep(Duration::from_millis(20)).await;
    projector.shutdown().await;
    assert!(
        writer
            .gaps
            .lock()
            .iter()
            .any(|gap| gap.reason == ProjectionGapReason::LateDropped)
    );
}
