// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use parking_lot::Mutex;

use super::*;
use crate::{
    domain::apm::{
        ApmQueryRepository, BucketDimension, BucketMeasurements, BucketQuery, CatalogQuery,
        ErrorGroupQuery, ErrorGroupRecord, ErrorSample, LatencyHistogram, MergedBucket,
        ProjectionGap, ProjectionGapReason, ProjectionState, QueryResolution, ServiceIdentity,
        ServiceObservation, TransactionIdentity, TransactionKind, VersionObservation,
    },
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

struct FixtureRepository {
    org_id: Id,
    services: Vec<ServiceObservation>,
    buckets: Vec<MergedBucket>,
    state: Option<ProjectionState>,
    gaps: Vec<ProjectionGap>,
    seen_bucket_queries: Mutex<Vec<BucketQuery>>,
    seen_catalog_queries: Mutex<Vec<CatalogQuery>>,
}

#[async_trait]
impl ApmQueryRepository for FixtureRepository {
    async fn query_buckets(&self, query: &BucketQuery) -> Result<Vec<MergedBucket>> {
        self.seen_bucket_queries.lock().push(query.clone());
        if query.org_id != self.org_id {
            return Ok(Vec::new());
        }
        Ok(self
            .buckets
            .iter()
            .filter(|bucket| bucket.dimension.kind() == query.kind)
            .filter(|bucket| {
                query
                    .service_name
                    .as_deref()
                    .is_none_or(|value| bucket.dimension.service().name == value)
                    && query
                        .environment
                        .as_deref()
                        .is_none_or(|value| bucket.dimension.service().environment == value)
                    && query
                        .version
                        .as_deref()
                        .is_none_or(|value| bucket.dimension.version() == Some(value))
            })
            .cloned()
            .collect())
    }

    async fn list_services(&self, query: &CatalogQuery) -> Result<Vec<ServiceObservation>> {
        self.seen_catalog_queries.lock().push(query.clone());
        if query.org_id != self.org_id {
            return Ok(Vec::new());
        }
        Ok(self
            .services
            .iter()
            .filter(|service| {
                query
                    .service_name
                    .as_deref()
                    .is_none_or(|value| service.service.name == value)
                    && query
                        .environment
                        .as_deref()
                        .is_none_or(|value| service.service.environment == value)
            })
            .cloned()
            .collect())
    }

    async fn list_versions(&self, _query: &CatalogQuery) -> Result<Vec<VersionObservation>> {
        Ok(Vec::new())
    }

    async fn list_error_groups(&self, _query: &ErrorGroupQuery) -> Result<Vec<ErrorGroupRecord>> {
        Ok(Vec::new())
    }

    async fn list_error_samples(
        &self,
        _org_id: &Id,
        _fingerprint: &str,
    ) -> Result<Vec<ErrorSample>> {
        Ok(Vec::new())
    }

    async fn projection_state(&self, org_id: &Id) -> Result<Option<ProjectionState>> {
        Ok((org_id == &self.org_id)
            .then(|| self.state.clone())
            .flatten())
    }

    async fn projection_gaps(&self, org_id: &Id, _range: TimeRange) -> Result<Vec<ProjectionGap>> {
        Ok(if org_id == &self.org_id {
            self.gaps.clone()
        } else {
            Vec::new()
        })
    }
}

fn service(name: &str) -> ServiceIdentity {
    ServiceIdentity::new(Some("shop"), Some(name), Some("prod"), None)
}

fn fixture() -> Arc<FixtureRepository> {
    let org_id = Id::from_string("org-a");
    let service = service("api");
    let schema = HistogramSchema::v1();
    let mut latency = LatencyHistogram::empty(&schema);
    latency.observe(&schema, 10_000).expect("observe");
    let transaction_latency = latency.clone();
    Arc::new(FixtureRepository {
        org_id: org_id.clone(),
        services: vec![ServiceObservation {
            org_id: org_id.clone(),
            service: service.clone(),
            first_seen_at: TimestampMicros(100),
            last_seen_at: TimestampMicros(900),
            runtime_language: Some("rust".into()),
            telemetry_sdk_name: Some("opentelemetry".into()),
            telemetry_sdk_version: Some("1".into()),
            recent_instance_count: 1,
        }],
        buckets: vec![
            MergedBucket {
                bucket_at: TimestampMicros(500),
                dimension: BucketDimension::Service {
                    service: service.clone(),
                    version: Some("v1".into()),
                },
                measurements: BucketMeasurements {
                    request_count: 1,
                    error_count: 0,
                    overflow_count: 0,
                    latency,
                    exemplars: Vec::new(),
                },
            },
            MergedBucket {
                bucket_at: TimestampMicros(500),
                dimension: BucketDimension::Transaction {
                    service,
                    version: Some("v1".into()),
                    transaction: TransactionIdentity {
                        name: "POST /orders".into(),
                        kind: TransactionKind::Http,
                    },
                },
                measurements: BucketMeasurements {
                    request_count: 1,
                    error_count: 0,
                    overflow_count: 0,
                    latency: transaction_latency,
                    exemplars: Vec::new(),
                },
            },
        ],
        state: Some(ProjectionState {
            org_id: org_id.clone(),
            projection_started_at: TimestampMicros(200),
            last_complete_bucket_at: Some(TimestampMicros(500)),
            last_rollup_bucket_at: None,
        }),
        gaps: vec![ProjectionGap {
            org_id,
            range: TimeRange::new(TimestampMicros(400), TimestampMicros(450)),
            reason: ProjectionGapReason::QueueFull,
            dropped_facts: 1,
            recorded_at: TimestampMicros(451),
        }],
        seen_bucket_queries: Mutex::new(Vec::new()),
        seen_catalog_queries: Mutex::new(Vec::new()),
    })
}

fn query_service(repository: Arc<FixtureRepository>) -> ApmQueryService {
    ApmQueryService::new(
        repository,
        ApmQueryConfig {
            max_range_micros: 10_000,
            hot_resolution_micros: 10_000,
            minimum_version_requests: 1,
            histogram: HistogramSchema::v1(),
        },
    )
}

#[tokio::test]
async fn overview_includes_high_impact_transactions_for_the_selected_scope() {
    let repository = fixture();
    let service = query_service(repository);
    let context = service
        .context(
            Id::from_string("org-a"),
            ApmQueryRequest {
                from: 0,
                to: 1_000,
                service: Some("api".into()),
                environment: Some("prod".into()),
                version: Some("v1".into()),
                resolution: QueryResolution::Minute,
                ..ApmQueryRequest::default()
            },
            &["total_time"],
            "total_time",
        )
        .expect("context");

    let response = service.overview(&context).await.expect("overview");

    assert_eq!(response.top_transactions.len(), 1);
    assert_eq!(
        response.top_transactions[0].transaction.name,
        "POST /orders"
    );
    assert_eq!(response.top_transactions[0].service.name, "api");
}

#[tokio::test]
async fn transaction_detail_returns_scoped_red_trend_and_identity() {
    let repository = fixture();
    let service = query_service(repository);
    let context = service
        .context(
            Id::from_string("org-a"),
            ApmQueryRequest {
                from: 0,
                to: 1_000,
                service: Some("api".into()),
                environment: Some("prod".into()),
                resolution: QueryResolution::Minute,
                ..ApmQueryRequest::default()
            },
            &["request_count"],
            "request_count",
        )
        .expect("context");

    let response = service
        .transaction_detail(&context, "POST /orders", Some(TransactionKind::Http))
        .await
        .expect("transaction detail");

    assert_eq!(response.transaction.transaction.name, "POST /orders");
    assert_eq!(response.transaction.transaction.kind, TransactionKind::Http);
    assert_eq!(response.transaction.service.name, "api");
    assert_eq!(response.transaction.version.as_deref(), Some("v1"));
    assert_eq!(response.transaction.red.request_count, 1);
    assert_eq!(response.trend.len(), 1);
}

#[tokio::test]
async fn missing_transaction_is_hidden_as_not_found() {
    let repository = fixture();
    let service = query_service(repository);
    let context = service
        .context(
            Id::from_string("org-a"),
            ApmQueryRequest {
                from: 0,
                to: 1_000,
                resolution: QueryResolution::Minute,
                ..ApmQueryRequest::default()
            },
            &["request_count"],
            "request_count",
        )
        .expect("context");

    assert!(matches!(
        service
            .transaction_detail(&context, "GET /missing", Some(TransactionKind::Http))
            .await,
        Err(Error::NotFound(_))
    ));
}

#[tokio::test]
async fn service_filters_reach_every_repository_query_and_partial_is_explicit() {
    let repository = fixture();
    let service = query_service(repository.clone());
    let context = service
        .context(
            Id::from_string("org-a"),
            ApmQueryRequest {
                from: 0,
                to: 1_000,
                service: Some("api".into()),
                environment: Some("prod".into()),
                version: Some("v1".into()),
                resolution: QueryResolution::Minute,
                ..ApmQueryRequest::default()
            },
            &["request_count"],
            "request_count",
        )
        .expect("context");
    let response = service.service_detail(&context).await.expect("detail");
    assert_eq!(response.red.request_count, 1);
    assert!(response.meta.data_quality.partial);
    assert!(response.meta.activation_boundary);
    assert_eq!(response.meta.data_quality.gaps.len(), 1);
    assert!(repository.seen_bucket_queries.lock().iter().all(|query| {
        query.service_name.as_deref() == Some("api")
            && query.environment.as_deref() == Some("prod")
            && query.version.as_deref() == Some("v1")
    }));
}

#[tokio::test]
async fn cross_organization_and_missing_service_are_both_hidden_as_not_found() {
    let repository = fixture();
    let service = query_service(repository);
    let context = service
        .context(
            Id::from_string("org-b"),
            ApmQueryRequest {
                from: 0,
                to: 1_000,
                service: Some("api".into()),
                environment: Some("prod".into()),
                ..ApmQueryRequest::default()
            },
            &["request_count"],
            "request_count",
        )
        .expect("context");
    assert!(matches!(
        service.service_detail(&context).await,
        Err(Error::NotFound(_))
    ));
}

#[tokio::test]
async fn stable_cursor_advances_without_repeating_a_service() {
    let repository = fixture();
    let mut second = repository.services[0].clone();
    second.service = service("worker");
    // Test-only fixture mutation occurs before sharing with the query service.
    let repository = Arc::new(FixtureRepository {
        org_id: repository.org_id.clone(),
        services: vec![repository.services[0].clone(), second],
        buckets: repository.buckets.clone(),
        state: repository.state.clone(),
        gaps: Vec::new(),
        seen_bucket_queries: Mutex::new(Vec::new()),
        seen_catalog_queries: Mutex::new(Vec::new()),
    });
    let service = query_service(repository);
    let first_context = service
        .context(
            Id::from_string("org-a"),
            ApmQueryRequest {
                from: 0,
                to: 1_000,
                sort: Some("name".into()),
                direction: Some(crate::domain::apm::SortDirection::Asc),
                limit: Some(1),
                ..ApmQueryRequest::default()
            },
            &["name"],
            "name",
        )
        .expect("context");
    let first = service.services(&first_context).await.expect("first page");
    let cursor = first.next_cursor.expect("next cursor");
    let second_context = service
        .context(
            Id::from_string("org-a"),
            ApmQueryRequest {
                from: 0,
                to: 1_000,
                sort: Some("name".into()),
                direction: Some(crate::domain::apm::SortDirection::Asc),
                limit: Some(1),
                cursor: Some(cursor),
                ..ApmQueryRequest::default()
            },
            &["name"],
            "name",
        )
        .expect("context");
    let second = service
        .services(&second_context)
        .await
        .expect("second page");
    assert_ne!(
        first.items[0].service.stable_key(),
        second.items[0].service.stable_key()
    );
    let previous_cursor = second.previous_cursor.expect("previous cursor");
    let previous_context = service
        .context(
            Id::from_string("org-a"),
            ApmQueryRequest {
                from: 0,
                to: 1_000,
                sort: Some("name".into()),
                direction: Some(crate::domain::apm::SortDirection::Asc),
                limit: Some(1),
                cursor: Some(previous_cursor),
                ..ApmQueryRequest::default()
            },
            &["name"],
            "name",
        )
        .expect("previous context");
    let previous = service
        .services(&previous_context)
        .await
        .expect("previous page");
    assert_eq!(
        first.items[0].service.stable_key(),
        previous.items[0].service.stable_key()
    );
}
