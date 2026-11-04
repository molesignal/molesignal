// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The error issues of a RUM application: the list ranked by what each touched, and one issue with
//! its history, its latest sessions and its stack. The reads are the ones the issue pages make.

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::ToolResult;

use super::{reader, summary_span};
use crate::{
    app::{
        iam::IamContext,
        rum::issues::{
            self, Query,
            select::{Holder, Sort, StatusFilter},
        },
        tools::{
            ToolRuntime,
            common::{TimeRangeArg, json_result, parse_args},
        },
    },
    shared::{Error, Result},
};

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 50;

/// How long a stack is the model is given; the rest is cut and said to be.
const STACK_TEXT_CHARS: usize = 4_000;
const STACK_FRAMES: usize = 25;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListArgs {
    application: String,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    sort: Option<String>,
    #[serde(default)]
    search: Option<String>,
    #[serde(default)]
    fresh: Option<bool>,
    #[serde(default)]
    limit: Option<usize>,
}

pub(super) async fn list(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: ListArgs = parse_args(arguments)?;
    let span = summary_span(args.time_range)?;
    let sort = Sort::parse(args.sort.as_deref())?;

    let application = args.application.trim();

    if application.is_empty() {
        return Err(Error::invalid("application is required"));
    }

    let query = Query {
        application: application.to_string(),
        env: args.environment,
        source: None,
        filters: Vec::new(),
        from: Some(span.from),
        to: Some(span.to),
        status: StatusFilter::parse(args.status.as_deref())?,
        assignee: Holder::Anyone,
        team: Holder::Anyone,
        search: args.search.unwrap_or_default(),
        fresh: args.fresh.unwrap_or_default(),
        sort,
        descending: sort.default_descending(),
        limit: args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT),
    };

    let reader = reader(runtime, auth);

    let listing = issues::list(&reader, runtime.observability.rum_issues.as_ref(), &query).await?;

    let mut answer = serde_json::to_value(&listing)
        .map_err(|error| Error::internal(format!("serialize tool result: {error}")))?;

    answer["application"] = json!(query.application);
    answer["range"] = json!({ "startTime": span.from, "endTime": span.to });
    answer["notes"] = json!({
        "trend": "occurrences in 12 equal parts of the range, oldest first",
        "numbers": "sessions, users and occurrences are of the range; first_seen_at and last_seen_at are of all time",
    });

    Ok(ToolResult::json(answer))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetArgs {
    issue_id: String,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
}

pub(super) async fn get(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: GetArgs = parse_args(arguments)?;
    let span = summary_span(args.time_range)?;
    let reader = reader(runtime, auth);

    let detail = match issues::detail(
        &reader,
        runtime.observability.rum_issues.as_ref(),
        &args.issue_id,
        Some(span.from),
        Some(span.to),
    )
    .await
    {
        Ok(detail) => detail,
        Err(Error::NotFound(_)) => {
            return Ok(ToolResult::error(format!(
                "RUM issue {} not found",
                args.issue_id.trim()
            )));
        }
        Err(error) => return Err(error),
    };

    let mut answer = serde_json::to_value(&detail)
        .map_err(|error| Error::internal(format!("serialize tool result: {error}")))?;

    cut_stack(&mut answer["stack"]);
    answer["range"] = json!({ "startTime": span.from, "endTime": span.to });

    json_result(&answer)
}

/// The stack of an issue as long as the model is given: its text and its frames cut.
fn cut_stack(stack: &mut Value) {
    let Some(stack) = stack.as_object_mut() else {
        return;
    };

    if let Some(text) = stack.get("text").and_then(Value::as_str)
        && text.chars().count() > STACK_TEXT_CHARS
    {
        let cut = text.chars().take(STACK_TEXT_CHARS).collect::<String>();

        stack.insert("text".into(), Value::String(cut));
        stack.insert("textTruncated".into(), Value::Bool(true));
    }

    if let Some(frames) = stack.get_mut("frames").and_then(Value::as_array_mut)
        && frames.len() > STACK_FRAMES
    {
        frames.truncate(STACK_FRAMES);
        stack.insert("framesTruncated".into(), Value::Bool(true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_stack_is_cut_and_says_so() {
        let mut stack = json!({
            "text": "x".repeat(STACK_TEXT_CHARS + 10),
            "frames": (0..40).map(|line| json!({ "line": line })).collect::<Vec<_>>(),
        });

        cut_stack(&mut stack);

        assert_eq!(
            stack["text"].as_str().unwrap().chars().count(),
            STACK_TEXT_CHARS
        );
        assert_eq!(stack["textTruncated"], true);
        assert_eq!(stack["frames"].as_array().unwrap().len(), STACK_FRAMES);
        assert_eq!(stack["framesTruncated"], true);
    }

    #[test]
    fn a_short_stack_is_left_as_it_is() {
        let mut stack = json!({ "text": "at a", "frames": [{ "line": 1 }] });
        let before = stack.clone();

        cut_stack(&mut stack);
        assert_eq!(stack, before);

        let mut none = Value::Null;

        cut_stack(&mut none);
        assert_eq!(none, Value::Null);
    }

    #[test]
    fn an_issue_list_needs_an_application_and_refuses_what_it_does_not_know() {
        let args = |value: Value| serde_json::from_value::<ListArgs>(value);

        assert!(args(json!({})).is_err());
        assert!(args(json!({ "application": "web", "nope": 1 })).is_err());
        assert!(args(json!({ "application": "web", "status": "all", "limit": 5 })).is_ok());
    }
}
