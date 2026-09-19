// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::{ApmQueryContext, ApmQueryRange, ApmQueryService, ApmResponseMeta, SignalFilterHandle};
use crate::{
    domain::apm::{
        BucketKind, BucketQuery, CatalogQuery, DataQuality, ErrorGroupQuery, QueryResolution,
    },
    shared::Result,
};

impl ApmQueryService {
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
