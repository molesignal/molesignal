// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `get_similar_incidents` and `list_changes`: what happened before that looks like an incident,
//! and what changed around it or in a stretch of time.

use domain::{alerting::incident::Incident, apm::ChangeKind, iam::IamContext};
use kernel::{Error, Result, ids::Id};
use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::ToolResult;

use super::{
    super::{
        ToolRuntime,
        common::{TimeRangeArg, parse_args, time_range},
    },
    ensure_tenant,
};
use crate::app::alerting::related::{IncidentChange, SimilarIncident, downstream_of};

/// How many similar incidents a call lists, unless it asks for fewer or more.
const DEFAULT_SIMILAR: usize = 5;
const MOST_SIMILAR: usize = 10;
/// How many changes a call lists, unless it asks for fewer or more.
const DEFAULT_CHANGES: usize = 50;
const MOST_CHANGES: usize = 200;
/// The most services a call may name.
const MOST_SERVICES: usize = 20;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SimilarArgs {
    incident_id: String,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChangesArgs {
    #[serde(default)]
    incident_id: Option<String>,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    services: Vec<String>,
    #[serde(default)]
    kinds: Vec<String>,
    #[serde(default)]
    limit: Option<usize>,
}

pub(super) async fn similar(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: SimilarArgs = parse_args(arguments)?;
    let incident = incident(runtime, auth, &args.incident_id).await?;
    let limit = args.limit.unwrap_or(DEFAULT_SIMILAR).clamp(1, MOST_SIMILAR);

    let similar = runtime
        .alerting
        .related
        .similar_up_to(&incident, limit)
        .await?;

    Ok(ToolResult::json(json!({
        "incidentId": incident.id.0,
        "incidents": similar.iter().map(similar_value).collect::<Vec<_>>(),
    })))
}

pub(super) async fn changes(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: ChangesArgs = parse_args(arguments)?;
    let kinds = parse_kinds(&args.kinds)?;
    let limit = args.limit.unwrap_or(DEFAULT_CHANGES).clamp(1, MOST_CHANGES);

    if args.services.len() > MOST_SERVICES {
        return Err(Error::invalid(format!(
            "name at most {MOST_SERVICES} services"
        )));
    }

    let incident_id = args
        .incident_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());

    match incident_id {
        Some(id) => incident_changes(runtime, auth, id, &args.services, &kinds, limit).await,
        None => {
            range_changes(
                runtime,
                auth,
                args.time_range,
                &args.services,
                &kinds,
                limit,
            )
            .await
        }
    }
}

/// The changes around one incident: its services and the services they call, from shortly before
/// it began until shortly after, and an edit of its rule the day before. Reading the incident is
/// enough, as on the incident's page.
async fn incident_changes(
    runtime: &ToolRuntime,
    auth: &IamContext,
    incident_id: &str,
    services: &[String],
    kinds: &[ChangeKind],
    limit: usize,
) -> Result<ToolResult> {
    if !auth.has_permission("alerts.incidents.read") {
        return Err(Error::forbidden(
            "the changes around an incident need alerts.incidents.read",
        ));
    }

    let incident = incident(runtime, auth, incident_id).await?;

    let downstream = downstream_of(
        &runtime.observability.apm,
        runtime.observability.service_graph.as_ref(),
        &auth.org_id,
        &incident,
    )
    .await;

    let found = runtime
        .alerting
        .related
        .changes(&incident, &downstream)
        .await?;

    Ok(ToolResult::json(json!({
        "incidentId": incident.id.0,
        "from": found.from,
        "to": found.to,
        "changes": keep(found.changes, services, kinds, limit),
    })))
}

/// The changes the organization reported in a stretch of time; they are read as deployments are.
async fn range_changes(
    runtime: &ToolRuntime,
    auth: &IamContext,
    range: Option<TimeRangeArg>,
    services: &[String],
    kinds: &[ChangeKind],
    limit: usize,
) -> Result<ToolResult> {
    if !auth.has_permission("apm.deployments.read") && !auth.has_permission("sys.telemetry.read") {
        return Err(Error::forbidden(
            "listing changes without an incident needs apm.deployments.read",
        ));
    }

    let range = range.ok_or_else(|| {
        Error::invalid("give incidentId, or a timeRange to list the changes of a stretch of time")
    })?;

    let range = time_range(range)?;

    let changes = runtime
        .alerting
        .related
        .changes_between(
            &auth.org_id,
            (range.start, range.end),
            services,
            kinds,
            limit,
        )
        .await?;

    Ok(ToolResult::json(json!({
        "from": range.start,
        "to": range.end,
        "changes": changes,
    })))
}

async fn incident(runtime: &ToolRuntime, auth: &IamContext, id: &str) -> Result<Incident> {
    let incident = runtime
        .alerting
        .service
        .get_incident(&Id(id.trim().to_string()))
        .await?;

    ensure_tenant(auth, &incident.org_id)?;
    Ok(incident)
}

fn parse_kinds(kinds: &[String]) -> Result<Vec<ChangeKind>> {
    kinds
        .iter()
        .map(|kind| {
            ChangeKind::parse(kind.trim()).ok_or_else(|| {
                Error::invalid(format!(
                    "unknown change kind `{kind}`: use deployment, configuration or feature_flag"
                ))
            })
        })
        .collect()
}

/// The incident's changes of the named services and kinds, the latest `limit`. An edit of the
/// incident's rule stays whatever was named: it is about this incident, not about a service.
fn keep(
    changes: Vec<IncidentChange>,
    services: &[String],
    kinds: &[ChangeKind],
    limit: usize,
) -> Vec<IncidentChange> {
    let named = |service: &str| {
        services.iter().any(|wanted| {
            let wanted = wanted.trim();
            service == wanted || service.rsplit('/').next() == wanted.rsplit('/').next()
        })
    };

    let mut kept: Vec<IncidentChange> = changes
        .into_iter()
        .filter(|change| {
            change.kind == "rule"
                || ((kinds.is_empty() || kinds.iter().any(|kind| kind.as_str() == change.kind))
                    && (services.is_empty() || change.service.as_deref().is_some_and(&named)))
        })
        .collect();

    let older = kept.len().saturating_sub(limit);
    kept.drain(..older);
    kept
}

/// A similar incident, with how long it took to resolve when it was.
fn similar_value(similar: &SimilarIncident) -> Value {
    let mut value = serde_json::to_value(similar).unwrap_or(Value::Null);

    if let (Some(resolved), Some(object)) = (similar.resolved_at, value.as_object_mut()) {
        // A duration in microseconds, like every time in tool results.
        object.insert(
            "duration".into(),
            json!(resolved.0.saturating_sub(similar.created_at.0)),
        );
    }

    value
}

#[cfg(test)]
#[path = "related_tests.rs"]
mod tests;
