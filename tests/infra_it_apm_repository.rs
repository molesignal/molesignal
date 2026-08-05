// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PostgreSQL APM repository idempotency, rollup and tenant-isolation coverage.

mod common;

use std::sync::Arc;

use molesignal::{
    domain::apm::{
        APM_PERSISTENCE_SCHEMA_VERSION, ApmMaintenanceRepository, ApmQueryRepository,
        ApmWriteRepository, BucketDimension, BucketKind, BucketMeasurements, BucketQuery,
        HistogramSchema, LatencyHistogram, OwnerSnapshot, QueryResolution, RollupRequest,
        ServiceIdentity, ServiceObservation,
    },
    infra::apm::PgApmRepository,
    shared::{
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

const MINUTE_MICROS: i64 = 60 * 1_000_000;
const HOUR_MICROS: i64 = 60 * MINUTE_MICROS;

#[tokio::test]
async fn apm_snapshots_rollups_retention_and_tenant_isolation_are_idempotent() {
    if common::skip_unless_enabled() {
        return;
    }
    let server = common::TestServer::start().await;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(&server.settings.store.meta.dsn)
        .await
        .expect("APM integration pool");
    let repository = Arc::new(PgApmRepository::new(pool.clone()));
    let other_org = Id::new();
    sqlx::query(
        "INSERT INTO organizations (id, name, slug, created_at_micros)
         VALUES ($1, 'APM other', $2, $3)",
    )
    .bind(other_org.as_str())
    .bind(format!("apm-other-{}", other_org.as_str()))
    .bind(TimestampMicros::now().0)
    .execute(&pool)
    .await
    .expect("create second organization");

    let service = ServiceIdentity::new(Some("shop"), Some("checkout"), Some("prod"), None);
    upsert_service(&repository, &server.root_org_id, &service).await;
    upsert_service(&repository, &other_org, &service).await;

    let now = TimestampMicros::now().0;
    let hour = now.div_euclid(HOUR_MICROS) * HOUR_MICROS - HOUR_MICROS;
    repository
        .ensure_projection_started(&server.root_org_id, TimestampMicros(hour))
        .await
        .expect("projection marker");

    let first = snapshot(&server.root_org_id, &service, "owner-a", 1, hour, &[10_000]);
    let applied = repository
        .replace_owner_snapshots(std::slice::from_ref(&first))
        .await
        .expect("first snapshot");
    assert_eq!(applied.applied, 1);
    let retry = repository
        .replace_owner_snapshots(std::slice::from_ref(&first))
        .await
        .expect("ambiguous retry");
    assert_eq!(retry.applied, 0);
    assert_eq!(retry.stale, 1);

    let newer = snapshot(
        &server.root_org_id,
        &service,
        "owner-a",
        2,
        hour,
        &[10_000, 20_000],
    );
    assert_eq!(
        repository
            .replace_owner_snapshots(std::slice::from_ref(&newer))
            .await
            .unwrap()
            .applied,
        1
    );

    let owner_b = snapshot(&server.root_org_id, &service, "owner-b", 1, hour, &[30_000]);
    let owner_c = snapshot(&server.root_org_id, &service, "owner-c", 1, hour, &[40_000]);
    let (left, right) = tokio::join!(
        repository.replace_owner_snapshots(std::slice::from_ref(&owner_b)),
        repository.replace_owner_snapshots(std::slice::from_ref(&owner_c))
    );
    assert_eq!(left.unwrap().applied, 1);
    assert_eq!(right.unwrap().applied, 1);

    let foreign = snapshot(&other_org, &service, "owner-a", 1, hour, &[1_000; 9]);
    repository
        .replace_owner_snapshots(&[foreign])
        .await
        .expect("other org snapshot");

    let org_query = query(&server.root_org_id, hour, hour + HOUR_MICROS - 1);
    let rows = repository.query_buckets(&org_query).await.unwrap();
    assert_eq!(request_count(&rows), 4);
    let other_rows = repository
        .query_buckets(&query(&other_org, hour, hour + HOUR_MICROS - 1))
        .await
        .unwrap();
    assert_eq!(request_count(&other_rows), 9);

    repository
        .replace_owner_snapshots(&[snapshot(
            &server.root_org_id,
            &service,
            "owner-a",
            1,
            hour + MINUTE_MICROS,
            &[50_000, 60_000, 70_000],
        )])
        .await
        .unwrap();
    let before = repository.query_buckets(&org_query).await.unwrap();
    assert_eq!(request_count(&before), 7);

    let rollup = RollupRequest {
        org_id: server.root_org_id.clone(),
        hour_at: TimestampMicros(hour),
        hot_retention_cutoff: TimestampMicros(hour - 48 * HOUR_MICROS),
        rollup_retention_cutoff: TimestampMicros(hour - 30 * 24 * HOUR_MICROS),
    };
    let first_rollup = repository.rollup_and_retain(&rollup).await.unwrap();
    assert_eq!(first_rollup.source_rows, 4);
    assert_eq!(first_rollup.rollup_rows, 1);

    let mut hourly_query = org_query.clone();
    hourly_query.resolution = QueryResolution::Hour;
    let hourly = repository.query_buckets(&hourly_query).await.unwrap();
    assert_eq!(request_count(&hourly), 7);
    assert_eq!(
        hourly[0].measurements.latency.count(),
        before
            .iter()
            .map(|row| row.measurements.latency.count())
            .sum::<u64>()
    );

    let retry_rollup = repository.rollup_and_retain(&rollup).await.unwrap();
    assert_eq!(retry_rollup.source_rows, 0);
    assert_eq!(
        request_count(&repository.query_buckets(&hourly_query).await.unwrap()),
        7
    );

    let old_hour = hour - 40 * 24 * HOUR_MICROS;
    repository
        .replace_owner_snapshots(&[snapshot(
            &server.root_org_id,
            &service,
            "owner-a",
            1,
            old_hour,
            &[5_000],
        )])
        .await
        .unwrap();
    repository
        .rollup_and_retain(&RollupRequest {
            org_id: server.root_org_id.clone(),
            hour_at: TimestampMicros(old_hour),
            hot_retention_cutoff: TimestampMicros(hour - 48 * HOUR_MICROS),
            rollup_retention_cutoff: TimestampMicros(hour - 30 * 24 * HOUR_MICROS),
        })
        .await
        .unwrap();
    let old_hourly = repository
        .query_buckets(&BucketQuery {
            resolution: QueryResolution::Hour,
            ..query(&server.root_org_id, old_hour, old_hour + HOUR_MICROS - 1)
        })
        .await
        .unwrap();
    assert!(
        old_hourly.is_empty(),
        "expired hourly rollup must be removed"
    );

    sqlx::query("DELETE FROM organizations WHERE id = $1")
        .bind(other_org.as_str())
        .execute(&pool)
        .await
        .expect("delete other organization");
    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM apm_services WHERE org_id = $1")
        .bind(other_org.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(remaining, 0);
    assert!(
        repository
            .query_buckets(&query(&other_org, hour, hour + HOUR_MICROS - 1))
            .await
            .unwrap()
            .is_empty()
    );
}

async fn upsert_service(repository: &PgApmRepository, org_id: &Id, service: &ServiceIdentity) {
    repository
        .upsert_catalog(
            &[ServiceObservation {
                org_id: org_id.clone(),
                service: service.clone(),
                first_seen_at: TimestampMicros(1),
                last_seen_at: TimestampMicros(2),
                runtime_language: Some("rust".into()),
                telemetry_sdk_name: Some("opentelemetry".into()),
                telemetry_sdk_version: Some("1".into()),
                recent_instance_count: 1,
            }],
            &[],
        )
        .await
        .expect("upsert service");
}

fn snapshot(
    org_id: &Id,
    service: &ServiceIdentity,
    owner_id: &str,
    snapshot_seq: u64,
    bucket_at: i64,
    durations: &[u64],
) -> OwnerSnapshot {
    let schema = HistogramSchema::v1();
    let mut latency = LatencyHistogram::empty(&schema);
    for duration in durations {
        latency.observe(&schema, *duration).unwrap();
    }
    OwnerSnapshot {
        schema_version: APM_PERSISTENCE_SCHEMA_VERSION,
        org_id: org_id.clone(),
        owner_id: owner_id.into(),
        bucket_at: TimestampMicros(bucket_at),
        snapshot_seq,
        dimension: BucketDimension::Service {
            service: service.clone(),
            version: Some("2.5.0".into()),
        },
        measurements: BucketMeasurements {
            request_count: durations.len() as u64,
            error_count: 0,
            overflow_count: 0,
            latency,
            exemplars: Vec::new(),
        },
        updated_at: TimestampMicros(bucket_at + 1),
    }
}

fn query(org_id: &Id, start: i64, end: i64) -> BucketQuery {
    BucketQuery {
        org_id: org_id.clone(),
        range: TimeRange::new(TimestampMicros(start), TimestampMicros(end)),
        kind: BucketKind::Service,
        resolution: QueryResolution::Minute,
        namespace: None,
        service_name: None,
        environment: None,
        version: None,
    }
}

fn request_count(rows: &[molesignal::domain::apm::MergedBucket]) -> u64 {
    rows.iter().map(|row| row.measurements.request_count).sum()
}
