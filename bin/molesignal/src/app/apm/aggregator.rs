// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Bounded owner-local minute aggregation and absolute snapshot construction.

use std::collections::{HashMap, HashSet};

use crate::{
    domain::apm::{
        APM_PERSISTENCE_SCHEMA_VERSION, ApmOutcome, ApmSpanFact, BucketDimension,
        BucketMeasurements, ErrorGroupRecord, ErrorSample, HistogramSchema, LatencyHistogram,
        OwnerSnapshot, ServiceIdentity, ServiceObservation, TraceExemplar, VersionObservation,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

mod support;

use support::{admit_error_sample, admit_exemplar, dimension_is_overflow};

const MINUTE_MICROS: i64 = 60_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BucketKey {
    org_id: Id,
    bucket_at: TimestampMicros,
    dimension: BucketDimension,
}

struct BucketState {
    measurements: BucketMeasurements,
    snapshot_seq: u64,
    dirty: bool,
}

struct CatalogState {
    observation: ServiceObservation,
    instances: HashSet<String>,
    dirty: bool,
}

struct VersionState {
    observation: VersionObservation,
    dirty: bool,
}

struct ErrorState {
    record: ErrorGroupRecord,
    samples: Vec<ErrorSample>,
    dirty: bool,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ApmFlushBatch {
    pub snapshots: Vec<OwnerSnapshot>,
    pub services: Vec<ServiceObservation>,
    pub versions: Vec<VersionObservation>,
    pub error_groups: Vec<ErrorGroupRecord>,
    pub error_samples: Vec<ErrorSample>,
    pub projection_starts: Vec<(Id, TimestampMicros)>,
}

impl ApmFlushBatch {
    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
            && self.services.is_empty()
            && self.versions.is_empty()
            && self.error_groups.is_empty()
            && self.error_samples.is_empty()
            && self.projection_starts.is_empty()
    }

    pub fn latest_buckets_by_org(&self) -> HashMap<Id, TimestampMicros> {
        let mut latest = HashMap::new();
        for snapshot in &self.snapshots {
            latest
                .entry(snapshot.org_id.clone())
                .and_modify(|current: &mut TimestampMicros| {
                    *current = (*current).max(snapshot.bucket_at)
                })
                .or_insert(snapshot.bucket_at);
        }
        latest
    }
}

pub struct ApmAggregator {
    owner_id: String,
    histogram: HistogramSchema,
    max_exemplars: usize,
    max_error_samples: usize,
    next_snapshot_seq: u64,
    buckets: HashMap<BucketKey, BucketState>,
    catalog: HashMap<(Id, ServiceIdentity), CatalogState>,
    versions: HashMap<(Id, ServiceIdentity, String), VersionState>,
    errors: HashMap<(Id, String), ErrorState>,
    projection_starts: HashMap<Id, TimestampMicros>,
    dirty_projection_starts: HashSet<Id>,
}

impl ApmAggregator {
    pub fn new(
        owner_id: String,
        histogram: HistogramSchema,
        max_exemplars: usize,
        max_error_samples: usize,
    ) -> Result<Self> {
        histogram.validate()?;
        Ok(Self {
            owner_id,
            histogram,
            max_exemplars: max_exemplars.max(1),
            max_error_samples: max_error_samples.max(1),
            next_snapshot_seq: 1,
            buckets: HashMap::new(),
            catalog: HashMap::new(),
            versions: HashMap::new(),
            errors: HashMap::new(),
            projection_starts: HashMap::new(),
            dirty_projection_starts: HashSet::new(),
        })
    }

    pub fn observe(&mut self, fact: ApmSpanFact, trace_available: bool) -> Result<()> {
        self.observe_catalog(&fact);
        let exemplar = TraceExemplar {
            trace_id: fact.trace_id.clone(),
            span_id: fact.span_id.clone(),
            event_time: fact.event_time,
            duration_micros: fact.duration_micros,
            trace_available,
        };
        if fact.contributes_service_red() {
            self.observe_bucket(
                &fact,
                BucketDimension::Service {
                    service: fact.service.clone(),
                    version: fact.service_version.clone(),
                },
                exemplar.clone(),
            )?;
            if let Some(transaction) = fact.transaction.clone() {
                self.observe_bucket(
                    &fact,
                    BucketDimension::Transaction {
                        service: fact.service.clone(),
                        version: fact.service_version.clone(),
                        transaction,
                    },
                    exemplar.clone(),
                )?;
            }
        }
        if fact.contributes_dependency()
            && let Some(dependency) = fact.dependency.clone()
        {
            self.observe_bucket(
                &fact,
                BucketDimension::Dependency {
                    service: fact.service.clone(),
                    version: fact.service_version.clone(),
                    dependency,
                },
                exemplar.clone(),
            )?;
        }
        if let Some(error) = fact.error.clone() {
            self.observe_bucket(
                &fact,
                BucketDimension::Error {
                    service: fact.service.clone(),
                    version: fact.service_version.clone(),
                    error: error.clone(),
                },
                exemplar,
            )?;
            self.observe_error(&fact, error, trace_available);
        }
        Ok(())
    }

    pub fn flush_batch(&self, now: TimestampMicros, max_snapshots: usize) -> ApmFlushBatch {
        let snapshots = self
            .buckets
            .iter()
            .filter(|(_, state)| state.dirty)
            .take(max_snapshots.max(1))
            .map(|(key, state)| OwnerSnapshot {
                schema_version: APM_PERSISTENCE_SCHEMA_VERSION,
                org_id: key.org_id.clone(),
                owner_id: self.owner_id.clone(),
                bucket_at: key.bucket_at,
                snapshot_seq: state.snapshot_seq,
                dimension: key.dimension.clone(),
                measurements: state.measurements.clone(),
                updated_at: now,
            })
            .collect();
        ApmFlushBatch {
            snapshots,
            services: self
                .catalog
                .values()
                .filter(|state| state.dirty)
                .map(|state| state.observation.clone())
                .collect(),
            versions: self
                .versions
                .values()
                .filter(|state| state.dirty)
                .map(|state| state.observation.clone())
                .collect(),
            error_groups: self
                .errors
                .values()
                .filter(|state| state.dirty)
                .map(|state| state.record.clone())
                .collect(),
            error_samples: self
                .errors
                .values()
                .filter(|state| state.dirty)
                .flat_map(|state| state.samples.clone())
                .collect(),
            projection_starts: self
                .dirty_projection_starts
                .iter()
                .filter_map(|org_id| {
                    self.projection_starts
                        .get(org_id)
                        .map(|started| (org_id.clone(), *started))
                })
                .collect(),
        }
    }

    pub fn acknowledge(&mut self, batch: &ApmFlushBatch) {
        for snapshot in &batch.snapshots {
            let key = BucketKey {
                org_id: snapshot.org_id.clone(),
                bucket_at: snapshot.bucket_at,
                dimension: snapshot.dimension.clone(),
            };
            if let Some(state) = self.buckets.get_mut(&key)
                && state.snapshot_seq == snapshot.snapshot_seq
            {
                state.dirty = false;
            }
        }
        for observation in &batch.services {
            if let Some(state) = self
                .catalog
                .get_mut(&(observation.org_id.clone(), observation.service.clone()))
            {
                state.dirty = false;
            }
        }
        for observation in &batch.versions {
            if let Some(state) = self.versions.get_mut(&(
                observation.org_id.clone(),
                observation.service.clone(),
                observation.version.clone(),
            )) {
                state.dirty = false;
            }
        }
        for record in &batch.error_groups {
            if let Some(state) = self
                .errors
                .get_mut(&(record.org_id.clone(), record.error.fingerprint.clone()))
            {
                state.dirty = false;
            }
        }
        for (org_id, _) in &batch.projection_starts {
            self.dirty_projection_starts.remove(org_id);
        }
    }

    /// Drops only acknowledged closed state. A late fact admitted by the
    /// worker always gets a process-global sequence newer than any evicted
    /// snapshot.
    pub fn evict_acked_before(&mut self, cutoff: TimestampMicros) {
        self.buckets
            .retain(|key, state| state.dirty || key.bucket_at >= cutoff);
        self.catalog
            .retain(|_, state| state.dirty || state.observation.last_seen_at >= cutoff);
        self.versions
            .retain(|_, state| state.dirty || state.observation.last_seen_at >= cutoff);
        self.errors
            .retain(|_, state| state.dirty || state.record.last_seen_at >= cutoff);
    }

    pub fn pending_snapshot_count(&self) -> usize {
        self.buckets.values().filter(|state| state.dirty).count()
    }

    fn observe_bucket(
        &mut self,
        fact: &ApmSpanFact,
        dimension: BucketDimension,
        exemplar: TraceExemplar,
    ) -> Result<()> {
        let bucket_at =
            TimestampMicros(fact.event_time.0.div_euclid(MINUTE_MICROS) * MINUTE_MICROS);
        let key = BucketKey {
            org_id: fact.org_id.clone(),
            bucket_at,
            dimension,
        };
        let snapshot_seq = self.next_snapshot_seq;
        self.next_snapshot_seq = self.next_snapshot_seq.saturating_add(1);
        let state = self.buckets.entry(key).or_insert_with(|| BucketState {
            measurements: BucketMeasurements {
                request_count: 0,
                error_count: 0,
                overflow_count: 0,
                latency: LatencyHistogram::empty(&self.histogram),
                exemplars: Vec::new(),
            },
            snapshot_seq,
            dirty: true,
        });
        state.snapshot_seq = snapshot_seq;
        state.dirty = true;
        state.measurements.request_count = state.measurements.request_count.saturating_add(1);
        if fact.outcome == ApmOutcome::Error {
            state.measurements.error_count = state.measurements.error_count.saturating_add(1);
        }
        if dimension_is_overflow(fact) {
            state.measurements.overflow_count = state.measurements.overflow_count.saturating_add(1);
        }
        state
            .measurements
            .latency
            .observe(&self.histogram, fact.duration_micros)?;
        admit_exemplar(
            &mut state.measurements.exemplars,
            exemplar,
            self.max_exemplars,
        );
        Ok(())
    }

    fn observe_catalog(&mut self, fact: &ApmSpanFact) {
        self.projection_starts
            .entry(fact.org_id.clone())
            .and_modify(|started| *started = (*started).min(fact.event_time))
            .or_insert(fact.event_time);
        self.dirty_projection_starts.insert(fact.org_id.clone());
        let state = self
            .catalog
            .entry((fact.org_id.clone(), fact.service.clone()))
            .or_insert_with(|| CatalogState {
                observation: ServiceObservation {
                    org_id: fact.org_id.clone(),
                    service: fact.service.clone(),
                    first_seen_at: fact.event_time,
                    last_seen_at: fact.event_time,
                    runtime_language: fact.instrumentation.language.clone(),
                    telemetry_sdk_name: fact.instrumentation.sdk_name.clone(),
                    telemetry_sdk_version: fact.instrumentation.sdk_version.clone(),
                    recent_instance_count: 0,
                },
                instances: HashSet::new(),
                dirty: true,
            });
        state.observation.first_seen_at = state.observation.first_seen_at.min(fact.event_time);
        state.observation.last_seen_at = state.observation.last_seen_at.max(fact.event_time);
        state.observation.runtime_language = fact
            .instrumentation
            .language
            .clone()
            .or(state.observation.runtime_language.take());
        state.observation.telemetry_sdk_name = fact
            .instrumentation
            .sdk_name
            .clone()
            .or(state.observation.telemetry_sdk_name.take());
        state.observation.telemetry_sdk_version = fact
            .instrumentation
            .sdk_version
            .clone()
            .or(state.observation.telemetry_sdk_version.take());
        if let Some(instance) = &fact.service_instance_id {
            state.instances.insert(instance.clone());
        }
        state.observation.recent_instance_count =
            u32::try_from(state.instances.len()).unwrap_or(u32::MAX);
        state.dirty = true;

        if let Some(version) = &fact.service_version {
            let version_state = self
                .versions
                .entry((fact.org_id.clone(), fact.service.clone(), version.clone()))
                .or_insert_with(|| VersionState {
                    observation: VersionObservation {
                        org_id: fact.org_id.clone(),
                        service: fact.service.clone(),
                        version: version.clone(),
                        first_seen_at: fact.event_time,
                        last_seen_at: fact.event_time,
                        observation_count: 0,
                    },
                    dirty: true,
                });
            version_state.observation.first_seen_at =
                version_state.observation.first_seen_at.min(fact.event_time);
            version_state.observation.last_seen_at =
                version_state.observation.last_seen_at.max(fact.event_time);
            version_state.observation.observation_count = version_state
                .observation
                .observation_count
                .saturating_add(1);
            version_state.dirty = true;
        }
    }

    fn observe_error(
        &mut self,
        fact: &ApmSpanFact,
        error: crate::domain::apm::ErrorIdentity,
        trace_available: bool,
    ) {
        let exception = fact.exception.as_ref();
        let state = self
            .errors
            .entry((fact.org_id.clone(), error.fingerprint.clone()))
            .or_insert_with(|| ErrorState {
                record: ErrorGroupRecord {
                    org_id: fact.org_id.clone(),
                    error: error.clone(),
                    service: fact.service.clone(),
                    first_seen_at: fact.event_time,
                    last_seen_at: fact.event_time,
                    occurrence_count: 0,
                    representative_message: exception.and_then(|value| value.message.clone()),
                    representative_stack: exception
                        .map(|value| value.stack_frames.clone())
                        .unwrap_or_default(),
                },
                samples: Vec::new(),
                dirty: true,
            });
        state.record.first_seen_at = state.record.first_seen_at.min(fact.event_time);
        state.record.last_seen_at = state.record.last_seen_at.max(fact.event_time);
        state.record.occurrence_count = state.record.occurrence_count.saturating_add(1);
        let sample = ErrorSample {
            org_id: fact.org_id.clone(),
            error,
            service: fact.service.clone(),
            event_time: fact.event_time,
            trace_id: fact.trace_id.clone(),
            span_id: fact.span_id.clone(),
            trace_available,
            representative_message: exception.and_then(|value| value.message.clone()),
            representative_stack: exception
                .map(|value| value.stack_frames.clone())
                .unwrap_or_default(),
        };
        admit_error_sample(&mut state.samples, sample, self.max_error_samples);
        state.dirty = true;
    }
}

#[cfg(test)]
mod tests;
