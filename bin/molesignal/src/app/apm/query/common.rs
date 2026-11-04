// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use super::{ApmQueryContext, ApmQueryRange, ApmQueryService, ApmResponseMeta, SignalFilterHandle};
use crate::{
    domain::apm::{
        BucketKind, BucketQuery, CatalogQuery, DataQuality, ErrorGroupQuery, QueryResolution,
        TraceExemplar,
    },
    shared::{
        Result,
        time::{TimeRange, TimestampMicros},
    },
};

const HOUR_MICROS: i64 = 3_600_000_000;

impl ApmQueryService {
    /// The traces the services in view kept as examples within the range,
    /// the most recent first, each trace once.
    pub async fn recent_exemplars(
        &self,
        context: &ApmQueryContext,
        limit: usize,
    ) -> Result<Vec<TraceExemplar>> {
        let buckets = self
            .repository
            .query_buckets(&self.bucket_query(context, BucketKind::Service))
            .await?;

        let mut exemplars = buckets
            .into_iter()
            .flat_map(|bucket| bucket.measurements.exemplars)
            .collect::<Vec<_>>();

        exemplars.sort_by_key(|exemplar| std::cmp::Reverse(exemplar.event_time));
        let mut seen = HashSet::new();
        exemplars.retain(|exemplar| seen.insert(exemplar.trace_id.clone()));
        exemplars.truncate(limit);

        Ok(exemplars)
    }

    /// `range` as the buckets still hold it: at `resolution` while minute
    /// buckets are kept for all of it, otherwise the hours that cover it. The
    /// last figure is how long the buckets read cover, which counts are told by.
    pub(super) fn held_range(
        &self,
        range: TimeRange,
        resolution: QueryResolution,
    ) -> (TimeRange, QueryResolution, i64) {
        let kept_from = TimestampMicros::now().0 - self.config.hot_resolution_micros;

        if resolution == QueryResolution::Hour || range.start.0 >= kept_from {
            return (range, resolution, range.duration_micros().max(1));
        }

        let from = range.start.0.div_euclid(HOUR_MICROS) * HOUR_MICROS;
        let last = (range.end.0 - 1).max(range.start.0).div_euclid(HOUR_MICROS) * HOUR_MICROS;

        (
            TimeRange::new(TimestampMicros(from), TimestampMicros(last)),
            QueryResolution::Hour,
            last - from + HOUR_MICROS,
        )
    }

    pub(super) fn bucket_query(&self, context: &ApmQueryContext, kind: BucketKind) -> BucketQuery {
        BucketQuery {
            org_id: context.org_id.clone(),
            range: context.range,
            kind,
            resolution: context.resolution,
            namespace: context.namespace.clone(),
            service_name: context.service_name.clone(),
            environment: context.environment.clone(),
            version: context.version.clone(),
        }
    }

    pub(super) fn catalog_query(&self, context: &ApmQueryContext) -> CatalogQuery {
        CatalogQuery {
            org_id: context.org_id.clone(),
            range: context.range,
            namespace: context.namespace.clone(),
            service_name: context.service_name.clone(),
            environment: context.environment.clone(),
        }
    }

    pub(super) fn error_group_query(
        &self,
        context: &ApmQueryContext,
        fingerprint: Option<String>,
    ) -> ErrorGroupQuery {
        ErrorGroupQuery {
            org_id: context.org_id.clone(),
            range: context.range,
            namespace: context.namespace.clone(),
            service_name: context.service_name.clone(),
            environment: context.environment.clone(),
            fingerprint,
        }
    }

    pub(super) async fn response_meta(
        &self,
        context: &ApmQueryContext,
        mut overflow_dimensions: Vec<String>,
        latency_partial: bool,
    ) -> Result<ApmResponseMeta> {
        let (state, gaps) = tokio::try_join!(
            self.repository.projection_state(&context.org_id),
            self.repository
                .projection_gaps(&context.org_id, context.range),
        )?;

        if latency_partial {
            overflow_dimensions.push("histogram_schema_mismatch".into());
        }

        overflow_dimensions.sort();
        overflow_dimensions.dedup();

        let activation_boundary = state
            .as_ref()
            .is_some_and(|state| context.range.start < state.projection_started_at);

        Ok(ApmResponseMeta {
            range: ApmQueryRange {
                from: context.range.start,
                to: context.range.end,
            },
            resolution: match context.resolution {
                QueryResolution::Auto => unreachable!("context resolves auto"),
                resolution => resolution,
            },
            projection_started_at: state.as_ref().map(|state| state.projection_started_at),
            last_complete_bucket_at: state
                .as_ref()
                .and_then(|state| state.last_complete_bucket_at),
            data_quality: DataQuality {
                partial: activation_boundary || !gaps.is_empty() || latency_partial,
                gaps,
                overflow_dimensions,
            },
            activation_boundary,
        })
    }
}

pub(super) fn signal_handle(
    context: &ApmQueryContext,
    service: &crate::domain::apm::ServiceIdentity,
    version: Option<String>,
    transaction: Option<String>,
    dependency: Option<String>,
    error_fingerprint: Option<String>,
) -> SignalFilterHandle {
    SignalFilterHandle {
        namespace: service.namespace.clone(),
        service: service.name.clone(),
        environment: service.environment.clone(),
        version,
        transaction,
        dependency,
        error_fingerprint,
        from: context.range.start,
        to: context.range.end,
    }
}

pub(super) fn overflow_dimensions(buckets: &[crate::domain::apm::MergedBucket]) -> Vec<String> {
    let mut dimensions = buckets
        .iter()
        .filter(|bucket| bucket.measurements.overflow_count > 0)
        .map(|bucket| format!("{:?}", bucket.dimension.kind()).to_ascii_lowercase())
        .collect::<Vec<_>>();

    dimensions.sort();
    dimensions.dedup();
    dimensions
}
