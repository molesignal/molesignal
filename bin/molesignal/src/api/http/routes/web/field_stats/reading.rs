// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Reads the rows the statements of a field analysis return.
//!
//! A masked value comes back as text. So a count of the field that is not a
//! number means the field is masked for the reader, and nothing more is read
//! of it.

use serde_json::Value;

use super::{
    FieldKind, Spread, TopValue, TrendSeries, ValueRange,
    statements::{
        AVG, BAR, COUNT, DISTINCT, MAX, MEDIAN, MIN, MOMENT_2, MOMENT_3, MOMENT_4, PRESENT, Q1, Q3,
        STDDEV_POP, STDDEV_SAMP, STRETCH, TOTAL, VALUE,
    },
};
use crate::shared::{Error, Result};

/// What the summary statement returned.
#[derive(Debug, Default, PartialEq)]
pub(super) struct SummaryRow {
    pub(super) total: u64,
    pub(super) present: u64,
    pub(super) distinct: u64,
    pub(super) range: Option<ValueRange>,
    pub(super) spread: Option<Spread>,
    /// What a number runs from and to, for its bars to be planned over.
    pub(super) bounds: Option<(f64, f64)>,
}

/// What the bars statement returned: how many values each bar holds, and what
/// all of them add up to about the mean.
#[derive(Debug, PartialEq)]
pub(super) struct Bars {
    pub(super) counts: Vec<u64>,
    pub(super) moments: Moments,
}

/// The sums of the second, third and fourth powers of the values' distances
/// from their mean.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Moments {
    count: u64,
    second: f64,
    third: f64,
    fourth: f64,
}

impl Moments {
    /// The variance of the population, where the values differ at all.
    fn variance(&self) -> Option<f64> {
        let variance = self.second / self.count as f64;

        (self.count > 0 && variance.is_finite() && variance > 0.0).then_some(variance)
    }

    /// To which side the values tail off: the third standardised moment.
    pub(super) fn skewness(&self) -> Option<f64> {
        let variance = self.variance()?;
        let skewness = self.third / self.count as f64 / variance.powf(1.5);

        skewness.is_finite().then_some(skewness)
    }

    /// How heavy the tails are beside a normal distribution's, whose fourth
    /// standardised moment is 3.
    pub(super) fn excess_kurtosis(&self) -> Option<f64> {
        let variance = self.variance()?;
        let kurtosis = self.fourth / self.count as f64 / (variance * variance) - 3.0;

        kurtosis.is_finite().then_some(kurtosis)
    }
}

struct Row<'a> {
    columns: &'a [String],
    values: &'a [Value],
}

impl<'a> Row<'a> {
    fn get(&self, name: &str) -> Option<&'a Value> {
        let position = self.columns.iter().position(|column| column == name)?;

        self.values.get(position)
    }

    fn count(&self, name: &str) -> Option<u64> {
        self.get(name).and_then(Value::as_u64)
    }

    /// A statistic as a number a reader can be shown: an aggregate of nothing
    /// is null, and one that overflowed is not finite.
    fn number(&self, name: &str) -> Option<f64> {
        self.get(name)
            .and_then(Value::as_f64)
            .filter(|number| number.is_finite())
    }

    fn text(&self, name: &str) -> Option<&'a str> {
        self.get(name).and_then(Value::as_str)
    }
}

/// Reads the one row of the summary statement.
///
/// `None` when the counts came back masked: the field is not this reader's to
/// count, and nothing more is read of it.
pub(super) fn summary(
    columns: &[String],
    rows: &[Vec<Value>],
    kind: FieldKind,
) -> Result<Option<SummaryRow>> {
    // An aggregate over no rows is still a row, but an answer may be cut short of it.
    let Some(values) = rows.first() else {
        return Ok(Some(SummaryRow::default()));
    };

    let row = Row { columns, values };

    let total = row
        .count(TOTAL)
        .ok_or_else(|| Error::internal("field analysis returned no row count"))?;

    // The count of rows is of no field. The count of values is of this one.
    let Some(present) = row.count(PRESENT) else {
        return Ok(None);
    };

    let range = row
        .text(MIN)
        .zip(row.text(MAX))
        .map(|(min, max)| ValueRange {
            min: min.to_string(),
            max: max.to_string(),
        });

    let numeric = kind == FieldKind::Number;

    let bounds = range.as_ref().filter(|_| numeric).and_then(|range| {
        range
            .min
            .parse::<f64>()
            .ok()
            .zip(range.max.parse::<f64>().ok())
    });

    let spread = numeric.then(|| Spread {
        avg: row.number(AVG),
        q1: row.number(Q1),
        median: row.number(MEDIAN),
        q3: row.number(Q3),
        stddev_samp: row.number(STDDEV_SAMP),
        stddev_pop: row.number(STDDEV_POP),
        skewness: None,
        kurtosis: None,
    });

    Ok(Some(SummaryRow {
        total,
        present: present.min(total),
        distinct: row.count(DISTINCT).unwrap_or_default().min(present),
        range,
        spread,
        bounds,
    }))
}

pub(super) fn top_values(columns: &[String], rows: &[Vec<Value>]) -> Result<Vec<TopValue>> {
    rows.iter()
        .map(|values| {
            let row = Row { columns, values };

            let value = row.text(VALUE).ok_or_else(|| {
                Error::internal("field analysis returned a value that is not text")
            })?;

            let count = row
                .count(COUNT)
                .ok_or_else(|| Error::internal("field analysis returned no value count"))?;

            Ok(TopValue {
                value: value.to_string(),
                count,
            })
        })
        .collect()
}

/// Reads the bars into `bins` counts. A value on the far edge of the last bar
/// is counted in it: rounding may put it one bar further than there are.
pub(super) fn bars(columns: &[String], rows: &[Vec<Value>], bins: usize) -> Result<Bars> {
    let mut counts = vec![0u64; bins];
    let mut moments = Moments::default();

    for values in rows {
        let row = Row { columns, values };

        let (Some(bar), Some(count)) = (row.get(BAR).and_then(Value::as_i64), row.count(COUNT))
        else {
            return Err(Error::internal("field analysis returned no bars"));
        };

        let index = usize::try_from(bar.max(0))
            .unwrap_or(0)
            .min(bins.saturating_sub(1));

        if let Some(held) = counts.get_mut(index) {
            *held += count;
        }

        moments.count += count;
        moments.second += row.number(MOMENT_2).unwrap_or_default();
        moments.third += row.number(MOMENT_3).unwrap_or_default();
        moments.fourth += row.number(MOMENT_4).unwrap_or_default();
    }

    Ok(Bars { counts, moments })
}

/// Reads how many rows hold each of `values` in each of `stretches` stretches
/// of time, in the order the values are given.
///
/// `None` when the stretches came back masked: time is a masked field of the
/// stream, and there is nothing to draw over it.
pub(super) fn trend(
    columns: &[String],
    rows: &[Vec<Value>],
    values: &[&str],
    stretches: usize,
) -> Result<Option<Vec<TrendSeries>>> {
    let mut series = values
        .iter()
        .map(|value| TrendSeries {
            value: (*value).to_string(),
            counts: vec![0; stretches],
        })
        .collect::<Vec<_>>();

    for values in rows {
        let row = Row { columns, values };

        let Some(stretch) = row.get(STRETCH).and_then(Value::as_i64) else {
            return Ok(None);
        };

        let (Some(value), Some(count)) = (row.text(VALUE), row.count(COUNT)) else {
            return Err(Error::internal("field analysis returned no trend"));
        };

        let held = series
            .iter_mut()
            .find(|series| series.value == value)
            .and_then(|series| series.counts.get_mut(usize::try_from(stretch).ok()?));

        // A value that was not asked for, or a stretch outside the plan, is not drawn.
        if let Some(held) = held {
            *held += count;
        }
    }

    Ok(Some(series))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn columns(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn a_number_is_read_with_its_range_and_spread() {
        let columns = columns(&[
            TOTAL,
            PRESENT,
            DISTINCT,
            MIN,
            MAX,
            AVG,
            STDDEV_SAMP,
            STDDEV_POP,
            Q1,
            MEDIAN,
            Q3,
        ]);
        let rows = vec![vec![
            json!(10),
            json!(8),
            json!(5),
            json!("200"),
            json!("504"),
            json!(301.5),
            json!(120.25),
            json!(112.5),
            json!(200.0),
            json!(204.0),
            json!(404.0),
        ]];

        let summary = summary(&columns, &rows, FieldKind::Number)
            .expect("summary")
            .expect("the field is not masked");

        assert_eq!(
            (summary.total, summary.present, summary.distinct),
            (10, 8, 5)
        );
        assert_eq!(summary.bounds, Some((200.0, 504.0)));
        assert_eq!(
            summary.range,
            Some(ValueRange {
                min: "200".into(),
                max: "504".into(),
            })
        );

        let spread = summary.spread.expect("a number has a spread");
        assert_eq!(spread.avg, Some(301.5));
        assert_eq!(spread.median, Some(204.0));
        assert_eq!(spread.skewness, None);
    }

    #[test]
    fn text_has_no_range_and_no_spread() {
        let columns = columns(&[TOTAL, PRESENT, DISTINCT]);
        let rows = vec![vec![json!(4), json!(3), json!(2)]];

        assert_eq!(
            summary(&columns, &rows, FieldKind::Text).expect("summary"),
            Some(SummaryRow {
                total: 4,
                present: 3,
                distinct: 2,
                ..SummaryRow::default()
            })
        );
    }

    #[test]
    fn a_masked_count_says_the_field_is_protected() {
        let columns = columns(&[TOTAL, PRESENT, DISTINCT]);
        let rows = vec![vec![json!(4), json!("***"), json!("***")]];

        assert_eq!(
            summary(&columns, &rows, FieldKind::Text).expect("summary"),
            None
        );
    }

    #[test]
    fn a_number_without_values_has_no_statistics() {
        let columns = columns(&[TOTAL, PRESENT, DISTINCT, MIN, MAX, AVG]);
        let rows = vec![vec![
            json!(4),
            json!(0),
            json!(0),
            Value::Null,
            Value::Null,
            Value::Null,
        ]];

        let summary = summary(&columns, &rows, FieldKind::Number)
            .expect("summary")
            .expect("the field is not masked");

        assert_eq!(summary.range, None);
        assert_eq!(summary.bounds, None);
        assert_eq!(summary.spread.expect("spread").avg, None);
    }

    #[test]
    fn bars_are_counted_and_the_far_edge_is_the_last_bar() {
        let columns = columns(&[BAR, COUNT, MOMENT_2, MOMENT_3, MOMENT_4]);
        // 1, 2, 2, 3 about their mean of 2, and one bar past the three there are.
        let rows = vec![
            vec![json!(0), json!(1), json!(1.0), json!(-1.0), json!(1.0)],
            vec![json!(1), json!(2), json!(0.0), json!(0.0), json!(0.0)],
            vec![json!(3), json!(1), json!(1.0), json!(1.0), json!(1.0)],
        ];

        let Bars { counts, moments } = bars(&columns, &rows, 3).expect("bars");

        assert_eq!(counts, [1, 2, 1]);
        assert_eq!(moments.skewness(), Some(0.0));
        // A fourth moment of 0.5 over a variance of 0.5 squared, less a normal's 3.
        assert_eq!(moments.excess_kurtosis(), Some(-1.0));
    }

    #[test]
    fn a_trend_is_read_value_by_value_in_the_order_asked() {
        let columns = columns(&[STRETCH, VALUE, COUNT]);
        let rows = vec![
            vec![json!(0), json!("web"), json!(2)],
            vec![json!(2), json!("api"), json!(5)],
            vec![json!(0), json!("api"), json!(1)],
            // Not one of the values asked for, and a stretch the plan does not have.
            vec![json!(1), json!("db"), json!(9)],
            vec![json!(3), json!("api"), json!(9)],
        ];

        assert_eq!(
            trend(&columns, &rows, &["api", "web"], 3).expect("trend"),
            Some(vec![
                TrendSeries {
                    value: "api".into(),
                    counts: vec![1, 0, 5],
                },
                TrendSeries {
                    value: "web".into(),
                    counts: vec![2, 0, 0],
                },
            ])
        );
    }

    #[test]
    fn a_trend_over_masked_time_is_not_drawn() {
        let columns = columns(&[STRETCH, VALUE, COUNT]);
        let rows = vec![vec![json!("***"), json!("api"), json!(1)]];

        assert_eq!(trend(&columns, &rows, &["api"], 3).expect("trend"), None);
    }

    #[test]
    fn values_that_are_all_the_same_have_no_shape() {
        let moments = Moments {
            count: 5,
            ..Moments::default()
        };

        assert_eq!(moments.skewness(), None);
        assert_eq!(moments.excess_kurtosis(), None);
    }
}
