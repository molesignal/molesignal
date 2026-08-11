// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashMap;

use super::{
    ApmQueryContext, ApmQueryService, RedDelta, RedSummary, VersionCompareResponse, VersionSide,
    aggregate::red_from_buckets, common::overflow_dimensions, errors::error_summaries,
    ranking::transaction_summaries,
};
use crate::{
    domain::apm::BucketKind,
    shared::{Error, Result},
};

impl ApmQueryService {
    pub async fn compare_versions(
        &self,
        context: &ApmQueryContext,
        baseline: &str,
        candidate: &str,
    ) -> Result<VersionCompareResponse> {
        validate_version(baseline)?;
        validate_version(candidate)?;
        if baseline == candidate {
            return Err(Error::invalid(
                "baseline and candidate versions must differ",
            ));
        }
        if context.service_name.is_none() {
            return Err(Error::invalid(
                "version comparison requires a service filter",
            ));
        }
        let mut baseline_context = context.clone();
        baseline_context.version = Some(baseline.to_owned());
        let mut candidate_context = context.clone();
        candidate_context.version = Some(candidate.to_owned());
        let baseline_service_query = self.bucket_query(&baseline_context, BucketKind::Service);
        let candidate_service_query = self.bucket_query(&candidate_context, BucketKind::Service);
        let baseline_transaction_query =
            self.bucket_query(&baseline_context, BucketKind::Transaction);
        let candidate_transaction_query =
            self.bucket_query(&candidate_context, BucketKind::Transaction);
        let baseline_error_query = self.bucket_query(&baseline_context, BucketKind::Error);
        let candidate_error_query = self.bucket_query(&candidate_context, BucketKind::Error);
        let error_group_query = self.error_group_query(context, None);
        let (
            baseline_service,
            candidate_service,
            baseline_transactions,
            candidate_transactions,
            baseline_errors,
            candidate_errors,
            error_groups,
        ) = tokio::try_join!(
            self.repository.query_buckets(&baseline_service_query),
            self.repository.query_buckets(&candidate_service_query),
            self.repository.query_buckets(&baseline_transaction_query),
            self.repository.query_buckets(&candidate_transaction_query),
            self.repository.query_buckets(&baseline_error_query),
            self.repository.query_buckets(&candidate_error_query),
            self.repository.list_error_groups(&error_group_query),
        )?;
        let baseline_red = red_from_buckets(&baseline_service, &self.config.histogram)?;
        let candidate_red = red_from_buckets(&candidate_service, &self.config.histogram)?;
        let sufficient_data = baseline_red.request_count >= self.config.minimum_version_requests
            && candidate_red.request_count >= self.config.minimum_version_requests;
        let delta = red_delta(&baseline_red, &candidate_red);
        let mut regressed_transactions = regressed_transactions(
            context,
            &baseline_transactions,
            &candidate_transactions,
            &self.config.histogram,
        )?;
        regressed_transactions.truncate(20);
        let mut regressed_errors = error_summaries(
            &candidate_context,
            &error_groups,
            &candidate_errors,
            &self.config.histogram,
        )?;
        regressed_errors.sort_by(|left, right| {
            right
                .occurrence_count
                .cmp(&left.occurrence_count)
                .then_with(|| left.error.fingerprint.cmp(&right.error.fingerprint))
        });
        regressed_errors.truncate(20);
        let status = if !sufficient_data {
            "insufficient_data"
        } else if delta.error_rate_absolute > 0.0
            || delta.p95_absolute_micros.is_some_and(|value| value > 0)
        {
            "regressed"
        } else if delta.error_rate_absolute < 0.0
            || delta.p95_absolute_micros.is_some_and(|value| value < 0)
        {
            "improved"
        } else {
            "neutral"
        };
        let latency_partial = baseline_red.latency_partial || candidate_red.latency_partial;
        let mut overflows = overflow_dimensions(&baseline_service);
        overflows.extend(overflow_dimensions(&candidate_service));
        overflows.extend(overflow_dimensions(&baseline_transactions));
        overflows.extend(overflow_dimensions(&candidate_transactions));
        overflows.extend(overflow_dimensions(&baseline_errors));
        overflows.extend(overflow_dimensions(&candidate_errors));
        Ok(VersionCompareResponse {
            meta: self
                .response_meta(context, overflows, latency_partial)
                .await?,
            baseline: VersionSide {
                version: baseline.to_owned(),
                sample_count: baseline_red.request_count,
                red: baseline_red,
            },
            candidate: VersionSide {
                version: candidate.to_owned(),
                sample_count: candidate_red.request_count,
                red: candidate_red,
            },
            sufficient_data,
            status,
            delta,
            regressed_transactions,
            regressed_errors,
        })
    }
}

fn validate_version(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 192 || value.contains('\0') {
        return Err(Error::invalid("invalid APM version"));
    }
    Ok(())
}

fn red_delta(baseline: &RedSummary, candidate: &RedSummary) -> RedDelta {
    RedDelta {
        request_count_absolute: signed_delta(candidate.request_count, baseline.request_count),
        request_count_relative: relative(
            candidate.request_count as f64,
            baseline.request_count as f64,
        ),
        error_rate_absolute: candidate.error_rate - baseline.error_rate,
        error_rate_relative: relative(candidate.error_rate, baseline.error_rate),
        p95_absolute_micros: candidate
            .p95_micros
            .zip(baseline.p95_micros)
            .map(|(candidate, baseline)| signed_delta(candidate, baseline)),
        p95_relative: candidate
            .p95_micros
            .zip(baseline.p95_micros)
            .and_then(|(candidate, baseline)| relative(candidate as f64, baseline as f64)),
    }
}

fn relative(candidate: f64, baseline: f64) -> Option<f64> {
    (baseline != 0.0).then(|| (candidate - baseline) / baseline)
}

fn signed_delta(candidate: u64, baseline: u64) -> i64 {
    i128::from(candidate)
        .saturating_sub(i128::from(baseline))
        .clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

fn regressed_transactions(
    context: &ApmQueryContext,
    baseline: &[crate::domain::apm::MergedBucket],
    candidate: &[crate::domain::apm::MergedBucket],
    histogram: &crate::domain::apm::HistogramSchema,
) -> Result<Vec<super::TransactionSummary>> {
    let baseline = transaction_summaries(context, baseline, histogram)?
        .into_iter()
        .map(|item| (item.transaction.name.clone(), item.red))
        .collect::<HashMap<_, _>>();
    let mut candidate = transaction_summaries(context, candidate, histogram)?;
    candidate.sort_by(|left, right| {
        regression_score(&right.red, baseline.get(&right.transaction.name))
            .total_cmp(&regression_score(
                &left.red,
                baseline.get(&left.transaction.name),
            ))
            .then_with(|| left.transaction.name.cmp(&right.transaction.name))
    });
    Ok(candidate)
}

fn regression_score(candidate: &RedSummary, baseline: Option<&RedSummary>) -> f64 {
    let Some(baseline) = baseline else {
        return candidate.error_rate + candidate.p95_micros.unwrap_or_default() as f64;
    };
    (candidate.error_rate - baseline.error_rate).max(0.0) * 1_000_000.0
        + candidate
            .p95_micros
            .unwrap_or_default()
            .saturating_sub(baseline.p95_micros.unwrap_or_default()) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_delta_and_insufficient_semantics_are_stable() {
        let baseline = RedSummary {
            request_count: 100,
            error_rate: 0.01,
            p95_micros: Some(100),
            ..RedSummary::default()
        };
        let candidate = RedSummary {
            request_count: 120,
            error_rate: 0.03,
            p95_micros: Some(150),
            ..RedSummary::default()
        };
        let delta = red_delta(&baseline, &candidate);
        assert_eq!(delta.request_count_absolute, 20);
        assert_eq!(delta.p95_absolute_micros, Some(50));
        assert!((delta.error_rate_absolute - 0.02).abs() < f64::EPSILON);
    }
}
