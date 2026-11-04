// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Where people click on a page of a RUM application and how far down they go, in numbers: the
//! elements with the most clicks and the share of views that reached each depth.

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::ToolResult;

use super::{reader, summary_span};
use crate::{
    app::{
        iam::IamContext,
        rum::{
            heatmap::{self, MapRequest},
            overview::scope_filters,
        },
        tools::{
            ToolRuntime,
            common::{TimeRangeArg, parse_args},
        },
    },
    infra::rum::query::Filter,
    shared::Result,
};

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Args {
    application: String,
    page: String,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    limit: Option<usize>,
}

pub(super) async fn get(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: Args = parse_args(arguments)?;
    let span = summary_span(args.time_range)?;
    let limit = args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let reader = reader(runtime, auth);

    let mut filters = scope_filters(&args.application, args.environment.as_deref());

    filters.push(Filter {
        field: "view.url_path_group".into(),
        op: "=".into(),
        value: args.page.trim().into(),
        quoted: false,
    });

    let request = MapRequest {
        from: Some(span.from),
        to: Some(span.to),
        filters,
        limit: Some(limit),
    };

    let clicks = heatmap::clicks(&reader, &request).await?;
    let scroll = heatmap::scroll(&reader, &request).await?;

    Ok(ToolResult::json(json!({
        "application": args.application,
        "page": args.page.trim(),
        "from": span.from,
        "to": span.to,
        "clicks": clicks.body.clicks,
        "elements": clicks.body.elements.iter().map(|element| json!({
            "element": element.element,
            "name": element.name,
            "selector": element.selector,
            "clicks": element.clicks,
            "sessions": element.sessions,
            "frustrated": element.frustrated,
        })).collect::<Vec<_>>(),
        "viewsWithScroll": scroll.body.views,
        "pageHeightPx": scroll.body.page_height,
        "medianDepthPx": scroll.body.median_depth,
        "reached": scroll.body.bands.iter().step_by(5).map(|band| json!({
            "depthPx": band.depth,
            "share": band.reached,
        })).collect::<Vec<_>>(),
        "unavailable": clicks.unavailable.or(scroll.unavailable),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_map_is_of_one_page() {
        assert!(parse_args::<Args>(json!({"application": "a"})).is_err());
        assert!(parse_args::<Args>(json!({"application": "a", "page": "/"})).is_ok());
    }
}
