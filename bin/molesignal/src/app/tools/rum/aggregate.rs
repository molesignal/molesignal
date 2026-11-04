// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Numbers over the rows of one RUM stream that a question selects: the explorer's charts and
//! tables, asked in words. The statement is the one the explorer's aggregate runs, so the numbers
//! agree with the rows the explorer lists.

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::ToolResult;

use super::{reader, summary_span};
use crate::{
    app::{
        iam::IamContext,
        rum::{
            aggregate::{self, Request, descending},
            overview::scope_filters,
            selection::Selection,
        },
        tools::{
            ToolRuntime,
            common::{TimeRangeArg, parse_args},
        },
    },
    infra::rum::query::{Filter, MeasureSpec},
    shared::{Error, Result},
};

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Args {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    application: Option<String>,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    filters: Vec<Filter>,
    #[serde(default)]
    free_text: Vec<String>,
    #[serde(default)]
    measures: Vec<MeasureSpec>,
    #[serde(default)]
    group_by: Vec<String>,
    #[serde(default)]
    bucket_seconds: Option<i64>,
    #[serde(default)]
    order_by: Option<String>,
    #[serde(default)]
    order: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    /// Count what the rows kept stand for (the default), or the rows that are stored.
    #[serde(default)]
    estimate: Option<bool>,
}

/// One count of the rows, for a question that asks for none.
fn count_of_rows() -> MeasureSpec {
    MeasureSpec {
        function: "count".into(),
        field: None,
        fallback: None,
        filters: Vec::new(),
        name: Some("count".into()),
    }
}

/// The bars of time a number of seconds stands for, in microseconds.
fn bucket_micros(seconds: Option<i64>) -> Result<Option<i64>> {
    seconds
        .map(|seconds| {
            seconds
                .checked_mul(1_000_000)
                .filter(|micros| *micros > 0)
                .ok_or_else(|| Error::invalid("bucketSeconds must be a positive number"))
        })
        .transpose()
}

/// The filters of a question: the application and the environment it names, then its own.
fn filters_of(args: &Args) -> Vec<Filter> {
    let mut filters = args
        .application
        .as_deref()
        .map(str::trim)
        .filter(|application| !application.is_empty())
        .map(|application| scope_filters(application, args.environment.as_deref()))
        .unwrap_or_else(|| {
            args.environment
                .as_deref()
                .map(str::trim)
                .filter(|environment| !environment.is_empty())
                .map(|environment| {
                    vec![Filter {
                        field: "env".into(),
                        op: "=".into(),
                        value: environment.into(),
                        quoted: false,
                    }]
                })
                .unwrap_or_default()
        });

    filters.extend(args.filters.iter().cloned());
    filters
}

pub(super) async fn run(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: Args = parse_args(arguments)?;
    let span = summary_span(args.time_range)?;

    let selection = Selection::new(
        Some(&args.kind),
        Some(span.from),
        Some(span.to),
        filters_of(&args),
        args.free_text.clone(),
    )?;

    let measures = if args.measures.is_empty() {
        vec![count_of_rows()]
    } else {
        args.measures.clone()
    };

    let reader = reader(runtime, auth);

    let answer = aggregate::run(
        &reader,
        &Request {
            selection: &selection,
            measures: &measures,
            group_by: &args.group_by,
            bucket_micros: bucket_micros(args.bucket_seconds)?,
            origin_micros: None,
            order_by: args.order_by.as_deref(),
            descending: descending(args.order.as_deref())?,
            limit: Some(args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)),
            estimate: args.estimate.unwrap_or(true),
        },
    )
    .await?;

    let mut answer = serde_json::to_value(&answer)
        .map_err(|error| Error::internal(format!("serialize tool result: {error}")))?;

    answer["type"] = json!(args.kind.trim());
    answer["range"] = json!({ "startTime": span.from, "endTime": span.to });

    Ok(ToolResult::json(answer))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(value: Value) -> Args {
        serde_json::from_value(value).expect("arguments")
    }

    #[test]
    fn a_question_names_a_stream_and_what_it_leaves_out_is_left_to_the_defaults() {
        let found = args(json!({ "type": "views" }));

        assert_eq!(found.kind, "views");
        assert!(found.measures.is_empty());
        assert!(serde_json::from_value::<Args>(json!({})).is_err());
        assert!(serde_json::from_value::<Args>(json!({ "type": "views", "nope": 1 })).is_err());
    }

    #[test]
    fn the_application_and_the_environment_come_before_the_filters_of_the_question() {
        let found = args(json!({
            "type": "errors",
            "application": "shop-web",
            "environment": "prod",
            "filters": [{ "field": "error.is_crash", "op": "=", "value": "true" }],
        }));

        let fields = filters_of(&found)
            .iter()
            .map(|filter| filter.field.clone())
            .collect::<Vec<_>>();

        assert_eq!(fields, ["application.id", "env", "error.is_crash"]);
    }

    #[test]
    fn an_environment_without_an_application_still_narrows() {
        let found = args(json!({ "type": "views", "environment": " staging " }));
        let filters = filters_of(&found);

        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].field, "env");
        assert_eq!(filters[0].value, "staging");
    }

    #[test]
    fn bars_are_seconds_that_make_a_positive_span() {
        assert_eq!(bucket_micros(None).unwrap(), None);
        assert_eq!(bucket_micros(Some(300)).unwrap(), Some(300_000_000));
        assert!(bucket_micros(Some(0)).is_err());
        assert!(bucket_micros(Some(-5)).is_err());
        assert!(bucket_micros(Some(i64::MAX)).is_err());
    }

    #[test]
    fn a_measure_is_asked_as_the_explorer_asks_it() {
        let found = args(json!({
            "type": "views",
            "measures": [{
                "fn": "p75",
                "field": "view.performance.lcp.timestamp",
                "name": "slow",
                "where": [{ "field": "env", "op": "=", "value": "prod" }],
            }],
            "groupBy": ["view.url_path_group"],
        }));

        assert_eq!(found.measures[0].function, "p75");
        assert_eq!(found.measures[0].filters[0].field, "env");
        assert_eq!(found.group_by, ["view.url_path_group"]);
    }
}
