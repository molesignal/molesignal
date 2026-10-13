// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The bars of an explorer histogram: what a client asks to have counted, and
//! what it is answered with.
//!
//! An explorer draws how many results a query matches over time, in bars of a
//! round length of time that begin on the reader's clock. The client chooses
//! the bars for the width it has to draw in, and sends how long one is and
//! where the first begins. The server counts into exactly those bars, so the
//! two never disagree about where a bar lies.

use std::{future::Future, time::Duration};

use serde::Serialize;
use serde_json::Value;

use crate::shared::{Error, Result};

/// A count gives up before the web client's own request limit of 30 s, so the
/// chart can fall back to the results that are loaded instead of failing.
const TIME_BUDGET: Duration = Duration::from_secs(20);
/// More bars than a plot is wide enough to draw.
const MAX_BUCKETS: i64 = 512;
/// The shortest bar a client draws.
const MIN_BUCKET_MICROS: i64 = 1_000;

/// Result columns of a counting statement. A result column inherits the
/// masking of a stream field of the same name, so these are names no field has.
pub(super) const BUCKET_COLUMN: &str = "__histogram_bucket";
pub(super) const TOTAL_COLUMN: &str = "__histogram_total";
pub(super) const ERRORS_COLUMN: &str = "__histogram_errors";

/// The bars a client asks for: each `bucket_micros` long, the first beginning
/// at `origin_micros`, holding what happened in `[from, to)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BucketPlan {
    pub(super) from: i64,
    pub(super) to: i64,
    bucket_micros: i64,
    origin_micros: i64,
}

/// What one bar holds. `index` counts bars from the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BarCount {
    pub(super) index: i64,
    pub(super) total: u64,
    pub(super) errors: u64,
}

/// What a counting statement returned.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Counts {
    Bars {
        bars: Vec<BarCount>,
        /// Errors came back counted apart from the rest.
        split: bool,
    },
    /// The bars came back masked: time is a masked field of the stream.
    TimeMasked,
}

/// Why a histogram has no counts. The client words the reason for its reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Unavailable {
    /// Counting took longer than the time budget.
    TimeBudget,
    /// Too many queries are running for this role.
    Busy,
    /// The range holds more rows than one query may read.
    ScanLimit,
    /// Time is a masked field of the stream.
    TimestampMasked,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct HistogramBucket {
    start_micros: i64,
    total: u64,
    errors: u64,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct HistogramResponse {
    bucket_micros: i64,
    origin_micros: i64,
    /// The bars that hold at least one result, oldest first.
    buckets: Vec<HistogramBucket>,
    /// Whether errors are counted apart from the rest.
    split: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable: Option<Unavailable>,
}

impl BucketPlan {
    pub(super) fn new(
        from: Option<i64>,
        to: Option<i64>,
        bucket_micros: Option<i64>,
        origin_micros: Option<i64>,
    ) -> Result<Self> {
        let (Some(from), Some(to)) = (from, to) else {
            return Err(Error::invalid("histogram range is required"));
        };

        if to <= from {
            return Err(Error::invalid(
                "histogram range end must be greater than start",
            ));
        }

        let bucket_micros = bucket_micros
            .filter(|bucket| *bucket >= MIN_BUCKET_MICROS)
            .ok_or_else(|| Error::invalid("histogram bucket_micros must be at least 1000"))?;

        // A bar that began after the range would leave its start uncounted, and
        // the index of a moment before the first bar would be negative.
        let origin_micros = origin_micros
            .filter(|origin| *origin <= from)
            .ok_or_else(|| {
                Error::invalid("histogram origin_micros must not be after the range start")
            })?;

        // The span is positive: the first bar begins no later than the range.
        to.checked_sub(origin_micros)
            .map(|span| (span - 1) / bucket_micros + 1)
            .filter(|bars| *bars <= MAX_BUCKETS)
            .ok_or_else(|| Error::invalid("histogram asks for too many bars"))?;

        Ok(Self {
            from,
            to,
            bucket_micros,
            origin_micros,
        })
    }

    /// How many bars there are from the first to the one the range ends in.
    pub(super) fn bars(&self) -> usize {
        // Positive and no more than `MAX_BUCKETS`: `new` refuses anything else.
        ((self.to - self.origin_micros - 1) / self.bucket_micros + 1) as usize
    }

    /// SQL for the index of the bar that a moment, as an expression in
    /// microseconds, falls in. No moment in the range lies before the first
    /// bar, so integer division rounds the way a bar is counted.
    pub(super) fn bucket_sql(&self, micros: &str) -> String {
        format!(
            "({micros} - {}) / {}",
            self.origin_micros, self.bucket_micros
        )
    }

    /// The bar a moment falls in, or `None` outside the range.
    pub(super) fn index_of(&self, micros: i64) -> Option<i64> {
        (self.from..self.to)
            .contains(&micros)
            .then(|| (micros - self.origin_micros) / self.bucket_micros)
    }

    pub(super) fn counted(&self, mut bars: Vec<BarCount>, split: bool) -> HistogramResponse {
        bars.sort_by_key(|bar| bar.index);

        let buckets = bars
            .into_iter()
            .filter(|bar| bar.total > 0)
            .filter_map(|bar| {
                let start_micros = bar
                    .index
                    .checked_mul(self.bucket_micros)?
                    .checked_add(self.origin_micros)?;

                // A bar outside the range is not one that was asked for.
                (start_micros < self.to && bar.index >= 0).then_some(HistogramBucket {
                    start_micros,
                    total: bar.total,
                    errors: if split { bar.errors.min(bar.total) } else { 0 },
                })
            })
            .collect();

        HistogramResponse {
            bucket_micros: self.bucket_micros,
            origin_micros: self.origin_micros,
            buckets,
            split,
            unavailable: None,
        }
    }

    pub(super) fn unavailable(&self, reason: Unavailable) -> HistogramResponse {
        HistogramResponse {
            bucket_micros: self.bucket_micros,
            origin_micros: self.origin_micros,
            buckets: Vec::new(),
            split: false,
            unavailable: Some(reason),
        }
    }
}

/// Whether a count may read `rows` rows. One query reads no more than
/// `[querier].max_scan_rows`, and a count is one query.
pub(super) fn within_scan_limit(rows: u64) -> bool {
    let limit = crate::config::get().querier.max_scan_rows;

    limit == 0 || rows <= limit
}

/// Runs a count inside the time budget. `None` when it took too long: the
/// count is dropped, which stops the scan behind it.
pub(super) async fn within_budget<T>(count: impl Future<Output = Result<T>>) -> Result<Option<T>> {
    within(TIME_BUDGET, count).await
}

async fn within<T>(budget: Duration, count: impl Future<Output = Result<T>>) -> Result<Option<T>> {
    match tokio::time::timeout(budget, count).await {
        Ok(counted) => counted.map(Some),
        Err(_) => Ok(None),
    }
}

/// Reads the bars out of the rows of a counting statement, which names its
/// columns [`BUCKET_COLUMN`], [`TOTAL_COLUMN`] and [`ERRORS_COLUMN`].
///
/// A masked value comes back as text. So a bar index that is not a number
/// means time is masked and there is nothing to draw over it, and an error
/// count that is not a number means the field errors are told by is masked:
/// the bars are then whole.
pub(super) fn read_counts(columns: &[String], rows: &[Vec<Value>]) -> Result<Counts> {
    if rows.is_empty() {
        return Ok(Counts::Bars {
            bars: Vec::new(),
            split: true,
        });
    }

    let position = |name: &str| columns.iter().position(|column| column == name);

    let (Some(bucket), Some(total)) = (position(BUCKET_COLUMN), position(TOTAL_COLUMN)) else {
        return Err(Error::internal("histogram statement returned no bars"));
    };

    let errors = position(ERRORS_COLUMN);
    let mut split = errors.is_some();
    let mut bars = Vec::with_capacity(rows.len());

    for row in rows {
        let Some(index) = row.get(bucket).and_then(Value::as_i64) else {
            return Ok(Counts::TimeMasked);
        };

        let total = row
            .get(total)
            .and_then(Value::as_u64)
            .ok_or_else(|| Error::internal("histogram statement returned no count"))?;

        let errors = errors
            .and_then(|errors| row.get(errors))
            .and_then(Value::as_u64);

        split &= errors.is_some();

        bars.push(BarCount {
            index,
            total,
            errors: errors.unwrap_or_default(),
        });
    }

    Ok(Counts::Bars { bars, split })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const MINUTE: i64 = 60_000_000;

    fn plan() -> BucketPlan {
        // Ten past to twenty-five past, in bars of five minutes from five past.
        BucketPlan::new(
            Some(10 * MINUTE + 1),
            Some(25 * MINUTE),
            Some(5 * MINUTE),
            Some(10 * MINUTE),
        )
        .expect("plan")
    }

    fn columns() -> Vec<String> {
        [BUCKET_COLUMN, TOTAL_COLUMN, ERRORS_COLUMN]
            .map(String::from)
            .to_vec()
    }

    #[test]
    fn a_plan_needs_a_range_and_bars_that_cover_it_from_the_start() {
        // A tenth of a second in bars of a millisecond is a plan. Each line
        // below takes one thing away from it.
        assert!(BucketPlan::new(Some(0), Some(100_000), Some(1_000), Some(0)).is_ok());
        assert!(BucketPlan::new(None, Some(100_000), Some(1_000), Some(0)).is_err());
        assert!(BucketPlan::new(Some(0), None, Some(1_000), Some(0)).is_err());
        assert!(BucketPlan::new(Some(100_000), Some(100_000), Some(1_000), Some(0)).is_err());
        assert!(BucketPlan::new(Some(0), Some(100_000), None, Some(0)).is_err());
        assert!(BucketPlan::new(Some(0), Some(100_000), Some(999), Some(0)).is_err());
        assert!(BucketPlan::new(Some(0), Some(100_000), Some(1_000), None).is_err());
        // The first bar may begin before the range does, and not after it.
        assert!(BucketPlan::new(Some(0), Some(100_000), Some(1_000), Some(-500)).is_ok());
        assert!(BucketPlan::new(Some(0), Some(100_000), Some(1_000), Some(1)).is_err());
    }

    #[test]
    fn a_plan_is_refused_when_it_asks_for_more_bars_than_can_be_drawn() {
        assert!(BucketPlan::new(Some(0), Some(512_000), Some(1_000), Some(0)).is_ok());
        assert!(BucketPlan::new(Some(0), Some(512_001), Some(1_000), Some(0)).is_err());
        assert!(BucketPlan::new(Some(0), Some(i64::MAX), Some(1_000), Some(i64::MIN)).is_err());
    }

    #[test]
    fn a_moment_falls_in_the_bar_that_holds_it_and_in_none_outside_the_range() {
        let plan = plan();
        assert_eq!(plan.index_of(10 * MINUTE), None);
        assert_eq!(plan.index_of(10 * MINUTE + 1), Some(0));
        assert_eq!(plan.index_of(15 * MINUTE - 1), Some(0));
        assert_eq!(plan.index_of(15 * MINUTE), Some(1));
        assert_eq!(plan.index_of(25 * MINUTE - 1), Some(2));
        assert_eq!(plan.index_of(25 * MINUTE), None);
        assert_eq!(plan.bars(), 3);
        assert_eq!(
            plan.bucket_sql("CAST(_timestamp AS BIGINT)"),
            "(CAST(_timestamp AS BIGINT) - 600000000) / 300000000"
        );
    }

    #[test]
    fn the_answer_names_each_bar_by_where_it_begins_oldest_first() {
        let bar = |index, total, errors| BarCount {
            index,
            total,
            errors,
        };
        let answer = plan().counted(
            vec![bar(2, 4, 1), bar(0, 7, 0), bar(1, 0, 0), bar(3, 9, 9)],
            true,
        );

        assert_eq!(
            serde_json::to_value(&answer).unwrap(),
            json!({
                "bucket_micros": 5 * MINUTE,
                "origin_micros": 10 * MINUTE,
                // The empty bar is left out, and so is the one past the range.
                "buckets": [
                    { "start_micros": 10 * MINUTE, "total": 7, "errors": 0 },
                    { "start_micros": 20 * MINUTE, "total": 4, "errors": 1 },
                ],
                "split": true,
            })
        );
    }

    #[test]
    fn bars_that_are_not_split_count_no_errors() {
        let answer = plan().counted(
            vec![BarCount {
                index: 0,
                total: 3,
                errors: 2,
            }],
            false,
        );

        assert_eq!(
            serde_json::to_value(&answer).unwrap()["buckets"],
            json!([{ "start_micros": 10 * MINUTE, "total": 3, "errors": 0 }])
        );
    }

    #[test]
    fn an_answer_without_counts_says_why() {
        assert_eq!(
            serde_json::to_value(plan().unavailable(Unavailable::TimeBudget)).unwrap(),
            json!({
                "bucket_micros": 5 * MINUTE,
                "origin_micros": 10 * MINUTE,
                "buckets": [],
                "split": false,
                "unavailable": "time_budget",
            })
        );
    }

    #[test]
    fn counts_are_read_from_the_named_columns() {
        let rows = vec![
            vec![json!(1), json!(5), json!(2)],
            vec![json!(0), json!(3), json!(0)],
        ];

        assert_eq!(
            read_counts(&columns(), &rows).unwrap(),
            Counts::Bars {
                bars: vec![
                    BarCount {
                        index: 1,
                        total: 5,
                        errors: 2
                    },
                    BarCount {
                        index: 0,
                        total: 3,
                        errors: 0
                    },
                ],
                split: true,
            }
        );
        assert_eq!(
            read_counts(&[], &[]).unwrap(),
            Counts::Bars {
                bars: Vec::new(),
                split: true
            }
        );
    }

    #[test]
    fn masked_errors_leave_whole_bars_and_masked_time_leaves_none() {
        let masked_errors = vec![vec![json!(1), json!(5), json!("***")]];

        assert_eq!(
            read_counts(&columns(), &masked_errors).unwrap(),
            Counts::Bars {
                bars: vec![BarCount {
                    index: 1,
                    total: 5,
                    errors: 0
                }],
                split: false,
            }
        );

        let masked_time = vec![vec![json!("***"), json!(5), json!(2)]];
        assert_eq!(
            read_counts(&columns(), &masked_time).unwrap(),
            Counts::TimeMasked
        );
    }

    #[test]
    fn rows_without_the_bar_columns_are_an_error() {
        let rows = vec![vec![json!(1)]];
        assert!(read_counts(&["other".to_string()], &rows).is_err());
    }

    #[tokio::test]
    async fn a_count_past_the_time_budget_is_dropped() {
        let slow = async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            Ok(1)
        };

        assert_eq!(within(Duration::from_millis(10), slow).await.unwrap(), None);
        assert_eq!(within_budget(async { Ok(1) }).await.unwrap(), Some(1));
        assert!(
            within_budget(async { Err::<i32, _>(Error::invalid("no")) })
                .await
                .is_err()
        );
    }
}
