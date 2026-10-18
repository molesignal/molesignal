// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 列清单与行到领域类型的转换。

use serde_json::Value;
use sqlx::{Row, postgres::PgRow, types::Json};

use super::super::super::sqlx_err;
use crate::{
    agent::{
        mcp::{McpPrompt, McpPromptArgument, NamedValue},
        model::RiskLevel,
        scope::Scope,
        tool_control::{
            ManagedToolStatus, McpServer, McpTool, MemberCredential, MemberCredentialSpec,
            ToolExecutionMode, ToolPolicy, ToolPolicyDefaults,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) const POLICY_COLS: &str = "org_id,tool_name,enabled,execution_mode,environment_overrides,\
    timeout_ms,max_calls_per_run,max_response_bytes,updated_by,created_at_micros,updated_at_micros";

pub(super) const DEFAULTS_COLS: &str = "org_id,risk_modes,environment_overrides,updated_by,\
    created_at_micros,updated_at_micros";

pub(super) const SERVER_COLS: &str = "id,org_id,scope,team_id,owner_id,handle,name,transport,\
    endpoint_url,command_template,auth_type,auth_header,credential_last4,credential_set,\
    member_credential,headers,env,private_only,allowed_domains,allowed_cidrs,follow_redirects,\
    tls_verify,timeout_ms,max_response_bytes,enabled,status,last_error,last_tested_at_micros,\
    last_synced_at_micros,created_by,created_at_micros,updated_at_micros";

pub(super) const MCP_TOOL_COLS: &str = "id,org_id,server_id,remote_name,name,display_name,description,\
    input_schema,schema_hash,schema_dialect,schema_synced_at_micros,unavailable_diagnostic,\
    output_schema,minimum_risk,risk,execution_mode,capabilities,tags,enabled,status,\
    version,last_synced_at_micros,created_at_micros,updated_at_micros";

pub(super) const PROMPT_COLS: &str =
    "org_id,server_id,name,title,description,arguments,synced_at_micros";

fn parse_execution_mode(value: &str) -> Result<ToolExecutionMode> {
    match value {
        "automatic" => Ok(ToolExecutionMode::Automatic),
        "confirmation" => Ok(ToolExecutionMode::Confirmation),
        "single_approval" => Ok(ToolExecutionMode::SingleApproval),
        "dual_approval" => Ok(ToolExecutionMode::DualApproval),
        "disabled" => Ok(ToolExecutionMode::Disabled),
        other => Err(Error::internal(format!(
            "invalid stored tool execution mode `{other}`"
        ))),
    }
}

pub(super) fn execution_mode_string(value: ToolExecutionMode) -> &'static str {
    match value {
        ToolExecutionMode::Automatic => "automatic",
        ToolExecutionMode::Confirmation => "confirmation",
        ToolExecutionMode::SingleApproval => "single_approval",
        ToolExecutionMode::DualApproval => "dual_approval",
        ToolExecutionMode::Disabled => "disabled",
    }
}

fn parse_risk(value: &str) -> Result<RiskLevel> {
    match value {
        "l0" => Ok(RiskLevel::L0),
        "l1" => Ok(RiskLevel::L1),
        "l2" => Ok(RiskLevel::L2),
        "l3" => Ok(RiskLevel::L3),
        "l4" => Ok(RiskLevel::L4),
        other => Err(Error::internal(format!("invalid stored risk `{other}`"))),
    }
}

pub(super) fn risk_string(value: RiskLevel) -> &'static str {
    match value {
        RiskLevel::L0 => "l0",
        RiskLevel::L1 => "l1",
        RiskLevel::L2 => "l2",
        RiskLevel::L3 => "l3",
        RiskLevel::L4 => "l4",
    }
}

fn parse_status(value: &str) -> Result<ManagedToolStatus> {
    match value {
        "healthy" => Ok(ManagedToolStatus::Healthy),
        "degraded" => Ok(ManagedToolStatus::Degraded),
        "unavailable" => Ok(ManagedToolStatus::Unavailable),
        "disabled" => Ok(ManagedToolStatus::Disabled),
        other => Err(Error::internal(format!(
            "invalid stored managed tool status `{other}`"
        ))),
    }
}

pub(super) fn status_string(value: ManagedToolStatus) -> &'static str {
    match value {
        ManagedToolStatus::Healthy => "healthy",
        ManagedToolStatus::Degraded => "degraded",
        ManagedToolStatus::Unavailable => "unavailable",
        ManagedToolStatus::Disabled => "disabled",
    }
}

fn json_string_vec(row: &PgRow, column: &str) -> Result<Vec<String>> {
    let value: Json<Value> = row.try_get(column).map_err(sqlx_err)?;
    serde_json::from_value(value.0)
        .map_err(|error| Error::internal(format!("invalid stored `{column}`: {error}")))
}

pub(super) fn named_values(row: &PgRow, column: &str) -> Result<Vec<NamedValue>> {
    let value: Json<Vec<NamedValue>> = row.try_get(column).map_err(sqlx_err)?;
    Ok(value.0)
}

fn micros(row: &PgRow, column: &str) -> Result<TimestampMicros> {
    Ok(TimestampMicros(row.try_get(column).map_err(sqlx_err)?))
}

fn optional_micros(row: &PgRow, column: &str) -> Result<Option<TimestampMicros>> {
    Ok(row
        .try_get::<Option<i64>, _>(column)
        .map_err(sqlx_err)?
        .map(TimestampMicros))
}

pub(super) fn policy_row(row: PgRow) -> Result<ToolPolicy> {
    let overrides: Json<Value> = row.try_get("environment_overrides").map_err(sqlx_err)?;

    Ok(ToolPolicy {
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        tool_name: row.try_get("tool_name").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        execution_mode: parse_execution_mode(
            row.try_get::<String, _>("execution_mode")
                .map_err(sqlx_err)?
                .as_str(),
        )?,
        environment_overrides: overrides.0,
        timeout_ms: row.try_get("timeout_ms").map_err(sqlx_err)?,
        max_calls_per_run: row.try_get("max_calls_per_run").map_err(sqlx_err)?,
        max_response_bytes: row.try_get("max_response_bytes").map_err(sqlx_err)?,
        updated_by: Id(row.try_get("updated_by").map_err(sqlx_err)?),
        created_at: micros(&row, "created_at_micros")?,
        updated_at: micros(&row, "updated_at_micros")?,
    })
}

pub(super) fn defaults_row(row: PgRow) -> Result<ToolPolicyDefaults> {
    let risk_modes: Json<Value> = row.try_get("risk_modes").map_err(sqlx_err)?;
    let environment_overrides: Json<Value> =
        row.try_get("environment_overrides").map_err(sqlx_err)?;

    Ok(ToolPolicyDefaults {
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        risk_modes: risk_modes.0,
        environment_overrides: environment_overrides.0,
        updated_by: Id(row.try_get("updated_by").map_err(sqlx_err)?),
        created_at: micros(&row, "created_at_micros")?,
        updated_at: micros(&row, "updated_at_micros")?,
    })
}

pub(super) fn server_row(row: PgRow) -> Result<McpServer> {
    let kind: String = row.try_get("scope").map_err(sqlx_err)?;
    let team_id: Option<String> = row.try_get("team_id").map_err(sqlx_err)?;
    let owner_id: Option<String> = row.try_get("owner_id").map_err(sqlx_err)?;

    let member_credential: Option<Json<MemberCredentialSpec>> =
        row.try_get("member_credential").map_err(sqlx_err)?;

    Ok(McpServer {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        scope: Scope::from_parts(&kind, team_id.as_deref(), owner_id.as_deref())?,
        handle: row.try_get("handle").map_err(sqlx_err)?,
        name: row.try_get("name").map_err(sqlx_err)?,
        transport: row.try_get("transport").map_err(sqlx_err)?,
        endpoint_url: row.try_get("endpoint_url").map_err(sqlx_err)?,
        command_template: row.try_get("command_template").map_err(sqlx_err)?,
        auth_type: row.try_get("auth_type").map_err(sqlx_err)?,
        auth_header: row.try_get("auth_header").map_err(sqlx_err)?,
        credential_last4: row.try_get("credential_last4").map_err(sqlx_err)?,
        credential_set: row.try_get("credential_set").map_err(sqlx_err)?,
        member_credential: member_credential.map(|value| value.0),
        headers: named_values(&row, "headers")?,
        env: named_values(&row, "env")?,
        private_only: row.try_get("private_only").map_err(sqlx_err)?,
        allowed_domains: json_string_vec(&row, "allowed_domains")?,
        allowed_cidrs: json_string_vec(&row, "allowed_cidrs")?,
        follow_redirects: row.try_get("follow_redirects").map_err(sqlx_err)?,
        tls_verify: row.try_get("tls_verify").map_err(sqlx_err)?,
        timeout_ms: row.try_get("timeout_ms").map_err(sqlx_err)?,
        max_response_bytes: row.try_get("max_response_bytes").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        status: row.try_get("status").map_err(sqlx_err)?,
        last_error: row.try_get("last_error").map_err(sqlx_err)?,
        last_tested_at: optional_micros(&row, "last_tested_at_micros")?,
        last_synced_at: optional_micros(&row, "last_synced_at_micros")?,
        created_by: Id(row.try_get("created_by").map_err(sqlx_err)?),
        created_at: micros(&row, "created_at_micros")?,
        updated_at: micros(&row, "updated_at_micros")?,
    })
}

pub(super) fn mcp_tool_row(row: PgRow) -> Result<McpTool> {
    let input_schema: Json<Value> = row.try_get("input_schema").map_err(sqlx_err)?;
    let output_schema: Option<Json<Value>> = row.try_get("output_schema").map_err(sqlx_err)?;
    let capabilities: Json<Value> = row.try_get("capabilities").map_err(sqlx_err)?;

    Ok(McpTool {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        server_id: Id(row.try_get("server_id").map_err(sqlx_err)?),
        remote_name: row.try_get("remote_name").map_err(sqlx_err)?,
        name: row.try_get("name").map_err(sqlx_err)?,
        display_name: row.try_get("display_name").map_err(sqlx_err)?,
        description: row.try_get("description").map_err(sqlx_err)?,
        input_schema: input_schema.0,
        schema_hash: row.try_get("schema_hash").map_err(sqlx_err)?,
        schema_dialect: row.try_get("schema_dialect").map_err(sqlx_err)?,
        schema_synced_at: micros(&row, "schema_synced_at_micros")?,
        unavailable_diagnostic: row.try_get("unavailable_diagnostic").map_err(sqlx_err)?,
        output_schema: output_schema.map(|value| value.0),
        minimum_risk: parse_risk(
            row.try_get::<String, _>("minimum_risk")
                .map_err(sqlx_err)?
                .as_str(),
        )?,
        risk: parse_risk(row.try_get::<String, _>("risk").map_err(sqlx_err)?.as_str())?,
        execution_mode: parse_execution_mode(
            row.try_get::<String, _>("execution_mode")
                .map_err(sqlx_err)?
                .as_str(),
        )?,
        capabilities: capabilities.0,
        tags: json_string_vec(&row, "tags")?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        status: parse_status(
            row.try_get::<String, _>("status")
                .map_err(sqlx_err)?
                .as_str(),
        )?,
        version: row.try_get("version").map_err(sqlx_err)?,
        last_synced_at: micros(&row, "last_synced_at_micros")?,
        created_at: micros(&row, "created_at_micros")?,
        updated_at: micros(&row, "updated_at_micros")?,
    })
}

pub(super) fn prompt_row(row: PgRow) -> Result<McpPrompt> {
    let arguments: Json<Vec<McpPromptArgument>> = row.try_get("arguments").map_err(sqlx_err)?;

    Ok(McpPrompt {
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        server_id: Id(row.try_get("server_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        title: row.try_get("title").map_err(sqlx_err)?,
        description: row.try_get("description").map_err(sqlx_err)?,
        arguments: arguments.0,
        synced_at: micros(&row, "synced_at_micros")?,
    })
}

pub(super) fn member_credential_row(row: PgRow) -> Result<MemberCredential> {
    Ok(MemberCredential {
        server_id: Id(row.try_get("server_id").map_err(sqlx_err)?),
        user_id: Id(row.try_get("user_id").map_err(sqlx_err)?),
        last4: row.try_get("last4").map_err(sqlx_err)?,
        updated_at: micros(&row, "updated_at_micros")?,
    })
}
