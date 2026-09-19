// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::{
    ToolRuntime,
    common::{TimeRangeArg, optional_time_range, parse_args},
};
use crate::{
    app::iam::IamContext,
    domain::alerting::incident::{Incident, IncidentStatus},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::ListRecentAlerts => list_recent(runtime, auth, arguments).await,
        BuiltinToolKind::ListIncidents => list_incidents(runtime, auth, arguments).await,
        BuiltinToolKind::GetIncident => get_incident(runtime, auth, arguments).await,
        BuiltinToolKind::GetIncidentRca => get_incident_rca(runtime, auth, arguments).await,
        BuiltinToolKind::GetIncidentInsights => {
            get_incident_insights(runtime, auth, arguments).await
        }
        BuiltinToolKind::ListOnCallSchedules => {
            list_on_call_schedules(runtime, auth, arguments).await
        }
        BuiltinToolKind::GetOnCallSchedule => get_on_call_schedule(runtime, auth, arguments).await,
        BuiltinToolKind::GetCurrentOnCall => current_on_call(runtime, auth, arguments).await,
        _ => unreachable!("incident handler received unrelated tool"),
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecentArgs {
    #[serde(default)]
    limit: Option<usize>,
}

async fn list_recent(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: RecentArgs = parse_args(arguments)?;
    let mut incidents = runtime
        .alerting
        .service
        .list_incidents_active(&auth.org_id)
        .await?;
    sort_and_limit(&mut incidents, args.limit.unwrap_or(50).clamp(1, 500));
    Ok(ToolResult::json(
        json!({"incidents": incidents, "status": "active"}),
    ))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListIncidentsArgs {
    #[serde(default = "active_status")]
    status: String,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    limit: Option<usize>,
}

fn active_status() -> String {
    "active".into()
}

async fn list_incidents(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: ListIncidentsArgs = parse_args(arguments)?;
    let mut incidents = match args.status.as_str() {
        "active" => {
            runtime
                .alerting
                .service
                .list_incidents_active(&auth.org_id)
                .await?
        }
        "resolved" => {
            let mut incidents = runtime
                .alerting
                .service
                .list_incidents_by_status(&auth.org_id, IncidentStatus::Resolved)
                .await?;
            incidents.extend(
                runtime
                    .alerting
                    .service
                    .list_incidents_by_status(&auth.org_id, IncidentStatus::Closed)
                    .await?,
            );
            incidents
        }
        "all" => {
            runtime
                .alerting
                .service
                .incidents
                .list_since(&auth.org_id, TimestampMicros(0))
                .await?
        }
        _ => return Err(Error::invalid("status must be active, resolved, or all")),
    };
    if args.time_range.is_some() {
        let range = optional_time_range(args.time_range)?;
        incidents.retain(|incident| {
            incident.created_at >= range.start && incident.created_at <= range.end
        });
    }
    let limit = args.limit.unwrap_or(50).clamp(1, 500);
    sort_and_limit(&mut incidents, limit);
    Ok(ToolResult::json(json!({
        "incidents": incidents, "status": args.status, "limit": limit,
    })))
}

fn sort_and_limit(incidents: &mut Vec<Incident>, limit: usize) {
    incidents.sort_by_key(|incident| std::cmp::Reverse(incident.created_at.0));
    incidents.truncate(limit);
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IncidentArgs {
    incident_id: String,
}

async fn get_incident(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: IncidentArgs = parse_args(arguments)?;
    let incident = runtime
        .alerting
        .service
        .get_incident(&Id(args.incident_id))
        .await?;
    ensure_tenant(auth, &incident.org_id)?;
    let rule = runtime
        .alerting
        .service
        .get_rule(&incident.rule_id)
        .await
        .ok();
    let rca = runtime.alerting.incident_rca.get(&incident.id).await?;
    Ok(ToolResult::json(json!({
        "incident": incident, "rule": rule, "rca_available": rca.is_some(),
    })))
}

async fn get_incident_rca(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: IncidentArgs = parse_args(arguments)?;
    let incident_id = Id(args.incident_id);
    let incident = runtime.alerting.service.get_incident(&incident_id).await?;
    ensure_tenant(auth, &incident.org_id)?;
    let rca = runtime.alerting.incident_rca.get(&incident_id).await?;
    Ok(ToolResult::json(json!({
        "incident_id": incident_id, "rca": rca,
        "available": rca.is_some(),
    })))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct IncidentInsightsArgs {
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn get_incident_insights(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: IncidentInsightsArgs = parse_args(arguments)?;
    let range = optional_time_range(args.time_range)?;
    let incidents = runtime
        .alerting
        .service
        .incidents
        .list_since(&auth.org_id, range.start)
        .await?
        .into_iter()
        .filter(|incident| incident.created_at <= range.end)
        .collect::<Vec<_>>();
    let mut status = BTreeMap::new();
    let mut severity = BTreeMap::new();
    let mut services = BTreeMap::new();
    let mut rules = BTreeMap::new();
    for incident in &incidents {
        increment(&mut status, incident.status.as_str());
        increment(&mut severity, incident.severity.as_str());
        increment(&mut rules, incident.rule_id.as_str());
        if incident.affected_services.is_empty() {
            increment(&mut services, "unknown");
        } else {
            for service in &incident.affected_services {
                increment(&mut services, service);
            }
        }
    }
    let limit = args.limit.unwrap_or(20).clamp(1, 100);
    Ok(ToolResult::json(json!({
        "time_range": range,
        "incident_count": incidents.len(),
        "by_status": status,
        "by_severity": severity,
        "top_services": top_counts(services, limit),
        "top_rules": top_counts(rules, limit),
    })))
}

fn increment(counts: &mut BTreeMap<String, usize>, value: &str) {
    *counts.entry(value.to_string()).or_default() += 1;
}

fn top_counts(counts: BTreeMap<String, usize>, limit: usize) -> Vec<Value> {
    let mut rows = counts
        .into_iter()
        .map(|(value, count)| json!({"value": value, "count": count}))
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| {
        std::cmp::Reverse(row.get("count").and_then(Value::as_u64).unwrap_or_default())
    });
    rows.truncate(limit);
    rows
}

fn ensure_tenant(auth: &IamContext, org_id: &Id) -> Result<()> {
    if &auth.org_id == org_id {
        Ok(())
    } else {
        Err(Error::not_found("incident not found"))
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScheduleArgs {
    #[serde(default)]
    enabled_only: Option<bool>,
    #[serde(default)]
    at_micros: Option<i64>,
}

async fn list_on_call_schedules(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: ScheduleArgs = parse_args(arguments)?;
    let at = TimestampMicros(args.at_micros.unwrap_or_else(|| TimestampMicros::now().0));
    let mut rows = Vec::new();
    for schedule in runtime
        .alerting
        .service
        .list_schedules(&auth.org_id)
        .await?
    {
        if args.enabled_only.unwrap_or(true) && !schedule.enabled {
            continue;
        }
        let user = match schedule.who_is_on_call(at) {
            Some(user_id) => on_call_user(runtime, &user_id).await,
            None => Value::Null,
        };
        rows.push(json!({
            "schedule_id": schedule.id, "name": schedule.name,
            "description": schedule.description, "team_id": schedule.team_id,
            "timezone": schedule.timezone, "enabled": schedule.enabled,
            "rotation_count": schedule.rotations.len(),
            "override_count": schedule.overrides.len(), "current_on_call": user,
        }));
    }
    Ok(ToolResult::json(json!({
        "at_micros": at, "schedule_count": rows.len(), "schedules": rows,
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetScheduleArgs {
    schedule_id: String,
}

async fn get_on_call_schedule(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: GetScheduleArgs = parse_args(arguments)?;
    let schedule = runtime
        .alerting
        .service
        .get_schedule(&Id(args.schedule_id))
        .await?;
    if schedule.org_id != auth.org_id {
        return Err(Error::not_found("schedule not found"));
    }
    Ok(ToolResult::json(json!({"schedule": schedule})))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CurrentOnCallArgs {
    #[serde(default)]
    schedule_id: Option<String>,
    #[serde(default)]
    at_micros: Option<i64>,
}

async fn current_on_call(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: CurrentOnCallArgs = parse_args(arguments)?;
    let at = TimestampMicros(args.at_micros.unwrap_or_else(|| TimestampMicros::now().0));
    let schedules = if let Some(id) = args.schedule_id {
        let schedule = runtime.alerting.service.get_schedule(&Id(id)).await?;
        if schedule.org_id != auth.org_id {
            return Err(Error::not_found("schedule not found"));
        }
        vec![schedule]
    } else {
        runtime
            .alerting
            .service
            .list_schedules(&auth.org_id)
            .await?
    };
    let mut rows = Vec::new();
    for schedule in schedules.into_iter().filter(|schedule| schedule.enabled) {
        if let Some(user_id) = schedule.who_is_on_call(at) {
            rows.push(json!({
                "schedule_id": schedule.id, "schedule_name": schedule.name,
                "at_micros": at, "user": on_call_user(runtime, &user_id).await,
            }));
        }
    }
    Ok(ToolResult::json(json!({"at_micros": at, "on_call": rows})))
}

async fn on_call_user(runtime: &ToolRuntime, user_id: &Id) -> Value {
    let user = runtime.administration.iam.current_user(user_id).await.ok();
    json!({
        "id": user_id,
        "display_name": user.as_ref().map(|user| user.display_name.as_str()),
        "avatar_url": user.as_ref().and_then(|user| user.avatar_url.as_deref()),
    })
}
