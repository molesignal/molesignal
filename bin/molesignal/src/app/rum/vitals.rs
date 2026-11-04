// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The Core Web Vitals a view carries, and the thresholds web.dev grades them with. The engine
//! computes the 75th percentile over every view in the range, with the same aggregate the RUM
//! overview reads.

use serde::Serialize;
use serde_json::{Value, json};

use crate::infra::rum::query::MeasureSpec;

/// A vital a view carries: what the summary calls it, the column, whether the SDK sent it in
/// nanoseconds (the summary has milliseconds), and the thresholds web.dev gives: a 75th percentile
/// at or under `good` is good, over `poor` is poor.
pub struct Vital {
    pub name: &'static str,
    pub column: &'static str,
    pub nanoseconds: bool,
    pub good: f64,
    pub poor: f64,
}

pub const VITALS: [Vital; 4] = [
    Vital {
        name: "lcp_ms",
        column: "view.performance.lcp.timestamp",
        nanoseconds: true,
        good: 2_500.0,
        poor: 4_000.0,
    },
    Vital {
        name: "inp_ms",
        column: "view.performance.inp.duration",
        nanoseconds: true,
        good: 200.0,
        poor: 500.0,
    },
    Vital {
        name: "cls",
        column: "view.performance.cls.score",
        nanoseconds: false,
        good: 0.1,
        poor: 0.25,
    },
    Vital {
        name: "ttfb_ms",
        column: "view.first_byte",
        nanoseconds: true,
        good: 800.0,
        poor: 1_800.0,
    },
];

/// How many of [`VITALS`] are Core Web Vitals: the first three.
pub const CORE: usize = 3;

/// Whether a vital of [`VITALS`] is one of the Core Web Vitals.
pub fn is_core(name: &str) -> bool {
    VITALS[..CORE].iter().any(|vital| vital.name == name)
}

const NANOSECONDS_PER_MILLISECOND: f64 = 1_000_000.0;

/// A measure of a stream's column, named.
pub fn spec(function: &str, column: Option<&str>, name: &str) -> MeasureSpec {
    MeasureSpec {
        function: function.into(),
        field: column.map(str::to_string),
        fallback: None,
        filters: Vec::new(),
        name: Some(name.into()),
    }
}

/// The views of a page or of every page, and for each vital its 75th percentile and the number of
/// views that had a value: `views`, then `p75` and `samples` of each vital in the order of
/// [`VITALS`].
pub fn measures() -> Vec<MeasureSpec> {
    let mut measures = vec![spec("count", None, "views")];

    for vital in &VITALS {
        measures.push(spec("p75", Some(vital.column), vital.name));
        measures.push(spec(
            "count",
            Some(vital.column),
            &format!("{}_samples", vital.name),
        ));
    }

    measures
}

/// One vital as a row of [`measures`] says it, in the unit it is read in.
#[derive(Debug, PartialEq, Serialize)]
pub struct Reading {
    pub name: &'static str,
    pub p75: f64,
    /// How many views had a value.
    pub samples: u64,
    pub rating: &'static str,
}

/// What `values`, a row of the aggregate of [`measures`], says of each vital that has a value. A
/// value the reader may not see is no value.
pub fn readings(values: &[Value]) -> Vec<Reading> {
    VITALS
        .iter()
        .enumerate()
        .filter_map(|(index, vital)| {
            let p75 = values.get(1 + 2 * index).and_then(Value::as_f64)?;

            let samples = values
                .get(2 + 2 * index)
                .and_then(Value::as_u64)
                .unwrap_or_default();

            let p75 = if vital.nanoseconds {
                p75 / NANOSECONDS_PER_MILLISECOND
            } else {
                p75
            };

            Some(Reading {
                name: vital.name,
                p75,
                samples,
                rating: rating(p75, vital.good, vital.poor),
            })
        })
        .collect()
}

/// What one row of the aggregate says: the views and, for each vital, its 75th percentile in the
/// summary's unit with the number of views that had a value and the grade.
pub fn summary(values: &[Value]) -> Value {
    let mut summary = json!({
        "views": values.first().and_then(Value::as_u64).unwrap_or_default(),
    });

    for vital in &VITALS {
        summary[vital.name] = Value::Null;
    }

    for reading in readings(values) {
        summary[reading.name] = json!({
            "p75": reading.p75,
            "samples": reading.samples,
            "rating": reading.rating,
        });
    }

    summary
}

pub fn rating(value: f64, good: f64, poor: f64) -> &'static str {
    if value <= good {
        "good"
    } else if value <= poor {
        "needs_improvement"
    } else {
        "poor"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratings_include_the_good_and_poor_thresholds_themselves() {
        assert_eq!(rating(2_500.0, 2_500.0, 4_000.0), "good");
        assert_eq!(rating(2_500.1, 2_500.0, 4_000.0), "needs_improvement");
        assert_eq!(rating(4_000.0, 2_500.0, 4_000.0), "needs_improvement");
        assert_eq!(rating(4_000.1, 2_500.0, 4_000.0), "poor");
    }

    #[test]
    fn the_measures_are_the_views_then_each_vital_and_its_samples() {
        let names = measures()
            .into_iter()
            .map(|measure| measure.name.expect("a name"))
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            [
                "views",
                "lcp_ms",
                "lcp_ms_samples",
                "inp_ms",
                "inp_ms_samples",
                "cls",
                "cls_samples",
                "ttfb_ms",
                "ttfb_ms_samples",
            ]
        );
    }

    #[test]
    fn a_row_of_the_aggregate_is_graded_in_milliseconds_and_scores() {
        let values = [
            json!(40),
            json!(3_000_000_000.0),
            json!(38),
            Value::Null,
            json!(0),
            json!(0.3),
            json!(40),
            json!("***"),
            json!(40),
        ];

        let summary = summary(&values);

        assert_eq!(summary["views"], 40);
        assert_eq!(summary["lcp_ms"]["p75"], 3_000.0);
        assert_eq!(summary["lcp_ms"]["samples"], 38);
        assert_eq!(summary["lcp_ms"]["rating"], "needs_improvement");
        assert_eq!(summary["inp_ms"], Value::Null);
        assert_eq!(summary["cls"]["p75"], 0.3);
        assert_eq!(summary["cls"]["rating"], "poor");
        // A value the reader may not see is no value.
        assert_eq!(summary["ttfb_ms"], Value::Null);
    }

    #[test]
    fn readings_are_the_vitals_that_have_a_value() {
        let values = [
            json!(40),
            json!(1_000_000_000.0),
            json!(30),
            Value::Null,
            json!(0),
            json!(0.05),
            json!(12),
            json!("***"),
            json!(40),
        ];

        let found = readings(&values);

        assert_eq!(
            found.iter().map(|reading| reading.name).collect::<Vec<_>>(),
            ["lcp_ms", "cls"]
        );
        assert_eq!(found[0].p75, 1_000.0);
        assert_eq!(found[0].rating, "good");
        assert_eq!(found[1].samples, 12);
    }

    #[test]
    fn the_first_three_are_the_core_vitals() {
        assert!(is_core("lcp_ms") && is_core("inp_ms") && is_core("cls"));
        assert!(!is_core("ttfb_ms"));
    }

    #[test]
    fn nothing_read_is_a_summary_of_no_views() {
        let summary = summary(&[]);

        assert_eq!(summary["views"], 0);
        assert_eq!(summary["lcp_ms"], Value::Null);
        assert!(readings(&[]).is_empty());
    }
}
