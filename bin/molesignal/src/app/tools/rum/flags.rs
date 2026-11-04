// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The feature flags a RUM application evaluates: which flags sessions met and, for one flag, how
//! the sessions of each of its values did, side by side.

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::ToolResult;

use super::{reader, summary_span};
use crate::{
    app::{
        iam::IamContext,
        rum::{
            aggregate::{self, Request},
            overview::scope_filters,
            reader::{Counted, Reader},
            selection::Selection,
        },
        tools::{
            ToolRuntime,
            common::{TimeRangeArg, parse_args},
        },
    },
    domain::rum::event::RumStream,
    infra::rum::query::{Aggregate, KEY_COLUMN, KEY_COUNT, MeasureSpec, Scope, keys_sql},
    shared::Result,
};

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Args {
    application: String,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    /// One flag, to compare its values; every flag when absent.
    #[serde(default)]
    flag: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

fn measure(
    function: &str,
    field: Option<&str>,
    name: &str,
    filters: Vec<(&str, &str)>,
) -> MeasureSpec {
    MeasureSpec {
        function: function.into(),
        field: field.map(str::to_string),
        fallback: None,
        filters: filters
            .into_iter()
            .map(|(field, value)| crate::infra::rum::query::Filter {
                field: field.into(),
                op: ">".into(),
                value: value.into(),
                quoted: false,
            })
            .collect(),
        name: Some(name.into()),
    }
}

/// How the sessions of each value of a flag did: the same numbers the page of the flag compares.
fn comparison() -> Vec<MeasureSpec> {
    vec![
        measure("count_distinct", Some("session.id"), "sessions", Vec::new()),
        measure("count", None, "views", Vec::new()),
        measure(
            "count",
            None,
            "views_with_error",
            vec![("view.error.count", "0")],
        ),
        measure(
            "count",
            None,
            "views_with_frustration",
            vec![("view.frustration.count", "0")],
        ),
        measure(
            "p75",
            Some("view.performance.lcp.timestamp"),
            "lcp_p75_ns",
            Vec::new(),
        ),
        measure(
            "p75",
            Some("view.performance.inp.duration"),
            "inp_p75_ns",
            Vec::new(),
        ),
        measure(
            "p75",
            Some("view.performance.cls.score"),
            "cls_p75",
            Vec::new(),
        ),
    ]
}

fn variants(answer: &Aggregate) -> Vec<Value> {
    answer
        .rows
        .iter()
        .map(|row| {
            let mut variant = serde_json::Map::new();

            variant.insert(
                "value".into(),
                row.group
                    .first()
                    .cloned()
                    .flatten()
                    .map_or(Value::Null, Value::String),
            );

            for (info, value) in answer.measures.iter().zip(&row.values) {
                variant.insert(info.name.clone(), value.clone());
            }

            Value::Object(variant)
        })
        .collect()
}

pub(super) async fn summarize(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: Args = parse_args(arguments)?;
    let span = summary_span(args.time_range)?;
    let limit = args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let reader = reader(runtime, auth);

    let selection = Selection::new(
        Some(RumStream::Views.collection()),
        Some(span.from),
        Some(span.to),
        scope_filters(&args.application, args.environment.as_deref()),
        Vec::new(),
    )?;

    let Some(flag) = args
        .flag
        .as_deref()
        .map(str::trim)
        .filter(|flag| !flag.is_empty())
    else {
        return list(&reader, &selection, limit).await;
    };

    let answer = aggregate::run(
        &reader,
        &Request {
            selection: &selection,
            measures: &comparison(),
            group_by: &[format!("feature_flags.{flag}")],
            bucket_micros: None,
            origin_micros: None,
            order_by: Some("sessions"),
            descending: Some(true),
            limit: Some(limit),
            estimate: true,
        },
    )
    .await?;

    Ok(ToolResult::json(json!({
        "application": args.application,
        "flag": flag,
        "from": span.from,
        "to": span.to,
        "variants": variants(&answer),
        "unavailable": answer.unavailable,
    })))
}

async fn list(reader: &Reader<'_>, selection: &Selection, limit: usize) -> Result<ToolResult> {
    let conditions = selection.conditions()?;

    let statement = keys_sql(
        &Scope {
            stream: selection.stream,
            from: selection.span.from,
            to: selection.span.to,
            conditions: &conditions,
        },
        "feature_flags",
        limit,
    );

    let flags = match reader
        .count(selection.stream, selection.span, statement)
        .await?
    {
        Counted::Rows(Some(result)) => {
            let at = |name: &str| result.columns.iter().position(|column| column == name);

            at(KEY_COLUMN)
                .zip(at(KEY_COUNT))
                .map(|(key, sessions)| {
                    result
                        .rows
                        .iter()
                        .map(|row| {
                            json!({
                                "flag": row.get(key),
                                "sessions": row.get(sessions),
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        }
        Counted::Rows(None) => Vec::new(),
        Counted::Unavailable(reason) => {
            return Ok(ToolResult::json(
                json!({ "flags": [], "unavailable": reason }),
            ));
        }
    };

    Ok(ToolResult::json(json!({
        "from": selection.span.from,
        "to": selection.span.to,
        "flags": flags,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::rum::query::{AggregateSpec, Plan};

    #[test]
    fn the_values_of_a_flag_are_compared_by_a_plan_of_the_views() {
        let measures = comparison();
        let group_by = ["feature_flags.checkout_v2".to_string()];

        let plan = Plan::new(
            RumStream::Views,
            &AggregateSpec {
                measures: &measures,
                group_by: &group_by,
                bars: None,
                order_by: Some("sessions"),
                descending: Some(true),
                limit: Some(10),
            },
        );

        assert!(plan.is_ok());
    }

    #[test]
    fn unknown_arguments_are_refused() {
        assert!(parse_args::<Args>(json!({"application": "a", "nope": 1})).is_err());
    }
}
