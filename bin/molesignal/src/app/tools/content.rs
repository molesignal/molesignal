// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::Value;
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::{
    ToolRuntime,
    common::{TimeRangeArg, bounded, json_result, parse_args, time_range},
};
use crate::{
    app::iam::IamContext,
    infra::persistence::repositories::annotations::AnnotationFilter,
    shared::{Result, ids::Id},
};

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::ListDashboards => {
            let args: DashboardListArgs = parse_args(arguments)?;
            let folder_id = args.folder_id.map(Id::from_string);
            let dashboards = runtime
                .content
                .dashboard
                .list(&auth.org_id, folder_id.as_ref())
                .await?;
            json_result(&bounded(dashboards, args.limit, 500))
        }
        BuiltinToolKind::GetDashboard => {
            let args: IdArg = parse_args(arguments)?;
            let dashboard = runtime
                .content
                .dashboard
                .get(&Id(args.required("dashboard_id")?))
                .await?;
            if dashboard.org_id != auth.org_id {
                return Err(crate::shared::Error::not_found("dashboard not found"));
            }
            json_result(&dashboard)
        }
        BuiltinToolKind::ListFolders => {
            let args: ListArgs = parse_args(arguments)?;
            let folders = runtime
                .content
                .dashboard
                .folders()
                .list(&auth.org_id)
                .await?;
            json_result(&bounded(folders, args.limit, 500))
        }
        BuiltinToolKind::GetFolder => {
            let args: IdArg = parse_args(arguments)?;
            json_result(
                &runtime
                    .content
                    .dashboard
                    .folders()
                    .get(&auth.org_id, &Id(args.required("folder_id")?))
                    .await?,
            )
        }
        BuiltinToolKind::ListAnnotations => {
            let args: AnnotationListArgs = parse_args(arguments)?;
            let range = args.time_range.map(time_range).transpose()?;
            let annotations = runtime
                .content
                .annotations
                .list(
                    &auth.org_id,
                    AnnotationFilter {
                        dashboard_id: args.dashboard_id.as_deref(),
                        from_micros: range.as_ref().map(|range| range.start.0),
                        to_micros: range.as_ref().map(|range| range.end.0),
                        ..AnnotationFilter::default()
                    },
                )
                .await?;
            json_result(&bounded(annotations, args.limit, 500))
        }
        BuiltinToolKind::GetAnnotation => {
            let args: IdArg = parse_args(arguments)?;
            json_result(
                &runtime
                    .content
                    .annotations
                    .get(&auth.org_id, &Id(args.required("annotation_id")?))
                    .await?,
            )
        }
        _ => unreachable!("content handler received unrelated tool"),
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DashboardListArgs {
    #[serde(default)]
    folder_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnnotationListArgs {
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    dashboard_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdArg {
    dashboard_id: Option<String>,
    folder_id: Option<String>,
    annotation_id: Option<String>,
}

impl IdArg {
    fn required(&self, field: &str) -> Result<String> {
        let value = match field {
            "dashboard_id" => &self.dashboard_id,
            "folder_id" => &self.folder_id,
            "annotation_id" => &self.annotation_id,
            _ => unreachable!(),
        };
        value
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .ok_or_else(|| crate::shared::Error::invalid(format!("{field} is required")))
    }
}
