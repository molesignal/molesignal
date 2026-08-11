// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::{BTreeSet, HashMap};

use super::{
    ApmQueryContext, ApmQueryService, DependencySummary, PagedResponse, TransactionDetailResponse,
    TransactionSummary,
    aggregate::{red_from_buckets, trend_from_buckets},
    common::{overflow_dimensions, signal_handle},
    errors::error_summaries,
    overview::version_summary,
    pagination::paginate,
};
use crate::{
    domain::apm::{BucketDimension, BucketKind, MergedBucket, TransactionKind},
    shared::{Error, Result},
};

impl ApmQueryService {
    pub async fn transactions(
        &self,
        context: &ApmQueryContext,
    ) -> Result<PagedResponse<TransactionSummary>> {
        let buckets = self
            .repository
            .query_buckets(&self.bucket_query(context, BucketKind::Transaction))
            .await?;
        let items = transaction_summaries(context, &buckets, &self.config.histogram)?;
        let latency_partial = items.iter().any(|item| item.red.latency_partial);
        let overflows = overflow_dimensions(&buckets);
        let (items, previous_cursor, next_cursor) = paginate(items, context)?;
        let has_more = next_cursor.is_some();
        Ok(PagedResponse {
            meta: self
                .response_meta(context, overflows, latency_partial)
                .await?,
            items,
            next_cursor,
            previous_cursor,
            has_more,
            sort: context.sort.clone(),
        })
    }

    pub async fn transaction_detail(
        &self,
        context: &ApmQueryContext,
        transaction_name: &str,
        kind: Option<TransactionKind>,
    ) -> Result<TransactionDetailResponse> {
        let transaction_name = transaction_name.trim();
        if transaction_name.is_empty() || transaction_name.len() > 512 {
            return Err(Error::not_found("APM transaction"));
        }

        let transaction_query = self.bucket_query(context, BucketKind::Transaction);
        let error_query = self.bucket_query(context, BucketKind::Error);
        let group_query = self.error_group_query(context, None);
        let catalog_query = self.catalog_query(context);
        let (transaction_buckets, error_buckets, groups, versions) = tokio::try_join!(
            self.repository.query_buckets(&transaction_query),
            self.repository.query_buckets(&error_query),
            self.repository.list_error_groups(&group_query),
            self.repository.list_versions(&catalog_query),
        )?;

        let matching = transaction_buckets
            .into_iter()
            .filter(|bucket| {
                matches!(
                    &bucket.dimension,
                    BucketDimension::Transaction { transaction, .. }
                        if transaction.name == transaction_name
                            && kind.is_none_or(|value| transaction.kind == value)
                )
            })
            .collect::<Vec<_>>();
        let identities = matching
            .iter()
            .filter_map(|bucket| match &bucket.dimension {
                BucketDimension::Transaction {
                    service,
                    transaction,
                    ..
                } => Some((service.clone(), transaction.clone())),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        if identities.len() > 1 {
            return Err(Error::invalid(
                "APM transaction identity is ambiguous; provide service, namespace, environment, and kind",
            ));
        }
        let (service, transaction) = identities
            .into_iter()
            .next()
            .ok_or_else(|| Error::not_found("APM transaction"))?;

        let observed_versions = matching
            .iter()
            .map(|bucket| bucket.dimension.version().map(str::to_owned))
            .collect::<BTreeSet<_>>();
        let version = (observed_versions.len() == 1)
            .then(|| observed_versions.into_iter().next())
            .flatten()
            .flatten();
        let red = red_from_buckets(&matching, &self.config.histogram)?;
        let transaction_summary = TransactionSummary {
            traces: signal_handle(
                context,
                &service,
                version.clone(),
                Some(transaction.name.clone()),
                None,
                None,
            ),
            service: service.clone(),
            version,
            transaction,
            total_time_micros: red.duration_sum_micros,
            red,
        };

        let matching_error_buckets = error_buckets
            .into_iter()
            .filter(|bucket| {
                matches!(
                    &bucket.dimension,
                    BucketDimension::Error {
                        service: error_service,
                        error,
                        ..
                    } if error_service == &service
                        && error.transaction_name.as_deref() == Some(transaction_name)
                )
            })
            .collect::<Vec<_>>();
        let matching_groups = groups
            .into_iter()
            .filter(|group| {
                group.service == service
                    && group.error.transaction_name.as_deref() == Some(transaction_name)
            })
            .collect::<Vec<_>>();
        let mut errors = error_summaries(
            context,
            &matching_groups,
            &matching_error_buckets,
            &self.config.histogram,
        )?;
        errors.sort_by(|left, right| {
            right
                .occurrence_count
                .cmp(&left.occurrence_count)
                .then_with(|| left.error.fingerprint.cmp(&right.error.fingerprint))
        });
        errors.truncate(10);

        let mut versions = versions
            .into_iter()
            .filter(|version| version.service == service)
            .map(version_summary)
            .collect::<Vec<_>>();
        versions.sort_by(|left, right| {
            right
                .last_seen_at
                .cmp(&left.last_seen_at)
                .then_with(|| left.version.cmp(&right.version))
        });

        let latency_partial = transaction_summary.red.latency_partial
            || errors.iter().any(|error| error.red.latency_partial);
        let mut overflows = overflow_dimensions(&matching);
        overflows.extend(overflow_dimensions(&matching_error_buckets));
        Ok(TransactionDetailResponse {
            meta: self
                .response_meta(context, overflows, latency_partial)
                .await?,
            trend: trend_from_buckets(&matching, &self.config.histogram)?,
            transaction: transaction_summary,
            errors,
            versions,
        })
    }

    pub async fn dependencies(
        &self,
        context: &ApmQueryContext,
    ) -> Result<PagedResponse<DependencySummary>> {
        let buckets = self
            .repository
            .query_buckets(&self.bucket_query(context, BucketKind::Dependency))
            .await?;
        let items = dependency_summaries(context, &buckets, &self.config.histogram)?;
        let latency_partial = items.iter().any(|item| item.red.latency_partial);
        let overflows = overflow_dimensions(&buckets);
        let (items, previous_cursor, next_cursor) = paginate(items, context)?;
        let has_more = next_cursor.is_some();
        Ok(PagedResponse {
            meta: self
                .response_meta(context, overflows, latency_partial)
                .await?,
            items,
            next_cursor,
            previous_cursor,
            has_more,
            sort: context.sort.clone(),
        })
    }
}

pub(super) fn transaction_summaries(
    context: &ApmQueryContext,
    buckets: &[MergedBucket],
    histogram: &crate::domain::apm::HistogramSchema,
) -> Result<Vec<TransactionSummary>> {
    let mut grouped = HashMap::new();
    for bucket in buckets {
        if let BucketDimension::Transaction {
            service,
            version,
            transaction,
        } = &bucket.dimension
        {
            grouped
                .entry((service.clone(), version.clone(), transaction.clone()))
                .or_insert_with(Vec::new)
                .push(bucket.clone());
        }
    }
    grouped
        .into_iter()
        .map(|((service, version, transaction), buckets)| {
            let red = red_from_buckets(&buckets, histogram)?;
            Ok(TransactionSummary {
                traces: signal_handle(
                    context,
                    &service,
                    version.clone(),
                    Some(transaction.name.clone()),
                    None,
                    None,
                ),
                service,
                version,
                transaction,
                total_time_micros: red.duration_sum_micros,
                red,
            })
        })
        .collect()
}

pub(super) fn dependency_summaries(
    context: &ApmQueryContext,
    buckets: &[MergedBucket],
    histogram: &crate::domain::apm::HistogramSchema,
) -> Result<Vec<DependencySummary>> {
    let mut grouped = HashMap::new();
    for bucket in buckets {
        if let BucketDimension::Dependency {
            service,
            version,
            dependency,
        } = &bucket.dimension
        {
            grouped
                .entry((service.clone(), version.clone(), dependency.clone()))
                .or_insert_with(Vec::new)
                .push(bucket.clone());
        }
    }
    grouped
        .into_iter()
        .map(|((service, version, dependency), buckets)| {
            let red = red_from_buckets(&buckets, histogram)?;
            Ok(DependencySummary {
                traces: signal_handle(
                    context,
                    &service,
                    version.clone(),
                    None,
                    Some(dependency.target.clone()),
                    None,
                ),
                service,
                version,
                dependency,
                total_time_micros: red.duration_sum_micros,
                red,
            })
        })
        .collect()
}
