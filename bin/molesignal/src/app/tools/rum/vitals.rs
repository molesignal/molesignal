// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Core Web Vitals page by page: the 75th percentile real users had on each page, graded with the
//! thresholds web.dev gives.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::ToolResult;

use super::rum_range;
use crate::{
    app::{
        iam::IamContext,
        tools::{
            ToolRuntime,
            common::{TimeRangeArg, parse_args},
        },
    },
    infra::rum::read_model::RumReadModelReader,
    shared::{Error, Result},
};

/// The Core Web Vitals and TTFB with the thresholds web.dev gives: a 75th percentile at or under
/// `good` is good, over `poor` is poor. The RUM performance page grades LCP, CLS and TTFB with the
/// same numbers.
const VITALS: [(&str, f64, f64); 4] = [
    ("lcp_ms", 2_500.0, 4_000.0),
    ("inp_ms", 200.0, 500.0),
    ("cls", 0.1, 0.25),
    ("ttfb_ms", 800.0, 1_800.0),
];

const VITAL_COLUMNS: &[&str] = &[
    "_timestamp",
    "ts_micros",
    "type",
    "page",
    "url",
    "application",
    "environment",
    "lcp_ms",
    "inp_ms",
    "cls",
    "ttfb_ms",
];

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct VitalsArgs {
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    application: Option<String>,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    page: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

/// One page's views and the values each vital had on them.
#[derive(Default)]
struct PageVitals {
    views: u64,
    values: [Vec<f64>; VITALS.len()],
}

impl PageVitals {
    fn add(&mut self, values: [Option<f64>; VITALS.len()]) {
        self.views += 1;

        for (samples, value) in self.values.iter_mut().zip(values) {
            if let Some(value) = value.filter(|value| value.is_finite() && *value >= 0.0) {
                samples.push(value);
            }
        }
    }

    fn summary(mut self) -> Value {
        let mut summary = json!({ "views": self.views });

        for ((name, good, poor), samples) in VITALS.iter().zip(self.values.iter_mut()) {
            summary[*name] = p75(samples).map_or(Value::Null, |value| {
                json!({
                    "p75": value,
                    "samples": samples.len(),
                    "rating": rating(value, *good, *poor),
                })
            });
        }

        summary
    }
}

pub(super) async fn summarize(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: VitalsArgs = parse_args(arguments)?;
    let range = rum_range(args.time_range)?;
    let limit = args.limit.unwrap_or(50).clamp(1, 200);

    let reader = RumReadModelReader::new(
        runtime.observability.catalog_files.clone(),
        runtime.observability.object_store.clone(),
    )
    .with_catalog_source(runtime.observability.catalog_query.clone());

    let keeps = |value: Option<&str>, wanted: Option<&str>| {
        wanted.is_none_or(|expected| value == Some(expected))
    };
    let mut overall = PageVitals::default();
    let mut pages = HashMap::<String, PageVitals>::new();

    let visited = reader
        .visit_actions(&auth.org_id, range, VITAL_COLUMNS, |record| {
            let Some(page) = record.page_key() else {
                return;
            };

            if record.event_type != "view"
                || !keeps(record.application.as_deref(), args.application.as_deref())
                || !keeps(record.environment.as_deref(), args.environment.as_deref())
                || args
                    .page
                    .as_deref()
                    .is_some_and(|wanted| !page.contains(wanted))
            {
                return;
            }

            let values = [record.lcp_ms, record.inp_ms, record.cls, record.ttfb_ms];

            overall.add(values);
            pages.entry(page.to_string()).or_default().add(values);
        })
        .await;

    let stats = match visited {
        Ok(stats) => stats,
        Err(Error::NotFound(_)) => {
            return Ok(ToolResult::json(json!({
                "pages": [], "stream_available": false,
                "message": "rum_actions has not received data yet",
            })));
        }
        Err(error) => return Err(error),
    };

    let page_count = pages.len();
    let mut rows = pages
        .into_iter()
        .map(|(page, vitals)| {
            let mut row = vitals.summary();
            row["page"] = json!(page);
            row
        })
        .collect::<Vec<_>>();

    // Slowest LCP first; pages without an LCP come last, the most viewed of them first.
    let lcp = |row: &Value| row["lcp_ms"]["p75"].as_f64();
    let views = |row: &Value| row["views"].as_u64().unwrap_or_default();
    rows.sort_by(|left, right| {
        lcp(right)
            .partial_cmp(&lcp(left))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| views(right).cmp(&views(left)))
    });
    rows.truncate(limit);

    let thresholds = VITALS
        .iter()
        .map(|(name, good, poor)| ((*name).to_string(), json!({ "good": good, "poor": poor })))
        .collect::<serde_json::Map<_, _>>();

    Ok(ToolResult::json(json!({
        "overall": overall.summary(),
        "pages": rows,
        "page_count": page_count,
        "truncated": page_count > limit,
        "thresholds": thresholds,
        "stream_available": true,
        "scanned_rows": stats.rows,
    })))
}

/// The 75th percentile by nearest rank, as the RUM pages compute it.
fn p75(samples: &mut [f64]) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }

    samples.sort_by(f64::total_cmp);

    let rank = (samples.len() * 3).div_ceil(4).max(1);

    Some(samples[rank.min(samples.len()) - 1])
}

fn rating(value: f64, good: f64, poor: f64) -> &'static str {
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
    fn p75_takes_the_nearest_rank_like_the_rum_pages() {
        assert_eq!(p75(&mut []), None);
        assert_eq!(p75(&mut [7.0]), Some(7.0));
        assert_eq!(p75(&mut [4.0, 1.0, 3.0, 2.0]), Some(3.0));
        assert_eq!(p75(&mut [5.0, 1.0, 4.0, 2.0, 3.0]), Some(4.0));
    }

    #[test]
    fn ratings_include_the_good_and_poor_thresholds_themselves() {
        assert_eq!(rating(2_500.0, 2_500.0, 4_000.0), "good");
        assert_eq!(rating(2_500.1, 2_500.0, 4_000.0), "needs_improvement");
        assert_eq!(rating(4_000.0, 2_500.0, 4_000.0), "needs_improvement");
        assert_eq!(rating(4_000.1, 2_500.0, 4_000.0), "poor");
    }

    #[test]
    fn a_page_counts_every_view_and_only_the_values_it_has() {
        let mut page = PageVitals::default();

        page.add([Some(3_000.0), None, Some(0.3), Some(f64::NAN)]);
        page.add([Some(1_000.0), Some(150.0), None, Some(-1.0)]);

        let summary = page.summary();

        assert_eq!(summary["views"], 2);
        assert_eq!(summary["lcp_ms"]["p75"], 3_000.0);
        assert_eq!(summary["lcp_ms"]["samples"], 2);
        assert_eq!(summary["lcp_ms"]["rating"], "needs_improvement");
        assert_eq!(summary["inp_ms"]["rating"], "good");
        assert_eq!(summary["cls"]["rating"], "poor");
        assert_eq!(summary["ttfb_ms"], Value::Null);
    }
}
