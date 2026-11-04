// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use parking_lot::Mutex;
use serde_json::{Map, Value};

use crate::{
    domain::{
        iam::DEFAULT_INTAKE_USAGE_MAX_SERVICES_PER_ORG,
        intake::{
            IntakeBatch, RawEvent,
            usage::source::{
                IntakeCredential, IntakeUsageRepository, IntakeUsageRow, MAX_SERVICE_NAME_CHARS,
                OTHER_SERVICES, UNNAMED_SERVICE, hour_bucket_start,
            },
        },
        stream::{StreamType, validate_stream_name},
    },
    shared::{Result, ids::Id, time::TimestampMicros, trace_normalization::CanonicalSpan},
};

/// The fields an event names its service in, in the order they are looked at.
const SERVICE_FIELDS: [&str; 3] = ["service.name", "service", "service_name"];

/// The most rows kept between two writes. A database that stays away for long costs the rows that
/// no longer fit, not the memory of the node.
const MAX_PENDING_ROWS: usize = 200_000;

/// The most rows written by one statement.
const WRITE_CHUNK_ROWS: usize = 5_000;

/// The service an event names, or none.
pub fn service_of_fields(fields: &Map<String, Value>) -> &str {
    SERVICE_FIELDS
        .iter()
        .find_map(|field| fields.get(*field).and_then(Value::as_str))
        .unwrap_or(UNNAMED_SERVICE)
}

/// The events of a request, by the service each names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServiceTally {
    events: HashMap<String, u64>,
}

impl ServiceTally {
    pub fn of_events(events: &[RawEvent]) -> Self {
        let mut tally = Self::default();
        tally.add_events(events);
        tally
    }

    pub fn of_spans(spans: &[CanonicalSpan]) -> Self {
        let mut tally = Self::default();

        for span in spans {
            let service = span
                .resource
                .attributes
                .get(SERVICE_FIELDS[0])
                .and_then(Value::as_str)
                .unwrap_or(UNNAMED_SERVICE);

            tally.add(service, 1);
        }

        tally
    }

    /// `events` events that all name one service.
    pub fn single(service: &str, events: u64) -> Self {
        let mut tally = Self::default();
        tally.add(service, events);
        tally
    }

    pub fn add_events(&mut self, events: &[RawEvent]) {
        // Events of one service usually come together, and comparing to the last costs less than
        // finding its count every time.
        let mut run = (UNNAMED_SERVICE, 0);

        for event in events {
            let service = service_of_fields(&event.fields);

            if service == run.0 {
                run.1 += 1;
            } else {
                self.add(run.0, run.1);
                run = (service, 1);
            }
        }

        self.add(run.0, run.1);
    }

    pub fn add(&mut self, service: &str, events: u64) {
        if events == 0 {
            return;
        }

        match self.events.get_mut(service) {
            Some(count) => *count += events,
            None => {
                self.events.insert(service.to_string(), events);
            }
        }
    }

    pub fn total(&self) -> u64 {
        self.events.values().sum()
    }
}

/// What a request brought, as it is measured before it is written.
#[derive(Debug, Clone)]
pub struct IntakeReceipt {
    pub credential: IntakeCredential,
    pub stream_type: StreamType,
    pub stream: String,
    /// The bytes of the request once decompressed.
    pub bytes: u64,
    pub services: ServiceTally,
}

impl IntakeReceipt {
    /// What the platform wrote itself: its own telemetry, which names no service.
    pub fn internal(batch: &IntakeBatch, bytes: u64) -> Self {
        Self {
            credential: IntakeCredential::internal(),
            stream_type: batch.stream_type,
            stream: batch.stream.clone(),
            bytes,
            services: ServiceTally::single(UNNAMED_SERVICE, batch.events.len() as u64),
        }
    }

    pub fn of_events(
        credential: IntakeCredential,
        stream_type: StreamType,
        stream: &str,
        bytes: u64,
        events: &[RawEvent],
    ) -> Self {
        Self {
            credential,
            stream_type,
            stream: stream.to_string(),
            bytes,
            services: ServiceTally::of_events(events),
        }
    }
}

/// `bytes` shared out by the counts of `events`: every share follows from the running total, so
/// that the shares add up to `bytes` exactly.
pub fn split_by_events(bytes: u64, events: &[u64]) -> Vec<u64> {
    let total = events.iter().sum::<u64>();

    if total == 0 {
        return vec![0; events.len()];
    }

    let mut running = 0_u64;
    let mut given = 0_u64;

    events
        .iter()
        .map(|count| {
            running += count;
            let reached = ((u128::from(bytes) * u128::from(running)) / u128::from(total)) as u64;
            let share = reached - given;
            given = reached;
            share
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct RowKey {
    org_id: String,
    bucket_start_micros: i64,
    credential: IntakeCredential,
    stream_type: StreamType,
    stream: String,
    service: String,
}

#[derive(Debug, Clone, Copy, Default)]
struct Measure {
    bytes: u64,
    events: u64,
}

/// The services of an organization seen in one hour.
#[derive(Debug, Default)]
struct HourServices {
    bucket_start_micros: i64,
    names: HashSet<String>,
}

#[derive(Debug, Default)]
struct MeterState {
    pending: HashMap<RowKey, Measure>,
    services: HashMap<String, HourServices>,
    /// Bytes that found no room since the last write.
    dropped_bytes: u64,
}

impl MeterState {
    /// The name a service is counted under: a name past the limit of its organization and hour
    /// shares the row of the others, so that a sender cannot grow the table by naming services.
    fn counted_service(
        &mut self,
        org_id: &str,
        bucket: i64,
        service: &str,
        limit: usize,
    ) -> String {
        let name = kept_service_name(service);

        if name.is_empty() {
            return name;
        }

        if !self.services.contains_key(org_id) {
            self.services
                .insert(org_id.to_string(), HourServices::default());
        }

        let Some(seen) = self.services.get_mut(org_id) else {
            return name;
        };

        if seen.bucket_start_micros != bucket {
            seen.bucket_start_micros = bucket;
            seen.names.clear();
        }

        if seen.names.contains(&name) {
            return name;
        }

        if seen.names.len() < limit {
            seen.names.insert(name.clone());
            return name;
        }

        OTHER_SERVICES.to_string()
    }

    fn add(&mut self, key: RowKey, measure: Measure) {
        if self.pending.len() >= MAX_PENDING_ROWS && !self.pending.contains_key(&key) {
            self.dropped_bytes = self.dropped_bytes.saturating_add(measure.bytes);
            return;
        }

        let row = self.pending.entry(key).or_default();
        row.bytes = row.bytes.saturating_add(measure.bytes);
        row.events = row.events.saturating_add(measure.events);
    }
}

/// Spans are measured before anything has looked at the stream they name, and one row the table
/// cannot hold would fail the write of all the others, every time.
fn fits_a_row(receipt: &IntakeReceipt) -> bool {
    validate_stream_name(&receipt.stream).is_ok() && receipt.credential.id.len() <= 64
}

/// A name as it can be stored: without control characters, at most so long.
fn kept_service_name(service: &str) -> String {
    service
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_SERVICE_NAME_CHARS)
        .collect()
}

/// What this node has measured and not written yet.
pub struct IntakeUsageMeter {
    repository: Arc<dyn IntakeUsageRepository>,
    max_services_per_org: AtomicUsize,
    state: Mutex<MeterState>,
}

impl IntakeUsageMeter {
    pub fn new(repository: Arc<dyn IntakeUsageRepository>) -> Self {
        Self {
            repository,
            max_services_per_org: AtomicUsize::new(
                DEFAULT_INTAKE_USAGE_MAX_SERVICES_PER_ORG as usize,
            ),
            state: Mutex::new(MeterState::default()),
        }
    }

    pub fn set_max_services_per_org(&self, limit: usize) {
        self.max_services_per_org.store(limit, Ordering::Relaxed);
    }

    /// Counts what a request brought, of which `accepted` events were taken in.
    pub fn record(&self, org_id: &Id, receipt: IntakeReceipt, accepted: u64) {
        self.record_at(org_id, receipt, accepted, TimestampMicros::now());
    }

    fn record_at(&self, org_id: &Id, receipt: IntakeReceipt, accepted: u64, at: TimestampMicros) {
        let total = receipt.services.total();
        let accepted = accepted.min(total);

        if accepted == 0 || !fits_a_row(&receipt) {
            return;
        }

        let accepted_bytes =
            ((u128::from(receipt.bytes) * u128::from(accepted)) / u128::from(total)) as u64;

        let mut services = receipt.services.events.into_iter().collect::<Vec<_>>();
        services.sort();

        let counts = services.iter().map(|(_, count)| *count).collect::<Vec<_>>();
        let bytes = split_by_events(accepted_bytes, &counts);
        let events = split_by_events(accepted, &counts);
        let bucket = hour_bucket_start(at.0);
        let limit = self.max_services_per_org.load(Ordering::Relaxed);
        let mut state = self.state.lock();

        for (index, (service, _)) in services.iter().enumerate() {
            let service = state.counted_service(org_id.as_str(), bucket, service, limit);

            state.add(
                RowKey {
                    org_id: org_id.0.clone(),
                    bucket_start_micros: bucket,
                    credential: receipt.credential.clone(),
                    stream_type: receipt.stream_type,
                    stream: receipt.stream.clone(),
                    service,
                },
                Measure {
                    bytes: bytes[index],
                    events: events[index],
                },
            );
        }
    }

    /// Writes what was measured. Rows that could not be written are kept for the next time.
    pub async fn flush(&self) -> Result<usize> {
        let current_bucket = hour_bucket_start(TimestampMicros::now().0);

        let (pending, dropped_bytes) = {
            let mut state = self.state.lock();

            state
                .services
                .retain(|_, seen| seen.bucket_start_micros >= current_bucket);

            (
                std::mem::take(&mut state.pending),
                std::mem::take(&mut state.dropped_bytes),
            )
        };

        if dropped_bytes > 0 {
            tracing::warn!(dropped_bytes, "intake usage had no room for some rows");
        }

        // Every node writes in the same order, so that two writes of one hour cannot deadlock.
        let mut rows = pending
            .into_iter()
            .map(|(key, measure)| IntakeUsageRow {
                org_id: Id::from_string(key.org_id),
                bucket_start_micros: key.bucket_start_micros,
                credential: key.credential,
                stream_type: key.stream_type,
                stream: key.stream,
                service: key.service,
                intake_bytes: measure.bytes,
                events: measure.events,
            })
            .collect::<Vec<_>>();

        rows.sort_by(|left, right| row_order(left).cmp(&row_order(right)));

        let mut written = 0;

        for chunk in rows.chunks(WRITE_CHUNK_ROWS) {
            if let Err(error) = self.repository.add(chunk).await {
                self.keep(&rows[written..]);
                return Err(error);
            }

            written += chunk.len();
        }

        Ok(written)
    }

    fn keep(&self, rows: &[IntakeUsageRow]) {
        let mut state = self.state.lock();

        for row in rows {
            state.add(
                RowKey {
                    org_id: row.org_id.0.clone(),
                    bucket_start_micros: row.bucket_start_micros,
                    credential: row.credential.clone(),
                    stream_type: row.stream_type,
                    stream: row.stream.clone(),
                    service: row.service.clone(),
                },
                Measure {
                    bytes: row.intake_bytes,
                    events: row.events,
                },
            );
        }
    }
}

fn row_order(row: &IntakeUsageRow) -> (&str, i64, &IntakeCredential, &str, &str, &str) {
    (
        row.org_id.as_str(),
        row.bucket_start_micros,
        &row.credential,
        row.stream_type.as_str(),
        row.stream.as_str(),
        row.service.as_str(),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use async_trait::async_trait;
    use serde_json::json;

    use super::*;
    use crate::{
        domain::intake::usage::source::{
            IntakeUsageBucket, SourceUsage, UsageDimension, UsageSelection, stream_source_key,
        },
        shared::Error,
    };

    #[derive(Default)]
    struct Rows {
        written: Mutex<Vec<IntakeUsageRow>>,
        fail: AtomicBool,
    }

    impl Rows {
        fn total(&self, pick: impl Fn(&IntakeUsageRow) -> bool) -> (u64, u64) {
            self.written
                .lock()
                .iter()
                .filter(|row| pick(row))
                .fold((0, 0), |(bytes, events), row| {
                    (bytes + row.intake_bytes, events + row.events)
                })
        }
    }

    #[async_trait]
    impl IntakeUsageRepository for Rows {
        async fn add(&self, rows: &[IntakeUsageRow]) -> Result<()> {
            if self.fail.load(Ordering::SeqCst) {
                return Err(Error::unavailable("database is away"));
            }

            self.written.lock().extend_from_slice(rows);
            Ok(())
        }

        async fn by_source(
            &self,
            _: UsageDimension,
            _: UsageSelection<'_>,
        ) -> Result<Vec<SourceUsage>> {
            unreachable!()
        }

        async fn hourly(&self, _: UsageSelection<'_>) -> Result<Vec<IntakeUsageBucket>> {
            unreachable!()
        }

        async fn first_bucket(&self, _: &Id) -> Result<Option<i64>> {
            unreachable!()
        }

        async fn delete_before(&self, _: i64, _: i64) -> Result<u64> {
            unreachable!()
        }
    }

    fn org() -> Id {
        Id::from_string("org-1")
    }

    fn token(id: &str) -> IntakeCredential {
        IntakeCredential {
            kind: crate::domain::intake::usage::source::CredentialKind::ApiToken,
            id: id.into(),
        }
    }

    fn event(fields: Value) -> RawEvent {
        RawEvent {
            timestamp: TimestampMicros(0),
            fields: fields.as_object().unwrap().clone(),
        }
    }

    fn receipt(
        credential: &str,
        stream: &str,
        bytes: u64,
        services: ServiceTally,
    ) -> IntakeReceipt {
        IntakeReceipt {
            credential: token(credential),
            stream_type: StreamType::LOGS,
            stream: stream.into(),
            bytes,
            services,
        }
    }

    fn meter() -> (Arc<Rows>, IntakeUsageMeter) {
        let rows = Arc::new(Rows::default());
        let meter = IntakeUsageMeter::new(rows.clone());
        (rows, meter)
    }

    #[test]
    fn an_event_names_its_service_in_the_first_field_it_has() {
        let named = |fields: Value| service_of_fields(fields.as_object().unwrap()).to_string();

        assert_eq!(named(json!({"service.name": "a", "service": "b"})), "a");
        assert_eq!(named(json!({"service": "b", "service_name": "c"})), "b");
        assert_eq!(named(json!({"service_name": "c"})), "c");
        assert_eq!(named(json!({"service.name": 7, "service": "b"})), "b");
        assert_eq!(named(json!({"message": "x"})), UNNAMED_SERVICE);
    }

    #[test]
    fn shares_add_up_to_the_whole() {
        assert_eq!(split_by_events(10, &[1, 1, 1]), vec![3, 3, 4]);
        assert_eq!(split_by_events(100, &[1, 3]), vec![25, 75]);
        assert_eq!(split_by_events(5, &[0, 0]), vec![0, 0]);
        assert_eq!(
            split_by_events(u64::MAX, &[1, 1]).iter().sum::<u64>(),
            u64::MAX
        );
    }

    #[tokio::test]
    async fn the_views_of_a_request_agree() {
        let (rows, meter) = meter();

        let services = ServiceTally::of_events(&[
            event(json!({"service.name": "checkout"})),
            event(json!({"service.name": "checkout"})),
            event(json!({"service": "search"})),
            event(json!({"message": "named nothing"})),
        ]);

        meter.record(&org(), receipt("t-1", "app_logs", 400, services), 4);
        assert_eq!(meter.flush().await.unwrap(), 3);

        assert_eq!(rows.total(|_| true), (400, 4));
        assert_eq!(rows.total(|row| row.service == "checkout"), (200, 2));
        assert_eq!(rows.total(|row| row.service == "search"), (100, 1));
        assert_eq!(rows.total(|row| row.service == UNNAMED_SERVICE), (100, 1));
    }

    #[tokio::test]
    async fn only_what_was_accepted_counts() {
        let (rows, meter) = meter();

        meter.record(
            &org(),
            receipt("t-1", "app_logs", 100, ServiceTally::single("a", 4)),
            1,
        );
        meter.record(
            &org(),
            receipt("t-1", "app_logs", 100, ServiceTally::single("a", 4)),
            0,
        );
        meter.record(
            &org(),
            receipt("t-1", "app_logs", 100, ServiceTally::single("a", 0)),
            3,
        );
        meter.flush().await.unwrap();

        assert_eq!(rows.total(|_| true), (25, 1));
    }

    #[tokio::test]
    async fn requests_of_one_hour_share_a_row() {
        let (rows, meter) = meter();
        let at = TimestampMicros(1_000 * 3_600 * 1_000_000 + 5);

        for _ in 0..3 {
            meter.record_at(
                &org(),
                receipt("t-1", "a", 10, ServiceTally::single("s", 1)),
                1,
                at,
            );
        }

        meter.record_at(
            &org(),
            receipt("t-2", "a", 1, ServiceTally::single("s", 1)),
            1,
            at,
        );
        meter.flush().await.unwrap();

        let written = rows.written.lock();

        assert_eq!(written.len(), 2);
        assert_eq!(written[0].credential.id, "t-1");
        assert_eq!((written[0].intake_bytes, written[0].events), (30, 3));
        assert_eq!(written[0].bucket_start_micros, 1_000 * 3_600 * 1_000_000);
        assert_eq!(written[1].credential.id, "t-2");
    }

    #[tokio::test]
    async fn services_past_the_limit_share_one_row() {
        let (rows, meter) = meter();
        meter.set_max_services_per_org(2);
        let at = TimestampMicros(0);

        for service in ["a", "b", "c", "d", "a"] {
            meter.record_at(
                &org(),
                receipt("t", "s", 10, ServiceTally::single(service, 1)),
                1,
                at,
            );
        }

        // Naming no service costs no place among them.
        meter.record_at(
            &org(),
            receipt("t", "s", 10, ServiceTally::single(UNNAMED_SERVICE, 1)),
            1,
            at,
        );

        meter.flush().await.unwrap();

        assert_eq!(rows.total(|row| row.service == "a"), (20, 2));
        assert_eq!(rows.total(|row| row.service == "b"), (10, 1));
        assert_eq!(rows.total(|row| row.service == OTHER_SERVICES), (20, 2));
        assert_eq!(rows.total(|row| row.service == UNNAMED_SERVICE), (10, 1));
        assert_eq!(rows.written.lock().len(), 4);

        // Another hour starts counting again.
        let later = TimestampMicros(3_600 * 1_000_000);
        meter.record_at(
            &org(),
            receipt("t", "s", 10, ServiceTally::single("c", 1)),
            1,
            later,
        );
        meter.flush().await.unwrap();

        assert_eq!(rows.total(|row| row.service == "c"), (10, 1));
    }

    #[tokio::test]
    async fn a_service_name_is_kept_as_it_can_be_stored() {
        let (rows, meter) = meter();
        let long = "x".repeat(MAX_SERVICE_NAME_CHARS + 10);

        meter.record(
            &org(),
            receipt("t", "s", 1, ServiceTally::single(&long, 1)),
            1,
        );
        meter.record(
            &org(),
            receipt("t", "s", 1, ServiceTally::single("a\0b\n", 1)),
            1,
        );
        meter.flush().await.unwrap();

        let written = rows.written.lock();
        let names = written
            .iter()
            .map(|row| row.service.clone())
            .collect::<Vec<_>>();

        assert!(names.contains(&"ab".to_string()));
        assert!(names.contains(&"x".repeat(MAX_SERVICE_NAME_CHARS)));
    }

    #[tokio::test]
    async fn a_stream_the_table_cannot_hold_is_not_counted_and_blocks_nothing() {
        let (rows, meter) = meter();
        let long = "x".repeat(300);

        meter.record(
            &org(),
            receipt("t", &long, 1, ServiceTally::single("a", 1)),
            1,
        );
        meter.record(
            &org(),
            receipt("t", "a\0b", 1, ServiceTally::single("a", 1)),
            1,
        );
        meter.record(
            &org(),
            receipt("t", "fine", 5, ServiceTally::single("a", 1)),
            1,
        );
        meter.flush().await.unwrap();

        let written = rows.written.lock();

        assert_eq!(written.len(), 1);
        assert_eq!(written[0].stream, "fine");
    }

    #[tokio::test]
    async fn rows_a_failed_write_left_behind_go_out_next_time() {
        let (rows, meter) = meter();

        meter.record(
            &org(),
            receipt("t", "s", 7, ServiceTally::single("a", 1)),
            1,
        );
        rows.fail.store(true, Ordering::SeqCst);
        assert!(meter.flush().await.is_err());

        meter.record(
            &org(),
            receipt("t", "s", 3, ServiceTally::single("a", 1)),
            1,
        );
        rows.fail.store(false, Ordering::SeqCst);
        meter.flush().await.unwrap();

        assert_eq!(rows.total(|_| true), (10, 2));
        assert_eq!(meter.flush().await.unwrap(), 0);
    }

    #[test]
    fn a_stream_is_known_by_its_type_and_name() {
        assert_eq!(
            stream_source_key(StreamType::TRACES, "spans"),
            "traces:spans"
        );
    }
}
