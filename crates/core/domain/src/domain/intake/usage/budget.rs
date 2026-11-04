// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A budget set on one source, for a calendar day or month in UTC. A budget only tells people:
//! intake answers and stores the same whatever the budget says.

use async_trait::async_trait;
use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc};
use serde::{Deserialize, Serialize};

use super::source::{UsageDimension, budgetable};
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

const MIB: u64 = 1024 * 1024;

/// The largest budget, in MiB: 1 PiB. Same as the CHECK constraint of `intake_budgets`.
pub const MAX_BUDGET_LIMIT_SIZE: u64 = 1 << 30;
/// The percentage of a budget at which people are warned unless they say otherwise.
pub const DEFAULT_BUDGET_WARN_PERCENT: u8 = 80;
/// The highest percentage to warn at: at 100 the budget is used up, which is told anyway.
pub const MAX_BUDGET_WARN_PERCENT: u8 = 99;
/// The most people one budget can name.
pub const MAX_BUDGET_NOTIFY_USERS: usize = 50;

/// The span a budget is counted over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetPeriod {
    Day,
    Month,
}

impl BudgetPeriod {
    pub const ALL: [Self; 2] = [Self::Day, Self::Month];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Month => "month",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "day" => Ok(Self::Day),
            "month" => Ok(Self::Month),
            other => Err(Error::invalid(format!("unknown budget period `{other}`"))),
        }
    }

    /// The start of the calendar day or month, in UTC, that `at` falls in.
    pub fn start_of(self, at: TimestampMicros) -> TimestampMicros {
        let moment = DateTime::<Utc>::from_timestamp_micros(at.0).unwrap_or_default();

        let date = match self {
            Self::Day => moment.date_naive(),
            Self::Month => NaiveDate::from_ymd_opt(moment.year(), moment.month(), 1)
                .unwrap_or_else(|| moment.date_naive()),
        };

        let start = date
            .and_hms_opt(0, 0, 0)
            .unwrap_or_else(|| moment.naive_utc().with_nanosecond(0).unwrap_or_default());

        TimestampMicros(start.and_utc().timestamp_micros())
    }
}

/// Where a source stands against its budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetState {
    Ok,
    Warning,
    Exceeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntakeBudget {
    pub id: Id,
    pub org_id: Id,
    pub dimension: UsageDimension,
    pub source_key: String,
    pub period: BudgetPeriod,
    /// In MiB.
    pub limit_size: u64,
    /// The percentage to warn at; 0 warns at none.
    pub warn_percent: u8,
    pub notify_user_ids: Vec<Id>,
    pub created_by: Id,
    pub updated_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl IntakeBudget {
    pub fn limit_bytes(&self) -> u64 {
        self.limit_size.saturating_mul(MIB)
    }

    pub fn state_at(&self, used_bytes: u64) -> BudgetState {
        let limit = u128::from(self.limit_bytes());
        let used = u128::from(used_bytes);

        if used >= limit {
            BudgetState::Exceeded
        } else if self.warn_percent > 0 && used * 100 >= limit * u128::from(self.warn_percent) {
            BudgetState::Warning
        } else {
            BudgetState::Ok
        }
    }

    pub fn used_ratio(&self, used_bytes: u64) -> f64 {
        used_bytes as f64 / self.limit_bytes().max(1) as f64
    }

    pub fn validate(&self) -> Result<()> {
        if !budgetable(self.dimension, &self.source_key) {
            return Err(Error::invalid(format!(
                "a budget cannot be set on the {} source `{}`",
                self.dimension.as_str(),
                self.source_key
            )));
        }

        if !(1..=MAX_BUDGET_LIMIT_SIZE).contains(&self.limit_size) {
            return Err(Error::invalid(format!(
                "limit_size must be between 1 and {MAX_BUDGET_LIMIT_SIZE} MiB"
            )));
        }

        if self.warn_percent > MAX_BUDGET_WARN_PERCENT {
            return Err(Error::invalid(format!(
                "warn_percent must be between 0 and {MAX_BUDGET_WARN_PERCENT}"
            )));
        }

        if self.notify_user_ids.is_empty() || self.notify_user_ids.len() > MAX_BUDGET_NOTIFY_USERS {
            return Err(Error::invalid(format!(
                "a budget must name between 1 and {MAX_BUDGET_NOTIFY_USERS} people to notify"
            )));
        }

        Ok(())
    }
}

#[async_trait]
pub trait IntakeBudgetRepository: Send + Sync {
    async fn list(&self, org_id: &Id) -> Result<Vec<IntakeBudget>>;

    /// Sets the budget of its source. A source that has one keeps its id and who created it.
    async fn upsert(&self, budget: IntakeBudget) -> Result<IntakeBudget>;

    /// Removes a budget and returns what it was.
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<IntakeBudget>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget(limit_size: u64, warn_percent: u8) -> IntakeBudget {
        IntakeBudget {
            id: Id::from_string("b-1"),
            org_id: Id::from_string("org-1"),
            dimension: UsageDimension::Service,
            source_key: "checkout".into(),
            period: BudgetPeriod::Day,
            limit_size,
            warn_percent,
            notify_user_ids: vec![Id::from_string("u-1")],
            created_by: Id::from_string("u-1"),
            updated_by: Id::from_string("u-1"),
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn micros(text: &str) -> TimestampMicros {
        TimestampMicros(
            DateTime::parse_from_rfc3339(text)
                .unwrap()
                .timestamp_micros(),
        )
    }

    #[test]
    fn a_period_starts_at_midnight_utc() {
        let at = micros("2026-03-31T23:59:59.5Z");

        assert_eq!(
            BudgetPeriod::Day.start_of(at),
            micros("2026-03-31T00:00:00Z")
        );
        assert_eq!(
            BudgetPeriod::Month.start_of(at),
            micros("2026-03-01T00:00:00Z")
        );

        let new_year = micros("2027-01-01T00:00:00Z");

        assert_eq!(BudgetPeriod::Day.start_of(new_year), new_year);
        assert_eq!(BudgetPeriod::Month.start_of(new_year), new_year);
    }

    #[test]
    fn the_state_follows_the_share_used() {
        let budget = budget(100, 80);
        let mib = MIB;

        assert_eq!(budget.state_at(0), BudgetState::Ok);
        assert_eq!(budget.state_at(80 * mib - 1), BudgetState::Ok);
        assert_eq!(budget.state_at(80 * mib), BudgetState::Warning);
        assert_eq!(budget.state_at(100 * mib - 1), BudgetState::Warning);
        assert_eq!(budget.state_at(100 * mib), BudgetState::Exceeded);
        assert_eq!(budget.state_at(u64::MAX), BudgetState::Exceeded);
    }

    #[test]
    fn no_warning_percent_goes_straight_to_exceeded() {
        let budget = budget(100, 0);

        assert_eq!(budget.state_at(99 * MIB), BudgetState::Ok);
        assert_eq!(budget.state_at(100 * MIB), BudgetState::Exceeded);
    }

    #[test]
    fn validation_keeps_the_table_constraints() {
        assert!(budget(1, 0).validate().is_ok());
        assert!(budget(MAX_BUDGET_LIMIT_SIZE, 99).validate().is_ok());
        assert!(budget(0, 80).validate().is_err());
        assert!(budget(MAX_BUDGET_LIMIT_SIZE + 1, 80).validate().is_err());
        assert!(budget(1, 100).validate().is_err());

        let mut nobody = budget(1, 80);
        nobody.notify_user_ids.clear();
        assert!(nobody.validate().is_err());

        let mut unnamed = budget(1, 80);
        unnamed.source_key = String::new();
        assert!(unnamed.validate().is_err());
    }
}
