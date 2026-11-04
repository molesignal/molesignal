// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use super::{
    ApmQueryContext, ApmQueryService, ErrorDetailResponse, ErrorIssueView, ErrorListResponse,
    ErrorSampleView, ErrorSummary, PagedResponse,
    aggregate::{red_from_buckets, trend_from_buckets},
    breakdown::error_breakdown,
    common::{overflow_dimensions, signal_handle},
    pagination::paginate,
    ranking::transaction_summaries,
};
use crate::{
    domain::apm::{BucketDimension, BucketKind, ErrorGroupRecord, ErrorIssueStatus, MergedBucket},
    shared::{Error, Result},
};

/// Which error groups a list keeps, by what is being done about them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorIssueFilter {
    #[default]
    All,
    /// Open or being investigated, and resolved ones seen again since.
    Unresolved,
    Investigating,
    Resolved,
    Ignored,
}

impl ErrorIssueFilter {
    fn keeps(self, summary: &ErrorSummary) -> bool {
        let issue = summary.issue.as_ref();
        let status = issue.map_or(ErrorIssueStatus::Open, |issue| issue.status);
        let regressed = issue.is_some_and(|issue| issue.regressed);

        match self {
            Self::All => true,
            Self::Unresolved => {
                matches!(
                    status,
                    ErrorIssueStatus::Open | ErrorIssueStatus::Investigating
                ) || regressed
            }
            Self::Investigating => status == ErrorIssueStatus::Investigating,
            Self::Resolved => status == ErrorIssueStatus::Resolved && !regressed,
            Self::Ignored => status == ErrorIssueStatus::Ignored,
        }
    }
}

impl ApmQueryService {
    pub async fn errors(
        &self,
        context: &ApmQueryContext,
        filter: ErrorIssueFilter,
    ) -> Result<ErrorListResponse> {
        let group_query = self.error_group_query(context, None);
        let bucket_query = self.bucket_query(context, BucketKind::Error);
        let catalog_query = self.catalog_query(context);

        let (groups, buckets, catalog) = tokio::try_join!(
            self.repository.list_error_groups(&group_query),
            self.repository.query_buckets(&bucket_query),
            self.repository.list_services(&catalog_query),
        )?;

        let mut items = error_summaries(context, &groups, &buckets, &self.config.histogram)?;

        self.attach_issues(context, &mut items).await?;
        items.retain(|item| filter.keeps(item));

        let kept = items
            .iter()
            .map(|item| item.error.fingerprint.as_str())
            .collect::<HashSet<_>>();
        let breakdown = error_breakdown(&buckets, &catalog, Some(&kept));

        let latency_partial = items.iter().any(|item| item.red.latency_partial);
        let overflows = overflow_dimensions(&buckets);
        let (items, previous_cursor, next_cursor) = paginate(items, context)?;
        let has_more = next_cursor.is_some();

        Ok(ErrorListResponse {
            page: PagedResponse {
                meta: self
                    .response_meta(context, overflows, latency_partial)
                    .await?,
                items,
                next_cursor,
                previous_cursor,
                has_more,
                sort: context.sort.clone(),
            },
            breakdown,
        })
    }

    pub async fn error_detail(
        &self,
        context: &ApmQueryContext,
        fingerprint: &str,
    ) -> Result<ErrorDetailResponse> {
        if fingerprint.is_empty() || fingerprint.len() > 192 {
            return Err(Error::not_found("APM error group"));
        }

        let group_query = self.error_group_query(context, Some(fingerprint.to_owned()));
        let error_query = self.bucket_query(context, BucketKind::Error);
        let transaction_query = self.bucket_query(context, BucketKind::Transaction);
        let catalog_query = self.catalog_query(context);

        let (groups, error_buckets, transaction_buckets, samples, thresholds, catalog) = tokio::try_join!(
            self.repository.list_error_groups(&group_query),
            self.repository.query_buckets(&error_query),
            self.repository.query_buckets(&transaction_query),
            self.repository
                .list_error_samples(&context.org_id, fingerprint),
            self.apdex_thresholds(&context.org_id),
            self.repository.list_services(&catalog_query),
        )?;

        let group = groups
            .into_iter()
            .next()
            .ok_or_else(|| Error::not_found("APM error group"))?;

        let matching = error_buckets
            .into_iter()
            .filter(|bucket| {
                matches!(
                    &bucket.dimension,
                    BucketDimension::Error { error, .. }
                        if error.fingerprint == fingerprint
                )
            })
            .collect::<Vec<_>>();

        let mut summaries = error_summaries(
            context,
            std::slice::from_ref(&group),
            &matching,
            &self.config.histogram,
        )?;

        self.attach_issues(context, &mut summaries).await?;

        let summary = summaries
            .into_iter()
            .next()
            .ok_or_else(|| Error::not_found("APM error group"))?;

        let transaction_name = group.error.transaction_name.as_deref();

        let affected_transactions = transaction_summaries(
            context,
            &transaction_buckets,
            &self.config.histogram,
            &thresholds,
        )?
        .into_iter()
        .filter(|transaction| {
            transaction_name.is_none_or(|name| transaction.transaction.name == name)
        })
        .take(20)
        .collect();

        let mut affected_versions = matching
            .iter()
            .filter_map(|bucket| bucket.dimension.version().map(str::to_owned))
            .collect::<Vec<_>>();

        affected_versions.sort();
        affected_versions.dedup();
        let latency_partial = summary.red.latency_partial;

        Ok(ErrorDetailResponse {
            meta: self
                .response_meta(context, overflow_dimensions(&matching), latency_partial)
                .await?,
            trend: trend_from_buckets(&matching, &self.config.histogram)?,
            representative_stack: group.representative_stack,
            group: summary,
            affected_transactions,
            affected_versions,
            breakdown: error_breakdown(&matching, &catalog, None),
            samples: samples
                .into_iter()
                .map(|sample| ErrorSampleView {
                    event_time: sample.event_time,
                    trace_link: sample
                        .trace_available
                        .then(|| format!("/traces/{}", sample.trace_id)),
                    trace_id: sample.trace_id,
                    span_id: sample.span_id,
                    trace_available: sample.trace_available,
                    representative_message: sample.representative_message,
                    representative_stack: sample.representative_stack,
                })
                .collect(),
        })
    }
}

impl ApmQueryService {
    /// Puts on each summary what is being done about its group.
    pub(super) async fn attach_issues(
        &self,
        context: &ApmQueryContext,
        summaries: &mut [ErrorSummary],
    ) -> Result<()> {
        let Some(catalog) = &self.catalog else {
            return Ok(());
        };

        let fingerprints = summaries
            .iter()
            .map(|summary| summary.error.fingerprint.clone())
            .collect::<Vec<_>>();

        let mut issues = catalog
            .list_issues(&context.org_id, &fingerprints)
            .await?
            .into_iter()
            .map(|issue| (issue.fingerprint.clone(), issue))
            .collect::<HashMap<_, _>>();

        for summary in summaries {
            summary.issue = issues
                .remove(&summary.error.fingerprint)
                .map(|issue| ErrorIssueView::of(issue, summary.last_seen_at));
        }

        Ok(())
    }
}

pub(super) fn error_summaries(
    context: &ApmQueryContext,
    groups: &[ErrorGroupRecord],
    buckets: &[MergedBucket],
    histogram: &crate::domain::apm::HistogramSchema,
) -> Result<Vec<ErrorSummary>> {
    let mut by_fingerprint = HashMap::<String, Vec<MergedBucket>>::new();

    for bucket in buckets {
        if let BucketDimension::Error { error, .. } = &bucket.dimension {
            by_fingerprint
                .entry(error.fingerprint.clone())
                .or_default()
                .push(bucket.clone());
        }
    }

    groups
        .iter()
        .map(|group| {
            let matching = by_fingerprint
                .remove(&group.error.fingerprint)
                .unwrap_or_default();

            let red = red_from_buckets(&matching, histogram)?;

            Ok(ErrorSummary {
                traces: signal_handle(
                    context,
                    &group.service,
                    None,
                    group.error.transaction_name.clone(),
                    None,
                    Some(group.error.fingerprint.clone()),
                ),
                error: group.error.clone(),
                service: group.service.clone(),
                first_seen_at: group.first_seen_at,
                last_seen_at: group.last_seen_at,
                occurrence_count: group.occurrence_count.max(red.request_count),
                representative_message: group.representative_message.clone(),
                red,
                issue: None,
            })
        })
        .collect()
}
