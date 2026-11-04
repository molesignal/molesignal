// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashMap;

use serde::Serialize;

use crate::{
    domain::intake::usage::{
        budget::{BudgetPeriod, BudgetState, IntakeBudget},
        source::{IntakeUsageRepository, UsageDimension, UsageSelection},
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// How a budget stands in the period under way.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetStanding {
    pub period_start: i64,
    /// What the source took in so far in the period; `None` while that is not known.
    pub used_bytes: Option<u64>,
    /// Whether metering covers the whole period. When it does not, `used_bytes` is a lower bound.
    pub used_complete: bool,
    pub used_ratio: Option<f64>,
    pub state: Option<BudgetState>,
}

impl BudgetStanding {
    /// A standing that cannot be told.
    pub fn unknown(budget: &IntakeBudget, now: TimestampMicros) -> Self {
        Self {
            period_start: budget.period.start_of(now).0,
            used_bytes: None,
            used_complete: false,
            used_ratio: None,
            state: None,
        }
    }
}

/// The standing of each budget, in the order given. A source that sent nothing in a period metering
/// covers has used nothing; in a period it does not cover, what it used is not known.
pub async fn budget_standings(
    usage: &dyn IntakeUsageRepository,
    org_id: &Id,
    budgets: &[IntakeBudget],
    now: TimestampMicros,
) -> Result<Vec<BudgetStanding>> {
    if budgets.is_empty() {
        return Ok(Vec::new());
    }

    let first_bucket = usage.first_bucket(org_id).await?;
    let mut used = HashMap::<(UsageDimension, BudgetPeriod), HashMap<String, u64>>::new();

    for budget in budgets {
        let read = (budget.dimension, budget.period);

        if used.contains_key(&read) {
            continue;
        }

        let rows = usage
            .by_source(
                budget.dimension,
                UsageSelection {
                    org_id,
                    start_micros: budget.period.start_of(now).0,
                    end_micros: now.0,
                    source: None,
                },
            )
            .await?;

        used.insert(
            read,
            rows.into_iter()
                .map(|row| (row.source_key, row.intake_bytes))
                .collect(),
        );
    }

    Ok(budgets
        .iter()
        .map(|budget| {
            let start = budget.period.start_of(now).0;
            let complete = first_bucket.is_some_and(|first| first <= start);

            let bytes = used
                .get(&(budget.dimension, budget.period))
                .and_then(|sources| sources.get(&budget.source_key))
                .copied()
                .or(complete.then_some(0));

            BudgetStanding {
                period_start: start,
                used_bytes: bytes,
                used_complete: complete,
                used_ratio: bytes.map(|bytes| budget.used_ratio(bytes)),
                state: bytes.map(|bytes| budget.state_at(bytes)),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::domain::intake::usage::source::{IntakeUsageBucket, IntakeUsageRow, SourceUsage};

    const DAY: i64 = 86_400 * 1_000_000;
    const MIB: u64 = 1024 * 1024;

    struct Usage {
        first: Option<i64>,
        rows: Vec<(UsageDimension, i64, &'static str, u64)>,
    }

    #[async_trait]
    impl IntakeUsageRepository for Usage {
        async fn add(&self, _: &[IntakeUsageRow]) -> Result<()> {
            unreachable!()
        }

        async fn by_source(
            &self,
            dimension: UsageDimension,
            selection: UsageSelection<'_>,
        ) -> Result<Vec<SourceUsage>> {
            let mut sum = HashMap::<&str, u64>::new();

            for (row_dimension, at, key, bytes) in &self.rows {
                if *row_dimension == dimension
                    && *at >= selection.start_micros
                    && *at <= selection.end_micros
                {
                    *sum.entry(key).or_default() += bytes;
                }
            }

            Ok(sum
                .into_iter()
                .map(|(key, bytes)| SourceUsage {
                    source_key: key.into(),
                    intake_bytes: bytes,
                    events: 1,
                })
                .collect())
        }

        async fn hourly(&self, _: UsageSelection<'_>) -> Result<Vec<IntakeUsageBucket>> {
            unreachable!()
        }

        async fn first_bucket(&self, _: &Id) -> Result<Option<i64>> {
            Ok(self.first)
        }

        async fn delete_before(&self, _: i64, _: i64) -> Result<u64> {
            unreachable!()
        }
    }

    fn budget(key: &str, period: BudgetPeriod, limit_size: u64) -> IntakeBudget {
        IntakeBudget {
            id: Id::from_string(key),
            org_id: Id::from_string("org-1"),
            dimension: UsageDimension::Service,
            source_key: key.into(),
            period,
            limit_size,
            warn_percent: 80,
            notify_user_ids: vec![Id::from_string("u-1")],
            created_by: Id::from_string("u-1"),
            updated_by: Id::from_string("u-1"),
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn at(days: i64) -> TimestampMicros {
        // 2026-01-01 and the days after it.
        TimestampMicros(1_767_225_600_000_000 + days * DAY)
    }

    #[tokio::test]
    async fn each_budget_reads_its_own_period() {
        let now = TimestampMicros(at(9).0 + DAY / 2);

        let usage = Usage {
            first: Some(at(0).0),
            rows: vec![
                (UsageDimension::Service, at(3).0, "a", 50 * MIB),
                (UsageDimension::Service, at(9).0, "a", 10 * MIB),
                (UsageDimension::Stream, at(9).0, "a", 999 * MIB),
            ],
        };

        let budgets = [
            budget("a", BudgetPeriod::Day, 100),
            budget("a", BudgetPeriod::Month, 50),
        ];

        let standings = budget_standings(&usage, &Id::from_string("org-1"), &budgets, now)
            .await
            .unwrap();

        assert_eq!(standings[0].period_start, at(9).0);
        assert_eq!(standings[0].used_bytes, Some(10 * MIB));
        assert_eq!(standings[0].state, Some(BudgetState::Ok));
        assert_eq!(standings[0].used_ratio, Some(0.1));
        assert!(standings[0].used_complete);

        assert_eq!(standings[1].period_start, at(0).0);
        assert_eq!(standings[1].used_bytes, Some(60 * MIB));
        assert_eq!(standings[1].state, Some(BudgetState::Exceeded));
    }

    #[tokio::test]
    async fn silence_is_zero_only_where_metering_covers_the_period() {
        let now = at(9);
        let budgets = [budget("quiet", BudgetPeriod::Month, 100)];

        let covered = Usage {
            first: Some(at(0).0),
            rows: Vec::new(),
        };

        let standing = budget_standings(&covered, &Id::from_string("org-1"), &budgets, now)
            .await
            .unwrap()
            .remove(0);

        assert_eq!(standing.used_bytes, Some(0));
        assert_eq!(standing.state, Some(BudgetState::Ok));
        assert!(standing.used_complete);

        let later_start = Usage {
            first: Some(at(5).0),
            rows: Vec::new(),
        };

        let standing = budget_standings(&later_start, &Id::from_string("org-1"), &budgets, now)
            .await
            .unwrap()
            .remove(0);

        assert_eq!(standing.used_bytes, None);
        assert_eq!(standing.used_ratio, None);
        assert_eq!(standing.state, None);
        assert!(!standing.used_complete);

        let never = Usage {
            first: None,
            rows: Vec::new(),
        };

        let standing = budget_standings(&never, &Id::from_string("org-1"), &budgets, now)
            .await
            .unwrap()
            .remove(0);

        assert_eq!(standing.used_bytes, None);
    }

    #[tokio::test]
    async fn a_lower_bound_past_the_limit_is_already_over() {
        let now = at(9);
        let budgets = [budget("a", BudgetPeriod::Month, 10)];

        let usage = Usage {
            first: Some(at(5).0),
            rows: vec![(UsageDimension::Service, at(6).0, "a", 11 * MIB)],
        };

        let standing = budget_standings(&usage, &Id::from_string("org-1"), &budgets, now)
            .await
            .unwrap()
            .remove(0);

        assert_eq!(standing.used_bytes, Some(11 * MIB));
        assert!(!standing.used_complete);
        assert_eq!(standing.state, Some(BudgetState::Exceeded));
    }
}
