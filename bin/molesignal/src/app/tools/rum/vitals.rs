// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Core Web Vitals page by page: the 75th percentile real users had on each page, graded with the
//! thresholds web.dev gives. The engine computes them, over every view in the range, with the same
//! aggregate the RUM overview reads.

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::ToolResult;

use super::{equal_to, rum_range};
use crate::{
    app::{
        iam::IamContext,
        rum::vitals::{VITALS, measures, summary},
        tools::{
            ToolRuntime,
            common::{TimeRangeArg, parse_args, run_optional_stream_query},
        },
    },
    domain::{rum::event::RumStream, stream::StreamType},
    infra::rum::query::{Aggregate, AggregateSpec, Filter, Plan, Scope, conditions},
    shared::{Result, time::TimeRange},
};

const PAGE_GROUP: &str = "view.url_path_group";
const PAGE_PATH: &str = "view.url_path";

/// Most pages one summary reads: the aggregate's own limit. A range with more is summarized by
/// its most viewed pages.
const MAX_PAGES: usize = 250;

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

pub(super) async fn summarize(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: VitalsArgs = parse_args(arguments)?;
    let range = rum_range(args.time_range)?;
    let limit = args.limit.unwrap_or(50).clamp(1, 200);

    let mut selected = Vec::new();

    selected.extend(equal_to("application.id", args.application.as_deref()));
    selected.extend(equal_to("env", args.environment.as_deref()));

    if let Some(page) = args
        .page
        .as_deref()
        .map(str::trim)
        .filter(|page| !page.is_empty())
    {
        selected.extend(conditions(
            RumStream::Views,
            &[Filter {
                field: PAGE_PATH.into(),
                op: "contains".into(),
                value: page.into(),
                quoted: false,
            }],
        )?);
    }

    let measures = measures();
    let by_page = [PAGE_GROUP.to_string()];

    let plan = |group_by: &[String]| {
        Plan::new(
            RumStream::Views,
            &AggregateSpec {
                measures: &measures,
                group_by,
                bars: None,
                order_by: Some("views"),
                descending: Some(true),
                limit: Some(MAX_PAGES),
            },
        )
    };

    let (overall, pages) = (plan(&[])?, plan(&by_page)?);

    let scope = Scope {
        stream: RumStream::Views,
        from: range.start.0,
        to: range.end.0,
        conditions: &selected,
    };

    let read = Read {
        runtime,
        auth,
        range,
        scope: &scope,
    };

    let (Some((overall, overall_scanned)), Some((pages, pages_scanned))) =
        (read.answer(&overall).await?, read.answer(&pages).await?)
    else {
        return Ok(ToolResult::json(json!({
            "pages": [], "streamAvailable": false,
            "message": "rum_views has not received data yet",
        })));
    };

    let mut rows = pages
        .rows
        .iter()
        .filter_map(|row| {
            let page = row
                .group
                .first()?
                .as_deref()
                .filter(|page| !page.is_empty())?;
            let mut summary = summary(&row.values);

            summary["page"] = json!(page);

            Some(summary)
        })
        .collect::<Vec<_>>();

    let page_count = rows.len();
    let reached_the_limit = pages.rows.len() >= MAX_PAGES;

    sort_slowest_first(&mut rows);
    rows.truncate(limit);

    let thresholds = VITALS
        .iter()
        .map(|vital| {
            (
                vital.name.to_string(),
                json!({ "good": vital.good, "poor": vital.poor }),
            )
        })
        .collect::<serde_json::Map<_, _>>();

    Ok(ToolResult::json(json!({
        "overall": overall.rows.first().map_or_else(|| summary(&[]), |row| summary(&row.values)),
        "pages": rows,
        "pageCount": page_count,
        "truncated": page_count > limit || reached_the_limit,
        "thresholds": thresholds,
        "streamAvailable": true,
        "scannedRows": overall_scanned + pages_scanned,
        "unavailable": overall.unavailable.or(pages.unavailable),
    })))
}

/// Where the aggregates of one summary are read.
struct Read<'a> {
    runtime: &'a ToolRuntime,
    auth: &'a IamContext,
    range: TimeRange,
    scope: &'a Scope<'a>,
}

impl Read<'_> {
    /// What `plan` comes to, and how many rows the engine read to say so; `None` when nothing was
    /// ever written to the stream.
    async fn answer(&self, plan: &Plan) -> Result<Option<(Aggregate, u64)>> {
        let Some(result) = run_optional_stream_query(
            self.runtime,
            &self.auth.org_id,
            plan.first_sql(self.scope),
            self.range,
            RumStream::Views.name(),
            StreamType::LOGS,
            MAX_PAGES,
        )
        .await?
        else {
            return Ok(None);
        };

        let answer = plan.answer(&result.columns, &result.rows)?;

        Ok(Some((answer, result.scanned_rows)))
    }
}

/// Slowest LCP first; pages without an LCP come last, the most viewed of them first.
fn sort_slowest_first(rows: &mut [Value]) {
    let lcp = |row: &Value| row["lcp_ms"]["p75"].as_f64();
    let views = |row: &Value| row["views"].as_u64().unwrap_or_default();

    rows.sort_by(|left, right| {
        lcp(right)
            .partial_cmp(&lcp(left))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| views(right).cmp(&views(left)))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slowest_pages_come_first_and_those_without_an_lcp_last() {
        let mut rows = vec![
            json!({ "page": "/a", "views": 9, "lcp_ms": Value::Null }),
            json!({ "page": "/b", "views": 5, "lcp_ms": { "p75": 1_000.0 } }),
            json!({ "page": "/c", "views": 1, "lcp_ms": { "p75": 5_000.0 } }),
            json!({ "page": "/d", "views": 20, "lcp_ms": Value::Null }),
        ];

        sort_slowest_first(&mut rows);

        assert_eq!(
            rows.iter()
                .map(|row| row["page"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["/c", "/b", "/d", "/a"]
        );
    }
}
