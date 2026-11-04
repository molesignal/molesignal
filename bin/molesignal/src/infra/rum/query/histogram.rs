// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The bars of the histogram over an explorer's results.
//!
//! The page chooses the bars for the width it has to draw in and sends how long one is and where
//! the first begins; the server counts into exactly those bars, so the two never disagree about
//! where a bar lies. The answer has the shape the other explorers answer with.

use serde::Serialize;
use serde_json::Value;

use super::statements::{BUCKET_COLUMN, TOTAL_COLUMN, TROUBLE_COLUMN};
use crate::shared::{Error, Result};

/// More bars than a plot is wide enough to draw.
pub(super) const MAX_BUCKETS: i64 = 512;
/// The shortest bar a page draws.
const MIN_BUCKET_MICROS: i64 = 1_000;

/// The bars a page asks for: each `bucket_micros` long, the first beginning at `origin_micros`,
/// holding what happened in `[from, to)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BucketPlan {
    pub from: i64,
    pub to: i64,
    pub bucket_micros: i64,
    pub origin_micros: i64,
}

/// Why a histogram has no bars. The page words the reason for its reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unavailable {
    /// Counting took longer than the time budget.
    TimeBudget,
    /// Too many queries are running for this role.
    Busy,
    /// Time is a masked field of the stream.
    TimestampMasked,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bar {
    pub start_time: i64,
    pub total: u64,
    pub errors: u64,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Histogram {
    pub bucket_time: i64,
    pub origin_time: i64,
    /// The bars that hold at least one result, oldest first.
    pub buckets: Vec<Bar>,
    /// Whether the troubled results are counted apart from the rest.
    pub split: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<Unavailable>,
}

impl BucketPlan {
    pub fn new(
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

        // A bar that began after the range would leave its start uncounted, and the index of a
        // moment before the first bar would be negative.
        let origin_micros = origin_micros.unwrap_or(from);

        if origin_micros > from {
            return Err(Error::invalid(
                "histogram origin_micros must not be after the range start",
            ));
        }

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

    /// Where bar `index` begins. `None` for a bar outside the range: it was not asked for.
    pub(super) fn start_of(&self, index: i64) -> Option<i64> {
        let start = index
            .checked_mul(self.bucket_micros)?
            .checked_add(self.origin_micros)?;

        (index >= 0 && start < self.to).then_some(start)
    }

    pub fn unavailable(&self, reason: Unavailable) -> Histogram {
        Histogram {
            unavailable: Some(reason),
            ..self.empty()
        }
    }

    /// The answer for a stream nothing was ever written to: no bars, and nothing missing.
    pub fn empty(&self) -> Histogram {
        Histogram {
            bucket_time: self.bucket_micros,
            origin_time: self.origin_micros,
            buckets: Vec::new(),
            split: false,
            unavailable: None,
        }
    }

    /// The bars out of the rows of [`super::statements::histogram_sql`].
    ///
    /// A masked value comes back as text. So a bar index that is not a number means time is
    /// masked and there is nothing to draw over it, and a count of the troubled that is not a
    /// number means what they are told by is masked: the bars are then whole.
    pub fn counted(&self, columns: &[String], rows: &[Vec<Value>]) -> Result<Histogram> {
        let position = |name: &str| columns.iter().position(|column| column == name);

        let (Some(bucket), Some(total)) = (position(BUCKET_COLUMN), position(TOTAL_COLUMN)) else {
            // A statement that found no rows says nothing of its columns: there are no bars.
            if rows.is_empty() {
                return Ok(self.empty());
            }

            return Err(Error::internal("histogram statement returned no bars"));
        };

        let errors = position(TROUBLE_COLUMN);
        let mut split = errors.is_some();
        let mut bars = Vec::with_capacity(rows.len());

        for row in rows {
            let Some(index) = row.get(bucket).and_then(Value::as_i64) else {
                return Ok(self.unavailable(Unavailable::TimestampMasked));
            };

            let total = row
                .get(total)
                .and_then(Value::as_u64)
                .ok_or_else(|| Error::internal("histogram statement returned no count"))?;

            let errors = errors
                .and_then(|errors| row.get(errors))
                .and_then(Value::as_u64);

            split &= errors.is_some();

            bars.push((index, total, errors.unwrap_or_default()));
        }

        bars.sort_by_key(|(index, _, _)| *index);

        let buckets = bars
            .into_iter()
            .filter(|(_, total, _)| *total > 0)
            .filter_map(|(index, total, errors)| {
                Some(Bar {
                    start_time: self.start_of(index)?,
                    total,
                    errors: if split { errors.min(total) } else { 0 },
                })
            })
            .collect();

        Ok(Histogram {
            bucket_time: self.bucket_micros,
            origin_time: self.origin_micros,
            buckets,
            split,
            unavailable: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const MINUTE: i64 = 60_000_000;

    fn plan() -> BucketPlan {
        // Ten past to twenty-five past, in bars of five minutes from ten past.
        BucketPlan::new(
            Some(10 * MINUTE + 1),
            Some(25 * MINUTE),
            Some(5 * MINUTE),
            Some(10 * MINUTE),
        )
        .expect("plan")
    }

    fn columns() -> Vec<String> {
        [BUCKET_COLUMN, TOTAL_COLUMN, TROUBLE_COLUMN]
            .map(String::from)
            .to_vec()
    }

    #[test]
    fn a_plan_needs_a_range_and_bars_that_cover_it_from_the_start() {
        assert!(BucketPlan::new(Some(0), Some(100_000), Some(1_000), Some(0)).is_ok());
        assert!(BucketPlan::new(None, Some(100_000), Some(1_000), Some(0)).is_err());
        assert!(BucketPlan::new(Some(0), None, Some(1_000), Some(0)).is_err());
        assert!(BucketPlan::new(Some(100_000), Some(100_000), Some(1_000), Some(0)).is_err());
        assert!(BucketPlan::new(Some(0), Some(100_000), None, Some(0)).is_err());
        assert!(BucketPlan::new(Some(0), Some(100_000), Some(999), Some(0)).is_err());
        // Without an origin the first bar begins at the start of the range.
        assert_eq!(
            BucketPlan::new(Some(7_000), Some(100_000), Some(1_000), None)
                .unwrap()
                .origin_micros,
            7_000
        );

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
    fn bars_are_named_by_where_they_begin_oldest_first() {
        let rows = vec![
            vec![json!(2), json!(4), json!(1)],
            vec![json!(0), json!(7), json!(0)],
            vec![json!(1), json!(0), json!(0)],
            vec![json!(3), json!(9), json!(9)],
        ];

        let answer = plan().counted(&columns(), &rows).unwrap();

        // The empty bar is left out, and so is the one past the range.
        assert_eq!(
            serde_json::to_value(&answer).unwrap(),
            json!({
                "bucketTime": 5 * MINUTE,
                "originTime": 10 * MINUTE,
                "buckets": [
                    { "startTime": 10 * MINUTE, "total": 7, "errors": 0 },
                    { "startTime": 20 * MINUTE, "total": 4, "errors": 1 },
                ],
                "split": true,
            })
        );
    }

    #[test]
    fn rows_without_a_count_of_the_troubled_leave_whole_bars() {
        let columns = [BUCKET_COLUMN, TOTAL_COLUMN].map(String::from);
        let answer = plan()
            .counted(&columns, &[vec![json!(0), json!(3)]])
            .unwrap();

        assert!(!answer.split);
        assert_eq!(answer.buckets[0].errors, 0);
    }

    #[test]
    fn masked_errors_leave_whole_bars_and_masked_time_leaves_none() {
        let masked_errors = vec![vec![json!(1), json!(5), json!("***")]];
        let answer = plan().counted(&columns(), &masked_errors).unwrap();

        assert!(!answer.split);
        assert_eq!(answer.buckets[0].total, 5);
        assert_eq!(answer.buckets[0].errors, 0);

        let masked_time = vec![vec![json!("***"), json!(5), json!(2)]];
        let answer = plan().counted(&columns(), &masked_time).unwrap();

        assert_eq!(answer.unavailable, Some(Unavailable::TimestampMasked));
        assert!(answer.buckets.is_empty());
    }

    #[test]
    fn no_rows_are_no_bars_and_rows_without_the_columns_are_an_error() {
        let answer = plan().counted(&columns(), &[]).unwrap();

        assert!(answer.buckets.is_empty());
        assert!(answer.split);
        assert!(
            plan()
                .counted(&["other".to_string()], &[vec![json!(1)]])
                .is_err()
        );
    }

    #[test]
    fn a_statement_that_found_no_rows_says_nothing_of_its_columns() {
        // DataFusion gives no batches for it, so the result has no columns at all.
        let answer = plan().counted(&[], &[]).unwrap();

        assert!(answer.buckets.is_empty());
        assert_eq!(answer.unavailable, None);
        assert_eq!(answer.bucket_time, 5 * MINUTE);
    }

    #[test]
    fn a_stream_nothing_was_written_to_has_no_bars_and_nothing_missing() {
        let answer = plan().empty();

        assert!(answer.buckets.is_empty());
        assert_eq!(answer.unavailable, None);
        assert_eq!(answer.bucket_time, 5 * MINUTE);
    }

    #[test]
    fn an_answer_without_counts_says_why() {
        assert_eq!(
            serde_json::to_value(plan().unavailable(Unavailable::TimeBudget)).unwrap(),
            json!({
                "bucketTime": 5 * MINUTE,
                "originTime": 10 * MINUTE,
                "buckets": [],
                "split": false,
                "unavailable": "time_budget",
            })
        );
    }
}
