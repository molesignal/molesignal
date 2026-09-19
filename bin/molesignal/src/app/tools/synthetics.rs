// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::{
    ToolRuntime,
    common::{TimeRangeArg, bounded, json_result, parse_args, time_range},
};
use crate::{
    app::iam::IamContext,
    domain::synthetics::{MonitorLifecycle, SyntheticResultListQuery},
    shared::{Result, ids::Id},
};

#[derive(Debug, Default, Deserialize)]
struct SyntheticArgs {
    monitor_id: Option<String>,
    location_id: Option<String>,
    lifecycle: Option<MonitorLifecycle>,
    time_range: Option<TimeRangeArg>,
    limit: Option<usize>,
}

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: serde_json::Value,
) -> Result<ToolResult> {
    let args: SyntheticArgs = parse_args(arguments)?;
    match kind {
        BuiltinToolKind::ListSyntheticMonitors => {
            let monitors = runtime.synthetics.list_monitors(&auth.org_id).await?;
            let filtered = monitors
                .into_iter()
                .filter(|monitor| {
                    args.lifecycle
                        .is_none_or(|value| monitor.lifecycle == value)
                })
                .collect();
            json_result(&bounded(filtered, args.limit, 500))
        }
        BuiltinToolKind::GetSyntheticMonitor => {
            let monitor_id = required_id(args.monitor_id, "monitor_id")?;
            let monitor = runtime
                .synthetics
                .get_monitor(&auth.org_id, &monitor_id)
                .await?;
            let revisions = runtime
                .synthetics
                .list_revisions(&auth.org_id, &monitor_id)
                .await?;
            Ok(ToolResult::json(serde_json::json!({
                "monitor": monitor,
                "revisions": revisions,
            })))
        }
        BuiltinToolKind::ListSyntheticRevisions => {
            let monitor_id = required_id(args.monitor_id, "monitor_id")?;
            let revisions = runtime
                .synthetics
                .list_revisions(&auth.org_id, &monitor_id)
                .await?;
            json_result(&bounded(revisions, args.limit, 200))
        }
        BuiltinToolKind::ListSyntheticResults => {
            let limit = args.limit.unwrap_or(100).clamp(1, 500);
            let range = args.time_range.map(time_range).transpose()?;
            let (results, total) = if let Some(monitor_id) = args.monitor_id {
                let results = runtime
                    .synthetics
                    .list_results(
                        &auth.org_id,
                        &Id(monitor_id),
                        range.as_ref().map(|range| range.end),
                        u32::try_from(limit).unwrap_or(500),
                    )
                    .await?;
                let total = u64::try_from(results.len()).unwrap_or(u64::MAX);
                (results, total)
            } else {
                let page = runtime
                    .synthetics
                    .list_results_page(
                        &auth.org_id,
                        &SyntheticResultListQuery {
                            location_id: args.location_id.clone().map(Id),
                            limit: u32::try_from(limit).unwrap_or(500),
                            ..SyntheticResultListQuery::default()
                        },
                    )
                    .await?;
                (page.items, page.total)
            };
            let items = results
                .into_iter()
                .filter(|result| {
                    args.location_id
                        .as_deref()
                        .is_none_or(|id| result.location_id.as_str() == id)
                        && range.as_ref().is_none_or(|range| {
                            result.finished_at >= range.start && result.finished_at <= range.end
                        })
                })
                .collect::<Vec<_>>();
            Ok(ToolResult::json(serde_json::json!({
                "items": items,
                "total_before_local_filters": total,
            })))
        }
        BuiltinToolKind::ListSyntheticLocations => {
            let locations = runtime.synthetics.list_locations(&auth.org_id).await?;
            json_result(&bounded(locations, args.limit, 500))
        }
        BuiltinToolKind::ListSyntheticAgents => {
            let agents = if let Some(location_id) = args.location_id {
                runtime
                    .synthetics
                    .list_agents(&auth.org_id, &Id(location_id))
                    .await?
            } else {
                runtime.synthetics.list_all_agents(&auth.org_id).await?
            };
            json_result(&bounded(agents, args.limit, 500))
        }
        BuiltinToolKind::ListSyntheticSecrets => {
            let secrets = runtime.synthetics.list_secrets(&auth.org_id).await?;
            json_result(&bounded(secrets, args.limit, 500))
        }
        _ => unreachable!("synthetics executor received unrelated tool"),
    }
}

fn required_id(value: Option<String>, field: &str) -> Result<Id> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(Id)
        .ok_or_else(|| crate::shared::Error::invalid(format!("{field} is required")))
}
