// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use sqlx::Row;

use crate::{
    domain::synthetics::{
        AgentCapacity, AgentStatus, EgressPolicy, LocationExecution, LocationHealth,
        LocationLifecycle, LocationScope, MonitorLifecycle, MonitorRevision, MonitorSchedule,
        MonitorSpec, MonitorState, MultiLocationPolicy, ProbeAgent, ProbeCapability, ProbeLocation,
        ProbeOutcome, ProbeTask, ProbeTaskState, SyntheticMonitor, SyntheticResult,
        SyntheticSecret,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) const LOCATION_COLS: &str = "id, organization_id, name, code, description, scope,
    execution, lifecycle, health, system_managed, egress_policy,
    created_at, updated_at";

pub(super) const AGENT_COLS: &str = "agent.id AS id,
    location.organization_id AS organization_id, agent.location_id AS location_id,
    agent.name AS name, agent.hostname AS hostname, agent.status AS status,
    agent.agent_version AS agent_version, agent.protocol_version AS protocol_version,
    agent.feature_level AS feature_level,
    agent.capabilities AS capabilities, agent.capacity AS capacity, agent.labels AS labels,
    agent.public_key_der AS public_key_der, agent.certificate_serial AS certificate_serial,
    agent.certificate_expires_at AS certificate_expires_at,
    agent.last_heartbeat_at AS last_heartbeat_at,
    agent.last_result_sequence AS last_result_sequence,
    agent.revoked_at AS revoked_at,
    agent.created_at AS created_at,
    agent.updated_at AS updated_at";

pub(super) const CONFIGURED_AGENT_COLS: &str = "agent.id AS id,
    location.organization_id AS organization_id, agent.location_id AS location_id,
    COALESCE(configuration.name, agent.name) AS name,
    agent.hostname AS hostname, agent.status AS status,
    agent.agent_version AS agent_version, agent.protocol_version AS protocol_version,
    agent.feature_level AS feature_level,
    agent.capabilities AS capabilities, agent.capacity AS capacity,
    COALESCE(configuration.labels, agent.labels) AS labels,
    agent.public_key_der AS public_key_der, agent.certificate_serial AS certificate_serial,
    agent.certificate_expires_at AS certificate_expires_at,
    agent.last_heartbeat_at AS last_heartbeat_at,
    agent.last_result_sequence AS last_result_sequence,
    agent.revoked_at AS revoked_at,
    agent.created_at AS created_at,
    GREATEST(
        agent.updated_at,
        COALESCE(configuration.updated_at, agent.updated_at)
    ) AS updated_at";

pub(super) const SECRET_COLS: &str = "id, organization_id, name, description, current_version,
    created_by, created_at, updated_at, archived_at";

pub(super) const MONITOR_COLS: &str = "id, organization_id, name, description, kind, lifecycle,
    state, team_id, tags, active_revision_id, draft_revision_id, next_due_at,
    created_by, created_at, updated_at, archived_at";

pub(super) const REVISION_COLS: &str = "id, organization_id, monitor_id, revision_number,
    spec, schedule, timeout_millis, max_retries, consecutive_failures, consecutive_recoveries,
    freshness_seconds, location_policy, location_ids, escalation_policy_id, alert_on_degraded,
    alert_on_flaky, alert_on_no_data, inject_trace_context, traceroute_on_failure,
    last_test_result_id, last_test_passed_at, created_by, created_at, content_hash";

pub(super) const TASK_COLS: &str = "id, organization_id, monitor_id, monitor_revision_id,
    location_id, spec, is_test, state, scheduled_at, deadline_at, timeout_millis,
    max_attempts, inject_trace_context, traceroute_on_failure, lease_agent_id, lease_token_hash,
    leased_until, created_at, updated_at";

pub(super) const RESULT_COLS: &str = "id, organization_id, monitor_id, monitor_revision_id,
    location_id, agent_id, task_id, is_test, result_sequence, scheduled_at, started_at,
    finished_at, received_at, outcome, attempts, assertions, secret_versions,
    protocol_version, metadata";

fn parse<T>(value: &str, parser: impl FnOnce(&str) -> Option<T>, label: &str) -> Result<T> {
    parser(value).ok_or_else(|| Error::internal(format!("unknown {label}: {value}")))
}

pub(super) fn row_to_location(row: sqlx::postgres::PgRow) -> Result<ProbeLocation> {
    let scope: String = row.try_get("scope").map_err(super::sqlx_err)?;
    let execution: String = row.try_get("execution").map_err(super::sqlx_err)?;
    let lifecycle: String = row.try_get("lifecycle").map_err(super::sqlx_err)?;
    let health: String = row.try_get("health").map_err(super::sqlx_err)?;
    let egress: serde_json::Value = row.try_get("egress_policy").map_err(super::sqlx_err)?;

    Ok(ProbeLocation {
        id: Id(row.try_get("id").map_err(super::sqlx_err)?),
        organization_id: row
            .try_get::<Option<String>, _>("organization_id")
            .map_err(super::sqlx_err)?
            .map(Id),
        name: row.try_get("name").map_err(super::sqlx_err)?,
        code: row.try_get("code").map_err(super::sqlx_err)?,
        description: row.try_get("description").map_err(super::sqlx_err)?,
        scope: parse(&scope, LocationScope::parse, "Location scope")?,
        execution: parse(&execution, LocationExecution::parse, "Location execution")?,
        lifecycle: parse(&lifecycle, LocationLifecycle::parse, "Location lifecycle")?,
        health: parse(&health, LocationHealth::parse, "Location health")?,
        system_managed: row.try_get("system_managed").map_err(super::sqlx_err)?,
        egress_policy: serde_json::from_value::<EgressPolicy>(egress)
            .map_err(|error| Error::internal(format!("decode Location egress policy: {error}")))?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(super::sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(super::sqlx_err)?,
    })
}

pub(super) fn row_to_agent(row: sqlx::postgres::PgRow) -> Result<ProbeAgent> {
    let status: String = row.try_get("status").map_err(super::sqlx_err)?;
    let capabilities: Vec<String> = row.try_get("capabilities").map_err(super::sqlx_err)?;
    let capacity: serde_json::Value = row.try_get("capacity").map_err(super::sqlx_err)?;
    let labels: serde_json::Value = row.try_get("labels").map_err(super::sqlx_err)?;

    Ok(ProbeAgent {
        id: Id(row.try_get("id").map_err(super::sqlx_err)?),
        organization_id: row
            .try_get::<Option<String>, _>("organization_id")
            .map_err(super::sqlx_err)?
            .map(Id),
        location_id: Id(row.try_get("location_id").map_err(super::sqlx_err)?),
        name: row.try_get("name").map_err(super::sqlx_err)?,
        hostname: row.try_get("hostname").map_err(super::sqlx_err)?,
        status: parse(&status, AgentStatus::parse, "Probe Agent status")?,
        agent_version: row.try_get("agent_version").map_err(super::sqlx_err)?,
        protocol_version: u32::try_from(
            row.try_get::<i32, _>("protocol_version")
                .map_err(super::sqlx_err)?,
        )
        .map_err(|_| Error::internal("negative Probe protocol version"))?,
        feature_level: u32::try_from(
            row.try_get::<i32, _>("feature_level")
                .map_err(super::sqlx_err)?,
        )
        .map_err(|_| Error::internal("negative Probe feature level"))?,
        capabilities: capabilities
            .into_iter()
            .map(|value| parse(&value, ProbeCapability::parse, "Probe capability"))
            .collect::<Result<Vec<_>>>()?,
        capacity: serde_json::from_value::<AgentCapacity>(capacity)
            .map_err(|error| Error::internal(format!("decode Agent capacity: {error}")))?,
        labels: serde_json::from_value(labels)
            .map_err(|error| Error::internal(format!("decode Agent labels: {error}")))?,
        public_key_der: row.try_get("public_key_der").map_err(super::sqlx_err)?,
        certificate_serial: row.try_get("certificate_serial").map_err(super::sqlx_err)?,
        certificate_expires_at: row
            .try_get::<Option<TimestampMicros>, _>("certificate_expires_at")
            .map_err(super::sqlx_err)?,
        last_heartbeat_at: row
            .try_get::<Option<TimestampMicros>, _>("last_heartbeat_at")
            .map_err(super::sqlx_err)?,
        last_result_sequence: u64::try_from(
            row.try_get::<i64, _>("last_result_sequence")
                .map_err(super::sqlx_err)?,
        )
        .map_err(|_| Error::internal("negative Agent result sequence"))?,
        revoked_at: row
            .try_get::<Option<TimestampMicros>, _>("revoked_at")
            .map_err(super::sqlx_err)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(super::sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(super::sqlx_err)?,
    })
}

pub(super) fn row_to_secret(row: sqlx::postgres::PgRow) -> Result<SyntheticSecret> {
    Ok(SyntheticSecret {
        id: Id(row.try_get("id").map_err(super::sqlx_err)?),
        organization_id: Id(row.try_get("organization_id").map_err(super::sqlx_err)?),
        name: row.try_get("name").map_err(super::sqlx_err)?,
        description: row.try_get("description").map_err(super::sqlx_err)?,
        current_version: row
            .try_get::<i32, _>("current_version")
            .map_err(super::sqlx_err)? as u32,
        created_by: Id(row.try_get("created_by").map_err(super::sqlx_err)?),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(super::sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(super::sqlx_err)?,
        archived_at: row
            .try_get::<Option<TimestampMicros>, _>("archived_at")
            .map_err(super::sqlx_err)?,
    })
}

pub(super) fn row_to_monitor(row: sqlx::postgres::PgRow) -> Result<SyntheticMonitor> {
    let kind: String = row.try_get("kind").map_err(super::sqlx_err)?;
    let lifecycle: String = row.try_get("lifecycle").map_err(super::sqlx_err)?;
    let state: String = row.try_get("state").map_err(super::sqlx_err)?;

    Ok(SyntheticMonitor {
        id: Id(row.try_get("id").map_err(super::sqlx_err)?),
        organization_id: Id(row.try_get("organization_id").map_err(super::sqlx_err)?),
        name: row.try_get("name").map_err(super::sqlx_err)?,
        description: row.try_get("description").map_err(super::sqlx_err)?,
        kind: parse(
            &kind,
            crate::domain::synthetics::MonitorKind::parse,
            "Monitor kind",
        )?,
        lifecycle: parse(&lifecycle, MonitorLifecycle::parse, "Monitor lifecycle")?,
        state: parse(&state, MonitorState::parse, "Monitor state")?,
        team_id: row
            .try_get::<Option<String>, _>("team_id")
            .map_err(super::sqlx_err)?
            .map(Id),
        tags: row.try_get("tags").map_err(super::sqlx_err)?,
        active_revision_id: row
            .try_get::<Option<String>, _>("active_revision_id")
            .map_err(super::sqlx_err)?
            .map(Id),
        draft_revision_id: row
            .try_get::<Option<String>, _>("draft_revision_id")
            .map_err(super::sqlx_err)?
            .map(Id),
        next_due_at: row
            .try_get::<Option<TimestampMicros>, _>("next_due_at")
            .map_err(super::sqlx_err)?,
        created_by: Id(row.try_get("created_by").map_err(super::sqlx_err)?),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(super::sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(super::sqlx_err)?,
        archived_at: row
            .try_get::<Option<TimestampMicros>, _>("archived_at")
            .map_err(super::sqlx_err)?,
    })
}

pub(super) fn row_to_revision(row: sqlx::postgres::PgRow) -> Result<MonitorRevision> {
    let spec: serde_json::Value = row.try_get("spec").map_err(super::sqlx_err)?;
    let location_ids: Vec<String> = row.try_get("location_ids").map_err(super::sqlx_err)?;
    let schedule: serde_json::Value = row.try_get("schedule").map_err(super::sqlx_err)?;
    let policy: serde_json::Value = row.try_get("location_policy").map_err(super::sqlx_err)?;

    Ok(MonitorRevision {
        id: Id(row.try_get("id").map_err(super::sqlx_err)?),
        organization_id: Id(row.try_get("organization_id").map_err(super::sqlx_err)?),
        monitor_id: Id(row.try_get("monitor_id").map_err(super::sqlx_err)?),
        number: row
            .try_get::<i32, _>("revision_number")
            .map_err(super::sqlx_err)? as u32,
        spec: serde_json::from_value::<MonitorSpec>(spec)
            .map_err(|error| Error::internal(format!("decode Monitor spec: {error}")))?,
        schedule: serde_json::from_value::<MonitorSchedule>(schedule)
            .map_err(|error| Error::internal(format!("decode Monitor schedule: {error}")))?,
        timeout_millis: row
            .try_get::<i32, _>("timeout_millis")
            .map_err(super::sqlx_err)? as u32,
        max_retries: row
            .try_get::<i16, _>("max_retries")
            .map_err(super::sqlx_err)? as u8,
        consecutive_failures: row
            .try_get::<i32, _>("consecutive_failures")
            .map_err(super::sqlx_err)? as u32,
        consecutive_recoveries: row
            .try_get::<i32, _>("consecutive_recoveries")
            .map_err(super::sqlx_err)? as u32,
        freshness_seconds: row
            .try_get::<i32, _>("freshness_seconds")
            .map_err(super::sqlx_err)? as u32,
        location_policy: serde_json::from_value::<MultiLocationPolicy>(policy)
            .map_err(|error| Error::internal(format!("decode Location policy: {error}")))?,
        location_ids: location_ids.into_iter().map(Id).collect(),
        escalation_policy_id: row
            .try_get::<Option<String>, _>("escalation_policy_id")
            .map_err(super::sqlx_err)?
            .map(Id),
        alert_on_degraded: row.try_get("alert_on_degraded").map_err(super::sqlx_err)?,
        alert_on_flaky: row.try_get("alert_on_flaky").map_err(super::sqlx_err)?,
        alert_on_no_data: row.try_get("alert_on_no_data").map_err(super::sqlx_err)?,
        inject_trace_context: row
            .try_get("inject_trace_context")
            .map_err(super::sqlx_err)?,
        traceroute_on_failure: row
            .try_get("traceroute_on_failure")
            .map_err(super::sqlx_err)?,
        last_test_result_id: row
            .try_get::<Option<String>, _>("last_test_result_id")
            .map_err(super::sqlx_err)?
            .map(Id),
        last_test_passed_at: row
            .try_get::<Option<TimestampMicros>, _>("last_test_passed_at")
            .map_err(super::sqlx_err)?,
        created_by: Id(row.try_get("created_by").map_err(super::sqlx_err)?),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(super::sqlx_err)?,
        content_hash: row.try_get("content_hash").map_err(super::sqlx_err)?,
    })
}

pub(super) fn row_to_task(row: sqlx::postgres::PgRow) -> Result<ProbeTask> {
    let spec: serde_json::Value = row.try_get("spec").map_err(super::sqlx_err)?;
    let state: String = row.try_get("state").map_err(super::sqlx_err)?;

    Ok(ProbeTask {
        id: Id(row.try_get("id").map_err(super::sqlx_err)?),
        organization_id: Id(row.try_get("organization_id").map_err(super::sqlx_err)?),
        monitor_id: Id(row.try_get("monitor_id").map_err(super::sqlx_err)?),
        monitor_revision_id: Id(row
            .try_get("monitor_revision_id")
            .map_err(super::sqlx_err)?),
        location_id: Id(row.try_get("location_id").map_err(super::sqlx_err)?),
        spec: serde_json::from_value(spec)
            .map_err(|error| Error::internal(format!("decode Probe task spec: {error}")))?,
        is_test: row.try_get("is_test").map_err(super::sqlx_err)?,
        state: parse(&state, ProbeTaskState::parse, "Probe task state")?,
        scheduled_at: row
            .try_get::<TimestampMicros, _>("scheduled_at")
            .map_err(super::sqlx_err)?,
        deadline_at: row
            .try_get::<TimestampMicros, _>("deadline_at")
            .map_err(super::sqlx_err)?,
        timeout_millis: row
            .try_get::<i32, _>("timeout_millis")
            .map_err(super::sqlx_err)? as u32,
        max_attempts: row
            .try_get::<i16, _>("max_attempts")
            .map_err(super::sqlx_err)? as u8,
        inject_trace_context: row
            .try_get("inject_trace_context")
            .map_err(super::sqlx_err)?,
        traceroute_on_failure: row
            .try_get("traceroute_on_failure")
            .map_err(super::sqlx_err)?,
        lease_agent_id: row
            .try_get::<Option<String>, _>("lease_agent_id")
            .map_err(super::sqlx_err)?
            .map(Id),
        lease_token_hash: row.try_get("lease_token_hash").map_err(super::sqlx_err)?,
        leased_until: row
            .try_get::<Option<TimestampMicros>, _>("leased_until")
            .map_err(super::sqlx_err)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(super::sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(super::sqlx_err)?,
    })
}

pub(super) fn row_to_result(row: sqlx::postgres::PgRow) -> Result<SyntheticResult> {
    let outcome: String = row.try_get("outcome").map_err(super::sqlx_err)?;
    let attempts: serde_json::Value = row.try_get("attempts").map_err(super::sqlx_err)?;
    let assertions: serde_json::Value = row.try_get("assertions").map_err(super::sqlx_err)?;
    let secret_versions: serde_json::Value =
        row.try_get("secret_versions").map_err(super::sqlx_err)?;
    let metadata: serde_json::Value = row.try_get("metadata").map_err(super::sqlx_err)?;

    Ok(SyntheticResult {
        id: Id(row.try_get("id").map_err(super::sqlx_err)?),
        organization_id: Id(row.try_get("organization_id").map_err(super::sqlx_err)?),
        monitor_id: Id(row.try_get("monitor_id").map_err(super::sqlx_err)?),
        monitor_revision_id: Id(row
            .try_get("monitor_revision_id")
            .map_err(super::sqlx_err)?),
        location_id: Id(row.try_get("location_id").map_err(super::sqlx_err)?),
        agent_id: row
            .try_get::<Option<String>, _>("agent_id")
            .map_err(super::sqlx_err)?
            .map(Id),
        task_id: Id(row.try_get("task_id").map_err(super::sqlx_err)?),
        is_test: row.try_get("is_test").map_err(super::sqlx_err)?,
        result_sequence: row
            .try_get::<Option<i64>, _>("result_sequence")
            .map_err(super::sqlx_err)?
            .map(|value| value as u64),
        scheduled_at: row
            .try_get::<TimestampMicros, _>("scheduled_at")
            .map_err(super::sqlx_err)?,
        started_at: row
            .try_get::<TimestampMicros, _>("started_at")
            .map_err(super::sqlx_err)?,
        finished_at: row
            .try_get::<TimestampMicros, _>("finished_at")
            .map_err(super::sqlx_err)?,
        received_at: row
            .try_get::<TimestampMicros, _>("received_at")
            .map_err(super::sqlx_err)?,
        outcome: parse(&outcome, ProbeOutcome::parse, "Probe outcome")?,
        attempts: serde_json::from_value(attempts)
            .map_err(|error| Error::internal(format!("decode Probe attempts: {error}")))?,
        assertions: serde_json::from_value(assertions)
            .map_err(|error| Error::internal(format!("decode Probe assertions: {error}")))?,
        artifacts: Vec::new(),
        secret_versions: serde_json::from_value(secret_versions)
            .map_err(|error| Error::internal(format!("decode Secret versions: {error}")))?,
        protocol_version: row
            .try_get::<i32, _>("protocol_version")
            .map_err(super::sqlx_err)? as u32,
        metadata: serde_json::from_value(metadata)
            .map_err(|error| Error::internal(format!("decode Probe metadata: {error}")))?,
    })
}
