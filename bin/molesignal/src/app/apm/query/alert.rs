// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The figures alert rules watch in APM, read over a rule's window from the
//! same buckets the pages read: a service, or one of its endpoints, in the
//! unit thresholds are set in.

use async_trait::async_trait;

use super::{
    ApmQueryService, RedSummary,
    aggregate::{DEFAULT_APDEX_THRESHOLD_MICROS, red_from_buckets_at},
};
use crate::{
    app::alerting::{ApmAlertReading, ApmAlertSource},
    domain::apm::{
        ApmAlertMetric, ApmAlertQuery, BucketDimension, BucketKind, BucketQuery, ErrorGroupQuery,
        QueryResolution,
    },
    shared::{Error, Result, ids::Id, time::TimeRange},
};

#[async_trait]
impl ApmAlertSource for ApmQueryService {
    async fn read(
        &self,
        org_id: &Id,
        query: &ApmAlertQuery,
        range: TimeRange,
    ) -> Result<Option<ApmAlertReading>> {
        if query.metric == ApmAlertMetric::SloBurnRate {
            let id = query
                .slo_id
                .as_ref()
                .ok_or_else(|| Error::invalid("an SLO burn-rate alert names its SLO"))?;
            let short = query
                .short_window_secs
                .map(|secs| i64::from(secs) * 1_000_000);

            return Ok(self
                .slo_burn_rate(org_id, id, range, short)
                .await?
                .map(|value| ApmAlertReading {
                    value,
                    service: query.service.clone(),
                    trace_ids: Vec::new(),
                }));
        }

        if query.metric == ApmAlertMetric::NewErrorGroups {
            return self.new_error_groups(org_id, query, range).await.map(Some);
        }

        let kind = if query.transaction.is_some() {
            BucketKind::Transaction
        } else {
            BucketKind::Service
        };
        let resolution = if range.duration_micros() > self.config.hot_resolution_micros {
            QueryResolution::Hour
        } else {
            QueryResolution::Minute
        };
        // A window older than minute buckets are kept, as the days an anomaly
        // rule compares with are, is read from the hours that cover it.
        let (held, resolution, covered) = self.held_range(range, resolution);
        let buckets = self
            .repository
            .query_buckets(&BucketQuery {
                org_id: org_id.clone(),
                range: held,
                kind,
                resolution,
                namespace: query.namespace.clone(),
                service_name: Some(query.service.clone()),
                environment: query.environment.clone(),
                version: None,
            })
            .await?;
        let buckets = match &query.transaction {
            Some(wanted) => buckets
                .into_iter()
                .filter(|bucket| matches!(&bucket.dimension, BucketDimension::Transaction { transaction, .. } if transaction == wanted))
                .collect(),
            None => buckets,
        };
        // Apdex against the service's own threshold, as its pages read it.
        let threshold = if query.metric == ApmAlertMetric::Apdex {
            self.apdex_thresholds(org_id)
                .await?
                .named(query.namespace.as_deref(), &query.service)
        } else {
            DEFAULT_APDEX_THRESHOLD_MICROS
        };
        let red = red_from_buckets_at(&buckets, &self.config.histogram, threshold)?;
        let seconds = covered as f64 / 1_000_000.0;
        // Counts over covering hours stand for the window's share of them.
        let share = range.duration_micros().max(1) as f64 / covered as f64;

        Ok(
            metric_value(query.metric, &red, seconds).map(|value| ApmAlertReading {
                value: match query.metric {
                    ApmAlertMetric::RequestCount | ApmAlertMetric::ErrorCount => value * share,
                    _ => value,
                },
                service: query.service.clone(),
                trace_ids: red
                    .exemplars
                    .iter()
                    .map(|exemplar| exemplar.trace_id.clone())
                    .collect(),
            }),
        )
    }
}

impl ApmQueryService {
    /// The error groups of the service, or of its endpoint, first seen inside
    /// `range`; zero when nothing new failed.
    async fn new_error_groups(
        &self,
        org_id: &Id,
        query: &ApmAlertQuery,
        range: TimeRange,
    ) -> Result<ApmAlertReading> {
        let groups = self
            .repository
            .list_error_groups(&ErrorGroupQuery {
                org_id: org_id.clone(),
                range,
                namespace: query.namespace.clone(),
                service_name: Some(query.service.clone()),
                environment: query.environment.clone(),
                fingerprint: None,
            })
            .await?;
        let fresh = groups
            .iter()
            .filter(|group| group.first_seen_at >= range.start)
            .filter(|group| {
                query.transaction.as_ref().is_none_or(|wanted| {
                    group.error.transaction_name.as_deref() == Some(wanted.name.as_str())
                })
            })
            .count();

        Ok(ApmAlertReading {
            value: fresh as f64,
            service: query.service.clone(),
            trace_ids: Vec::new(),
        })
    }
}

/// A figure of a summary in the unit thresholds are set in: rates in percent,
/// latencies in milliseconds, throughput a second. Counts and throughput are
/// zero without requests, so a rule can watch traffic stop; rates, latencies
/// and Apdex have no figure then, so no threshold matches and an open
/// incident resolves.
fn metric_value(metric: ApmAlertMetric, red: &RedSummary, seconds: f64) -> Option<f64> {
    let millis = |micros: Option<u64>| micros.map(|value| value as f64 / 1_000.0);

    match metric {
        ApmAlertMetric::Throughput => Some(red.request_count as f64 / seconds),
        ApmAlertMetric::RequestCount => Some(red.request_count as f64),
        ApmAlertMetric::ErrorCount => Some(red.error_count as f64),
        _ if red.request_count == 0 => None,
        ApmAlertMetric::ErrorRate => Some(red.error_rate * 100.0),
        ApmAlertMetric::P50 => millis(red.p50_time),
        ApmAlertMetric::P90 => millis(red.p90_time),
        ApmAlertMetric::P95 => millis(red.p95_time),
        ApmAlertMetric::P99 => millis(red.p99_time),
        ApmAlertMetric::Average => millis(red.duration_average),
        ApmAlertMetric::Apdex => red.apdex,
        ApmAlertMetric::SloBurnRate | ApmAlertMetric::NewErrorGroups => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_figure_in_the_unit_thresholds_are_set_in() {
        let red = RedSummary {
            request_count: 600,
            error_count: 30,
            error_rate: 0.05,
            p95_time: Some(420_000),
            apdex: Some(0.9),
            ..RedSummary::default()
        };

        assert_eq!(
            metric_value(ApmAlertMetric::ErrorRate, &red, 60.0),
            Some(5.0)
        );
        assert_eq!(metric_value(ApmAlertMetric::P95, &red, 60.0), Some(420.0));
        assert_eq!(
            metric_value(ApmAlertMetric::Throughput, &red, 60.0),
            Some(10.0)
        );
        assert_eq!(
            metric_value(ApmAlertMetric::ErrorCount, &red, 60.0),
            Some(30.0)
        );
        assert_eq!(metric_value(ApmAlertMetric::Apdex, &red, 60.0), Some(0.9));
        assert_eq!(metric_value(ApmAlertMetric::P99, &red, 60.0), None);
    }

    #[test]
    fn traffic_that_stopped_reads_zero_and_has_no_rate() {
        let idle = RedSummary::default();

        assert_eq!(
            metric_value(ApmAlertMetric::RequestCount, &idle, 60.0),
            Some(0.0)
        );
        assert_eq!(
            metric_value(ApmAlertMetric::Throughput, &idle, 60.0),
            Some(0.0)
        );
        assert_eq!(
            metric_value(ApmAlertMetric::ErrorCount, &idle, 60.0),
            Some(0.0)
        );
        assert_eq!(metric_value(ApmAlertMetric::ErrorRate, &idle, 60.0), None);
        assert_eq!(metric_value(ApmAlertMetric::P95, &idle, 60.0), None);
        assert_eq!(metric_value(ApmAlertMetric::Apdex, &idle, 60.0), None);
    }
}
