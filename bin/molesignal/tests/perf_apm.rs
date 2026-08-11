// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Release-gate APM projection/PostgreSQL/query/rollup benchmark.
//!
//! Run against a disposable PostgreSQL database:
//! `APM_BENCH_DATABASE_URL=postgres://... cargo test --release --test perf_apm -- --nocapture`

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use molesignal::{
    app::apm::{
        ApmAggregator, ApmCandidateProjector, ApmCardinalityLimiter, ApmProjectorConfig,
        ApmQueryConfig, ApmQueryRequest, ApmQueryService, BufferedApmProjector,
    },
    config::{ApmSettings, MetaStoreSettings},
    domain::apm::{
        APM_FACT_SCHEMA_VERSION, APM_PERSISTENCE_SCHEMA_VERSION, ApmMaintenanceRepository,
        ApmOutcome, ApmQueryRepository, ApmSpanFact, ApmSpanKind, ApmWriteRepository,
        BucketDimension, BucketKind, BucketMeasurements, BucketQuery, DependencyCategory,
        DependencyIdentity, ErrorGroupRecord, ErrorIdentity, ErrorSample, HistogramSchema,
        InstrumentationMetadata, LatencyHistogram, OwnerSnapshot, ProjectionGap, ProjectionState,
        QueryResolution, RollupRequest, ServiceIdentity, ServiceObservation, SnapshotWriteStats,
        TransactionIdentity, TransactionKind, VersionObservation,
    },
    infra::{apm::PgApmRepository, persistence::MetaStore},
    shared::{
        Result,
        ids::Id,
        tail_sampling::CandidateDisposition,
        time::{TimeRange, TimestampMicros},
    },
};
use serde_json::json;
use sqlx::PgPool;

const SERVICE_COUNT: usize = 50;
const MINUTE_COUNT: usize = 50;
const OWNER_COUNT: usize = 4;
const SNAPSHOT_COUNT: usize = SERVICE_COUNT * MINUTE_COUNT * OWNER_COUNT;
const FLUSH_SAMPLE_COUNT: usize = 20;
const QUERY_SAMPLE_COUNT: usize = 20;
const ACTIVE_DIMENSION_PERCENT: u64 = 5;
const BUCKET_KINDS: [BucketKind; 4] = [
    BucketKind::Service,
    BucketKind::Transaction,
    BucketKind::Dependency,
    BucketKind::Error,
];

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn apm_capacity_gate() {
    let Ok(dsn) = std::env::var("APM_BENCH_DATABASE_URL") else {
        eprintln!("APM benchmark skipped: APM_BENCH_DATABASE_URL is not set");
        return;
    };
    let settings = ApmSettings::default();
    let projection = measure_projection(&settings).await;

    let store = MetaStore::connect(&MetaStoreSettings {
        backend: "postgres".into(),
        dsn,
        min_connections: 2,
        max_connections: 16,
    })
    .await
    .expect("connect and migrate benchmark PostgreSQL");
    let pool = store.pool;
    let org_id = Id::from_string("apm-capacity-benchmark");
    sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(org_id.as_str())
        .execute(&pool)
        .await
        .expect("clear prior benchmark tenant");
    sqlx::query(
        "TRUNCATE TABLE
            apm_service_buckets_default,
            apm_transaction_buckets_default,
            apm_dependency_buckets_default,
            apm_error_buckets_default,
            apm_service_buckets_hourly_default,
            apm_transaction_buckets_hourly_default,
            apm_dependency_buckets_hourly_default,
            apm_error_buckets_hourly_default",
    )
    .execute(&pool)
    .await
    .expect("reset disposable benchmark bucket partitions");
    let mut relation_baselines = [0_i64; BUCKET_KINDS.len()];
    for (index, kind) in BUCKET_KINDS.into_iter().enumerate() {
        relation_baselines[index] = relation_size(&pool, bucket_relation(kind)).await;
    }
    sqlx::query(
        "INSERT INTO organizations (id, name, slug, created_at_micros)
         VALUES ($1, 'APM capacity benchmark', 'apm-capacity-benchmark', $2)",
    )
    .bind(org_id.as_str())
    .bind(TimestampMicros::now().0)
    .execute(&pool)
    .await
    .expect("create benchmark tenant");

    let repository = Arc::new(PgApmRepository::new(pool.clone()));
    let hour = TimestampMicros::now().0.div_euclid(3_600_000_000) * 3_600_000_000 - 3_600_000_000;
    seed_catalog(repository.as_ref(), &org_id, hour).await;
    repository
        .ensure_projection_started(&org_id, TimestampMicros(hour))
        .await
        .expect("projection start");

    let mut snapshots = snapshots(&org_id, hour);
    assert_eq!(snapshots.len(), SNAPSHOT_COUNT);
    let mut flush_samples = Vec::new();
    let mut relation_bytes_after_insert: Option<i64> = None;
    for sequence in 1..=FLUSH_SAMPLE_COUNT as u64 {
        for snapshot in &mut snapshots {
            snapshot.snapshot_seq = sequence;
        }
        let started = Instant::now();
        let stats = repository
            .replace_owner_snapshots(&snapshots)
            .await
            .expect("snapshot flush");
        flush_samples.push(started.elapsed());
        assert_eq!(stats.applied, SNAPSHOT_COUNT as u64);
        if sequence == 1 {
            let inserted_rows: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM apm_service_buckets WHERE org_id = $1")
                    .bind(org_id.as_str())
                    .fetch_one(&pool)
                    .await
                    .expect("initial hot row count");
            assert_eq!(inserted_rows, SNAPSHOT_COUNT as i64);
            relation_bytes_after_insert = Some(
                sqlx::query_scalar(
                    "SELECT pg_total_relation_size(
                        'apm_service_buckets_default'::regclass
                     )",
                )
                .fetch_one(&pool)
                .await
                .expect("initial hot relation size"),
            );
        }
    }
    let retry = repository
        .replace_owner_snapshots(&snapshots)
        .await
        .expect("ambiguous retry");
    assert_eq!(retry.applied, 0);
    assert_eq!(retry.stale, SNAPSHOT_COUNT as u64);

    let hot_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM apm_service_buckets WHERE org_id = $1")
            .bind(org_id.as_str())
            .fetch_one(&pool)
            .await
            .expect("hot row count");
    assert_eq!(hot_rows, SNAPSHOT_COUNT as i64);
    let relation_bytes_after_updates: i64 = sqlx::query_scalar(
        "SELECT pg_total_relation_size('apm_service_buckets_default'::regclass)",
    )
    .fetch_one(&pool)
    .await
    .expect("updated hot relation size");
    let relation_bytes_after_insert = relation_bytes_after_insert
        .expect("initial relation size captured")
        .saturating_sub(relation_baselines[0]);
    let relation_bytes_after_updates =
        relation_bytes_after_updates.saturating_sub(relation_baselines[0]);

    let query_service = ApmQueryService::new(
        repository.clone() as Arc<dyn ApmQueryRepository>,
        ApmQueryConfig::from_settings(&settings),
    );
    let context = query_service
        .context(
            org_id.clone(),
            ApmQueryRequest {
                from: hour,
                to: hour + 3_599_999_999,
                resolution: QueryResolution::Minute,
                ..ApmQueryRequest::default()
            },
            &["request_count"],
            "request_count",
        )
        .expect("benchmark query context");
    let overview = query_service
        .overview(&context)
        .await
        .expect("warm overview query");
    assert_eq!(overview.red.request_count, SNAPSHOT_COUNT as u64 * 100);
    let mut query_samples = Vec::new();
    for _ in 0..QUERY_SAMPLE_COUNT {
        let started = Instant::now();
        query_service
            .overview(&context)
            .await
            .expect("overview query");
        query_samples.push(started.elapsed());
    }

    let mut relation_bytes_by_kind = [0_i64; BUCKET_KINDS.len()];
    relation_bytes_by_kind[0] = relation_bytes_after_insert;
    for (index, kind) in BUCKET_KINDS.into_iter().enumerate().skip(1) {
        let storage_rows = snapshots_for_kind(&snapshots, kind);
        let stats = repository
            .replace_owner_snapshots(&storage_rows)
            .await
            .expect("representative storage snapshot flush");
        assert_eq!(stats.applied, SNAPSHOT_COUNT as u64);
        relation_bytes_by_kind[index] = relation_size(&pool, bucket_relation(kind))
            .await
            .saturating_sub(relation_baselines[index]);
    }
    let mut modeled_hot_rows = 0_u64;
    let mut modeled_rollup_rows = 0_u64;
    let mut projected_bytes = 0_f64;
    let storage_by_kind = BUCKET_KINDS
        .into_iter()
        .enumerate()
        .map(|(index, kind)| {
            let (hot_rows, rollup_rows) = modeled_rows(&settings, kind);
            let bytes_per_row = relation_bytes_by_kind[index] as f64 / SNAPSHOT_COUNT as f64;
            let kind_projected_bytes = bytes_per_row * (hot_rows + rollup_rows) as f64;
            modeled_hot_rows = modeled_hot_rows.saturating_add(hot_rows);
            modeled_rollup_rows = modeled_rollup_rows.saturating_add(rollup_rows);
            projected_bytes += kind_projected_bytes;
            json!({
                "kind": kind,
                "sample_rows": SNAPSHOT_COUNT,
                "relation_bytes_after_insert": relation_bytes_by_kind[index],
                "bytes_per_row": bytes_per_row,
                "modeled_hot_rows": hot_rows,
                "modeled_rollup_rows": rollup_rows,
                "projected_gib": kind_projected_bytes / 1024_f64.powi(3),
            })
        })
        .collect::<Vec<_>>();
    let projected_gib = projected_bytes / 1024_f64.powi(3);

    let rollup_started = Instant::now();
    let rollup = repository
        .rollup_and_retain(&RollupRequest {
            org_id: org_id.clone(),
            hour_at: TimestampMicros(hour),
            hot_retention_cutoff: TimestampMicros(
                hour - i64::from(settings.hot_retention_hours) * 3_600_000_000,
            ),
            rollup_retention_cutoff: TimestampMicros(
                hour - i64::from(settings.rollup_retention_days) * 24 * 3_600_000_000,
            ),
        })
        .await
        .expect("hour rollup");
    let rollup_elapsed = rollup_started.elapsed();
    assert_eq!(
        rollup.source_rows,
        SNAPSHOT_COUNT as u64 * BUCKET_KINDS.len() as u64
    );
    assert_eq!(rollup.rollup_rows, 3_250);
    let hourly = repository
        .query_buckets(&BucketQuery {
            org_id: org_id.clone(),
            range: TimeRange::new(TimestampMicros(hour), TimestampMicros(hour + 3_599_999_999)),
            kind: BucketKind::Service,
            resolution: QueryResolution::Hour,
            namespace: None,
            service_name: None,
            environment: None,
            version: None,
        })
        .await
        .expect("hourly query");
    assert_eq!(
        hourly
            .iter()
            .map(|bucket| bucket.measurements.request_count)
            .sum::<u64>(),
        SNAPSHOT_COUNT as u64 * 100,
    );
    let retry_rollup = repository
        .rollup_and_retain(&RollupRequest {
            org_id: org_id.clone(),
            hour_at: TimestampMicros(hour),
            hot_retention_cutoff: TimestampMicros(
                hour - i64::from(settings.hot_retention_hours) * 3_600_000_000,
            ),
            rollup_retention_cutoff: TimestampMicros(
                hour - i64::from(settings.rollup_retention_days) * 24 * 3_600_000_000,
            ),
        })
        .await
        .expect("idempotent rollup retry");
    assert_eq!(retry_rollup.source_rows, 0);

    let flush_p95 = percentile(&mut flush_samples, 0.95);
    let query_p95 = percentile(&mut query_samples, 0.95);
    println!(
        "APM_BENCH_RESULT={}",
        serde_json::to_string_pretty(&json!({
            "postgres_version": sqlx::query_scalar::<String>("SHOW server_version")
                .fetch_one(&pool)
                .await
                .expect("PostgreSQL version"),
            "projection": projection,
            "snapshot_flush": {
                "rows": SNAPSHOT_COUNT,
                "samples": flush_samples.len(),
                "p95_ms": millis(flush_p95),
            },
            "overview_query": {
                "source_owner_rows": SNAPSHOT_COUNT,
                "merged_minute_rows": SERVICE_COUNT * MINUTE_COUNT,
                "samples": query_samples.len(),
                "p95_ms": millis(query_p95),
            },
            "storage": {
                "service_hot_rows": hot_rows,
                "service_relation_bytes_after_insert": relation_bytes_after_insert,
                "service_relation_bytes_after_flush_samples": relation_bytes_after_updates,
                "service_update_growth_ratio":
                    relation_bytes_after_updates as f64 / relation_bytes_after_insert as f64,
                "modeled_hot_retention_hours": settings.hot_retention_hours,
                "modeled_rollup_retention_days": settings.rollup_retention_days,
                "modeled_hot_rows": modeled_hot_rows,
                "modeled_rollup_rows": modeled_rollup_rows,
                "projected_heavy_org_gib": projected_gib,
                "by_kind": storage_by_kind,
            },
            "rollup": {
                "source_rows": rollup.source_rows,
                "hourly_rows": rollup.rollup_rows,
                "elapsed_ms": millis(rollup_elapsed),
                "retry_source_rows": retry_rollup.source_rows,
            },
        }))
        .expect("benchmark JSON"),
    );

    assert!(projection.aggregate_p99_micros <= 25.0);
    assert!(projection.enqueue_p99_micros <= 5.0);
    assert!(projection.queue_drop_rate < 0.001);
    assert!(flush_p95 <= Duration::from_millis(500));
    assert!(query_p95 <= Duration::from_millis(500));
    assert!(modeled_hot_rows <= 11_000_000);
    assert!(projected_gib <= 16.0);
    assert!(rollup_elapsed <= Duration::from_secs(60));
}

#[derive(serde::Serialize)]
struct ProjectionResult {
    aggregate_samples: usize,
    aggregate_p99_micros: f64,
    enqueue_samples: usize,
    enqueue_p99_micros: f64,
    queue_drop_rate: f64,
    service_rejections: u64,
}

async fn measure_projection(settings: &ApmSettings) -> ProjectionResult {
    let org_id = Id::from_string("apm-projection-benchmark");
    let base = fact(&org_id, "service-000", "span-0");
    let mut aggregator =
        ApmAggregator::new("owner-0".into(), HistogramSchema::v1(), 3, 8).expect("aggregator");
    let mut aggregate_samples = Vec::with_capacity(100_000);
    for index in 0..100_000 {
        let mut item = base.clone();
        item.span_id = format!("span-{index}");
        let started = Instant::now();
        aggregator.observe(item, false).expect("aggregate fact");
        aggregate_samples.push(started.elapsed());
    }

    let mut limiter = ApmCardinalityLimiter::new(settings.cardinality.clone());
    let mut service_rejections = 0_u64;
    for index in 0..=settings.cardinality.services_per_org_hour {
        let mut item = fact(&org_id, &format!("bounded-service-{index}"), "span");
        if !limiter.admit(&mut item).accepted {
            service_rejections += 1;
        }
    }
    assert_eq!(service_rejections, 1);

    let repository = Arc::new(NullWriter);
    let mut config = ApmProjectorConfig::from_settings(settings);
    config.flush_interval = Duration::from_secs(3_600);
    config.shutdown_timeout = Duration::from_secs(30);
    let projector =
        BufferedApmProjector::start("owner-queue".into(), repository, config).expect("projector");
    let mut enqueue_samples = Vec::with_capacity(50_000);
    for index in 0..50_000 {
        let mut item = base.clone();
        item.span_id = format!("queue-{index}");
        let started = Instant::now();
        projector.project(item, CandidateDisposition::Accepted);
        enqueue_samples.push(started.elapsed());
    }
    projector.shutdown().await;
    let health = projector.health();
    let total = health.accepted_facts + health.queue_drops;
    let queue_drop_rate = if total == 0 {
        0.0
    } else {
        health.queue_drops as f64 / total as f64
    };

    ProjectionResult {
        aggregate_samples: aggregate_samples.len(),
        aggregate_p99_micros: micros(percentile(&mut aggregate_samples, 0.99)),
        enqueue_samples: enqueue_samples.len(),
        enqueue_p99_micros: micros(percentile(&mut enqueue_samples, 0.99)),
        queue_drop_rate,
        service_rejections,
    }
}

fn fact(org_id: &Id, service: &str, span_id: &str) -> ApmSpanFact {
    ApmSpanFact {
        schema_version: APM_FACT_SCHEMA_VERSION,
        org_id: org_id.clone(),
        service: ServiceIdentity::new(Some("bench"), Some(service), Some("prod"), None),
        service_version: Some("2.0.0".into()),
        service_instance_id: Some("instance-0".into()),
        instrumentation: InstrumentationMetadata::default(),
        trace_id: "0123456789abcdef0123456789abcdef".into(),
        span_id: span_id.into(),
        parent_span_id: None,
        event_time: TimestampMicros::now(),
        duration_micros: 125_000,
        span_kind: ApmSpanKind::Server,
        outcome: ApmOutcome::Success,
        transaction: Some(TransactionIdentity {
            name: "GET /orders/{id}".into(),
            kind: TransactionKind::Http,
        }),
        dependency: None,
        error: None,
        exception: None,
    }
}

fn snapshots(org_id: &Id, hour: i64) -> Vec<OwnerSnapshot> {
    let schema = HistogramSchema::v1();
    let mut latency = LatencyHistogram::empty(&schema);
    for _ in 0..100 {
        latency.observe(&schema, 125_000).expect("histogram");
    }
    let measurements = BucketMeasurements {
        request_count: 100,
        error_count: 2,
        overflow_count: 0,
        latency,
        exemplars: Vec::new(),
    };
    let mut rows = Vec::with_capacity(SNAPSHOT_COUNT);
    for service_index in 0..SERVICE_COUNT {
        let service = ServiceIdentity::new(
            Some("bench"),
            Some(&format!("service-{service_index:03}")),
            Some("prod"),
            None,
        );
        for minute in 0..MINUTE_COUNT {
            for owner in 0..OWNER_COUNT {
                rows.push(OwnerSnapshot {
                    schema_version: APM_PERSISTENCE_SCHEMA_VERSION,
                    org_id: org_id.clone(),
                    owner_id: format!("owner-{owner}"),
                    bucket_at: TimestampMicros(hour + minute as i64 * 60_000_000),
                    snapshot_seq: 1,
                    dimension: BucketDimension::Service {
                        service: service.clone(),
                        version: Some("2.0.0".into()),
                    },
                    measurements: measurements.clone(),
                    updated_at: TimestampMicros(hour + minute as i64 * 60_000_000 + 1),
                });
            }
        }
    }
    rows
}

fn snapshots_for_kind(base: &[OwnerSnapshot], kind: BucketKind) -> Vec<OwnerSnapshot> {
    base.iter()
        .enumerate()
        .map(|(index, snapshot)| {
            let service = snapshot.dimension.service().clone();
            let version = snapshot.dimension.version().map(str::to_owned);
            let minute_slot = (index / OWNER_COUNT) % MINUTE_COUNT;
            let dimension = match kind {
                BucketKind::Service => BucketDimension::Service { service, version },
                BucketKind::Transaction => BucketDimension::Transaction {
                    service,
                    version,
                    transaction: TransactionIdentity {
                        name: format!("GET /orders/type-{:02}", minute_slot % 32),
                        kind: TransactionKind::Http,
                    },
                },
                BucketKind::Dependency => BucketDimension::Dependency {
                    service,
                    version,
                    dependency: DependencyIdentity {
                        category: DependencyCategory::Database,
                        target: format!("postgres-cluster-{:02}", minute_slot % 16),
                        operation: Some("SELECT orders".into()),
                    },
                },
                BucketKind::Error => BucketDimension::Error {
                    service,
                    version,
                    error: ErrorIdentity {
                        fingerprint: format!("{:064x}", minute_slot % 16),
                        error_type: "CheckoutFailure".into(),
                        application_frame: Some("checkout::handler".into()),
                        transaction_name: Some("GET /orders/{type}".into()),
                        overflow: false,
                    },
                },
            };
            OwnerSnapshot {
                snapshot_seq: 1,
                dimension,
                ..snapshot.clone()
            }
        })
        .collect()
}

fn modeled_rows(settings: &ApmSettings, kind: BucketKind) -> (u64, u64) {
    let services = u64::try_from(settings.cardinality.services_per_org_hour)
        .expect("service cardinality fits u64");
    let dimensions_per_service = u64::try_from(match kind {
        BucketKind::Service => 1,
        BucketKind::Transaction => settings.cardinality.transactions_per_service_hour,
        BucketKind::Dependency => settings.cardinality.dependencies_per_service_hour,
        BucketKind::Error => settings.cardinality.error_groups_per_service_hour,
    })
    .expect("dimension cardinality fits u64");
    let active_dimensions_per_minute = match kind {
        BucketKind::Service => services,
        _ => {
            services
                .saturating_mul(dimensions_per_service)
                .saturating_mul(ACTIVE_DIMENSION_PERCENT)
                / 100
        }
    };
    let hot_rows = active_dimensions_per_minute
        .saturating_mul(OWNER_COUNT as u64)
        .saturating_mul(60)
        .saturating_mul(u64::from(settings.hot_retention_hours));
    let rollup_rows = services
        .saturating_mul(dimensions_per_service)
        .saturating_mul(24)
        .saturating_mul(u64::from(settings.rollup_retention_days));
    (hot_rows, rollup_rows)
}

fn bucket_relation(kind: BucketKind) -> &'static str {
    match kind {
        BucketKind::Service => "apm_service_buckets_default",
        BucketKind::Transaction => "apm_transaction_buckets_default",
        BucketKind::Dependency => "apm_dependency_buckets_default",
        BucketKind::Error => "apm_error_buckets_default",
    }
}

async fn relation_size(pool: &PgPool, relation: &str) -> i64 {
    sqlx::query_scalar("SELECT pg_total_relation_size($1::regclass)")
        .bind(relation)
        .fetch_one(pool)
        .await
        .expect("measure APM bucket relation")
}

async fn seed_catalog(repository: &PgApmRepository, org_id: &Id, hour: i64) {
    let services = (0..SERVICE_COUNT)
        .map(|index| ServiceObservation {
            org_id: org_id.clone(),
            service: ServiceIdentity::new(
                Some("bench"),
                Some(&format!("service-{index:03}")),
                Some("prod"),
                None,
            ),
            first_seen_at: TimestampMicros(hour),
            last_seen_at: TimestampMicros(hour + 3_599_999_999),
            runtime_language: Some("rust".into()),
            telemetry_sdk_name: Some("opentelemetry".into()),
            telemetry_sdk_version: Some("0.29".into()),
            recent_instance_count: 4,
        })
        .collect::<Vec<_>>();
    let versions = services
        .iter()
        .map(|service| VersionObservation {
            org_id: org_id.clone(),
            service: service.service.clone(),
            version: "2.0.0".into(),
            first_seen_at: service.first_seen_at,
            last_seen_at: service.last_seen_at,
            observation_count: 200_000,
        })
        .collect::<Vec<_>>();
    repository
        .upsert_catalog(&services, &versions)
        .await
        .expect("seed service catalog");
}

fn percentile(samples: &mut [Duration], quantile: f64) -> Duration {
    samples.sort_unstable();
    let rank = (samples.len() as f64 * quantile).ceil().max(1.0) as usize;
    samples[rank - 1]
}

fn micros(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000_000.0
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

struct NullWriter;

#[async_trait]
impl ApmWriteRepository for NullWriter {
    async fn upsert_catalog(
        &self,
        _services: &[ServiceObservation],
        _versions: &[VersionObservation],
    ) -> Result<()> {
        Ok(())
    }

    async fn replace_owner_snapshots(
        &self,
        snapshots: &[OwnerSnapshot],
    ) -> Result<SnapshotWriteStats> {
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

    async fn record_projection_gaps(&self, _gaps: &[ProjectionGap]) -> Result<()> {
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
