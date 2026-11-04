// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a service level objective has met, read from the APM buckets when
//! asked: the share of good requests over its window, how much of its error
//! budget is left, how fast it is spending it, and its days one by one.

use std::collections::BTreeMap;

use serde::Serialize;

use super::{ApmQueryService, aggregate::merged_latency};
use crate::{
    domain::apm::{
        BucketDimension, BucketKind, BucketQuery, MergedBucket, QueryResolution,
        ServiceLevelObjective, SloKind,
    },
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

const HOUR_MICROS: i64 = 3_600_000_000;
const DAY_MICROS: i64 = 24 * HOUR_MICROS;

/// A budget spent more than this many times faster than allowed over an hour,
/// or over six hours, puts the objective at risk; so does less than a quarter
/// of it left.
const FAST_BURN: f64 = 14.4;
const SLOW_BURN: f64 = 6.0;
const LOW_BUDGET: f64 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SloState {
    Healthy,
    AtRisk,
    Breached,
    NoData,
}

/// How many times faster than the objective allows the budget is spent.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SloBurnRates {
    pub one_hour: Option<f64>,
    pub six_hours: Option<f64>,
    pub one_day: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SloStatus {
    pub slo: ServiceLevelObjective,
    pub state: SloState,
    /// Good requests over all requests in the window; `None` without requests.
    pub sli: Option<f64>,
    pub good_requests: f64,
    pub total_requests: u64,
    /// The share of the window's error budget not yet spent; below zero once broken.
    pub budget_remaining: Option<f64>,
    pub burn_rates: SloBurnRates,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SloDay {
    pub day_start: TimestampMicros,
    pub sli: Option<f64>,
    pub good_requests: f64,
    pub total_requests: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SloDetailResponse {
    pub status: SloStatus,
    /// Each day of the window, the oldest first.
    pub history: Vec<SloDay>,
}

/// Good and all requests of buckets.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Tally {
    good: f64,
    total: u64,
}

impl Tally {
    fn sli(self) -> Option<f64> {
        (self.total > 0).then(|| (self.good / self.total as f64).clamp(0.0, 1.0))
    }
}

impl ApmQueryService {
    fn objectives(&self) -> Result<&std::sync::Arc<dyn crate::domain::apm::ApmCatalogRepository>> {
        self.catalog
            .as_ref()
            .ok_or_else(|| Error::unavailable("APM service level objectives are not available"))
    }

    /// Every objective of the organization, or of one service, with what it has met now.
    pub async fn slo_statuses(
        &self,
        org_id: &Id,
        service: Option<(&str, &str, &str)>,
        now: TimestampMicros,
    ) -> Result<Vec<SloStatus>> {
        let objectives = self.objectives()?.list_slos(org_id).await?;
        let mut statuses = Vec::new();

        for slo in objectives {
            if let Some((namespace, name, environment)) = service
                && (slo.namespace != namespace
                    || slo.service != name
                    || slo.environment != environment)
            {
                continue;
            }

            let (status, _) = self.evaluate_slo(org_id, slo, now, false).await?;
            statuses.push(status);
        }

        Ok(statuses)
    }

    pub async fn slo_detail(
        &self,
        org_id: &Id,
        id: &Id,
        now: TimestampMicros,
    ) -> Result<SloDetailResponse> {
        let slo = self
            .objectives()?
            .get_slo(org_id, id)
            .await?
            .ok_or_else(|| Error::not_found("SLO not found"))?;
        let (status, history) = self.evaluate_slo(org_id, slo, now, true).await?;

        Ok(SloDetailResponse { status, history })
    }

    /// The burn rate of an objective over `range`, and over its last
    /// `short_micros` too when asked: the lower of the two, so both must pass a
    /// threshold for it to. `None` without requests in either.
    pub async fn slo_burn_rate(
        &self,
        org_id: &Id,
        id: &Id,
        range: TimeRange,
        short_micros: Option<i64>,
    ) -> Result<Option<f64>> {
        let slo = self
            .objectives()?
            .get_slo(org_id, id)
            .await?
            .ok_or_else(|| Error::not_found("SLO not found"))?;
        // Minute buckets are kept for a day; a longer window, such as the
        // three days a ticket-level burn alert reads, is read by the hour.
        let resolution = if range.duration_micros() > self.config.hot_resolution_micros {
            QueryResolution::Hour
        } else {
            QueryResolution::Minute
        };
        let buckets = self.slo_buckets(org_id, &slo, range, resolution).await?;
        let long = burn(&slo, self.tally(&slo, buckets.iter())?);
        let Some(short) = short_micros else {
            return Ok(long);
        };
        let since = TimestampMicros(range.end.0 - short);
        let recent = burn(
            &slo,
            self.tally(
                &slo,
                buckets.iter().filter(|bucket| bucket.bucket_at >= since),
            )?,
        );

        Ok(match (long, recent) {
            (Some(long), Some(recent)) => Some(long.min(recent)),
            _ => None,
        })
    }

    async fn evaluate_slo(
        &self,
        org_id: &Id,
        slo: ServiceLevelObjective,
        now: TimestampMicros,
        with_history: bool,
    ) -> Result<(SloStatus, Vec<SloDay>)> {
        let window = TimeRange::new(TimestampMicros(now.0 - slo.window_micros()), now);
        let recent = TimeRange::new(TimestampMicros(now.0 - DAY_MICROS), now);
        let (hourly, minutes) = tokio::try_join!(
            self.slo_buckets(org_id, &slo, window, QueryResolution::Hour),
            self.slo_buckets(org_id, &slo, recent, QueryResolution::Minute),
        )?;

        let whole = self.tally(&slo, hourly.iter())?;
        let since = |micros: i64| TimestampMicros(now.0 - micros);
        let burn_rates = SloBurnRates {
            one_hour: burn(
                &slo,
                self.tally(
                    &slo,
                    minutes
                        .iter()
                        .filter(|bucket| bucket.bucket_at >= since(HOUR_MICROS)),
                )?,
            ),
            six_hours: burn(
                &slo,
                self.tally(
                    &slo,
                    minutes
                        .iter()
                        .filter(|bucket| bucket.bucket_at >= since(6 * HOUR_MICROS)),
                )?,
            ),
            one_day: burn(&slo, self.tally(&slo, minutes.iter())?),
        };
        let sli = whole.sli();
        let budget_remaining = sli.map(|sli| 1.0 - (1.0 - sli) / slo.budget());
        let state = state_of(sli, budget_remaining, &burn_rates, slo.objective);

        let history = if with_history {
            let mut days = BTreeMap::<i64, Vec<&MergedBucket>>::new();

            for bucket in &hourly {
                days.entry(bucket.bucket_at.0.div_euclid(DAY_MICROS))
                    .or_default()
                    .push(bucket);
            }

            days.into_iter()
                .map(|(day, buckets)| {
                    let tally = self.tally(&slo, buckets.into_iter())?;
                    Ok(SloDay {
                        day_start: TimestampMicros(day * DAY_MICROS),
                        sli: tally.sli(),
                        good_requests: tally.good,
                        total_requests: tally.total,
                    })
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };

        Ok((
            SloStatus {
                slo,
                state,
                sli,
                good_requests: whole.good,
                total_requests: whole.total,
                budget_remaining,
                burn_rates,
            },
            history,
        ))
    }

    /// The buckets of the objective's service, or of its endpoint, over `range`.
    async fn slo_buckets(
        &self,
        org_id: &Id,
        slo: &ServiceLevelObjective,
        range: TimeRange,
        resolution: QueryResolution,
    ) -> Result<Vec<MergedBucket>> {
        let kind = if slo.transaction.is_some() {
            BucketKind::Transaction
        } else {
            BucketKind::Service
        };
        let buckets = self
            .repository
            .query_buckets(&BucketQuery {
                org_id: org_id.clone(),
                range,
                kind,
                resolution,
                namespace: Some(slo.namespace.clone()),
                service_name: Some(slo.service.clone()),
                environment: Some(slo.environment.clone()),
                version: None,
            })
            .await?;

        Ok(match &slo.transaction {
            Some(wanted) => buckets
                .into_iter()
                .filter(|bucket| matches!(&bucket.dimension, BucketDimension::Transaction { transaction, .. } if transaction == wanted))
                .collect(),
            None => buckets,
        })
    }

    /// Good and all requests of `buckets` for `slo`. A failed request is never
    /// good; a latency objective also wants a request within its threshold,
    /// read from the merged histogram, with failures taken out evenly.
    fn tally<'a>(
        &self,
        slo: &ServiceLevelObjective,
        buckets: impl Iterator<Item = &'a MergedBucket>,
    ) -> Result<Tally> {
        let buckets = buckets.collect::<Vec<_>>();
        let total = buckets
            .iter()
            .map(|bucket| bucket.measurements.request_count)
            .sum::<u64>();
        let errors = buckets
            .iter()
            .map(|bucket| bucket.measurements.error_count)
            .sum::<u64>();
        let succeeded = total.saturating_sub(errors) as f64;

        let good = match (slo.kind, slo.latency_threshold) {
            (SloKind::Latency, Some(threshold)) => {
                let schema = &self.config.histogram;
                let (histogram, _) =
                    merged_latency(buckets.iter().map(|bucket| &bucket.measurements), schema)?;
                let observed = histogram.count();

                if observed == 0 {
                    0.0
                } else {
                    succeeded * histogram.count_at_or_below(schema, threshold)? / observed as f64
                }
            }
            _ => succeeded,
        };

        Ok(Tally { good, total })
    }
}

/// How many times faster than allowed a tally spends the budget.
fn burn(slo: &ServiceLevelObjective, tally: Tally) -> Option<f64> {
    tally.sli().map(|sli| (1.0 - sli) / slo.budget())
}

fn state_of(
    sli: Option<f64>,
    remaining: Option<f64>,
    burn: &SloBurnRates,
    objective: f64,
) -> SloState {
    let (Some(sli), Some(remaining)) = (sli, remaining) else {
        return SloState::NoData;
    };

    if sli < objective {
        SloState::Breached
    } else if remaining < LOW_BUDGET
        || burn.one_hour.is_some_and(|rate| rate > FAST_BURN)
        || burn.six_hours.is_some_and(|rate| rate > SLOW_BURN)
    {
        SloState::AtRisk
    } else {
        SloState::Healthy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_an_objective_breached_at_risk_healthy_or_without_data() {
        let calm = SloBurnRates {
            one_hour: Some(1.0),
            six_hours: Some(1.0),
            one_day: Some(1.0),
        };
        assert_eq!(state_of(None, None, &calm, 0.999), SloState::NoData);
        assert_eq!(
            state_of(Some(0.998), Some(-1.0), &calm, 0.999),
            SloState::Breached
        );
        assert_eq!(
            state_of(Some(0.9998), Some(0.8), &calm, 0.999),
            SloState::Healthy
        );
        assert_eq!(
            state_of(Some(0.9991), Some(0.1), &calm, 0.999),
            SloState::AtRisk
        );

        let fast = SloBurnRates {
            one_hour: Some(20.0),
            ..calm.clone()
        };
        assert_eq!(
            state_of(Some(0.9998), Some(0.8), &fast, 0.999),
            SloState::AtRisk
        );
    }

    #[test]
    fn burns_at_the_rate_of_bad_requests_over_the_budget() {
        let slo = ServiceLevelObjective {
            id: Id::from_string("slo"),
            name: "n".into(),
            description: String::new(),
            namespace: "shop".into(),
            service: "checkout".into(),
            environment: "prod".into(),
            transaction: None,
            kind: SloKind::Availability,
            latency_threshold: None,
            objective: 0.99,
            window_days: 30,
            created_by: Id::from_string("u"),
            created_at: TimestampMicros(0),
            updated_by: Id::from_string("u"),
            updated_at: TimestampMicros(0),
        };

        let rate = burn(
            &slo,
            Tally {
                good: 980.0,
                total: 1_000,
            },
        )
        .expect("rate");
        assert!((rate - 2.0).abs() < 1e-9, "{rate}");
        assert_eq!(burn(&slo, Tally::default()), None);
    }
}
