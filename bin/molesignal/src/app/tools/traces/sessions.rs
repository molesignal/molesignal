// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use crate::{
    app::{
        iam::IamContext,
        tools::{
            ToolRuntime,
            common::{
                TimeRangeArg, optional_time_range, parse_args, quote_ident, require_field,
                resolve_traces_stream, rows_as_objects, run_query, schema_has, sql_literal,
            },
        },
    },
    domain::{query::QueryLanguage, stream::StreamType},
    shared::{Error, Result},
};

const SESSION_FIELDS: [&str; 4] = [
    "session_id",
    "gen_ai.conversation.id",
    "conversation_id",
    "gen_ai.session.id",
];
const USER_FIELDS: [&str; 4] = ["user_id", "enduser.id", "gen_ai.user.id", "user.id"];

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::ListTraceSessions => list_sessions(runtime, auth, arguments).await,
        BuiltinToolKind::GetTraceSession => get_session(runtime, auth, arguments).await,
        BuiltinToolKind::ListTraceUsers => list_users(runtime, auth, arguments).await,
        _ => unreachable!("trace session executor received unrelated tool"),
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AggregateArgs {
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn list_sessions(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: AggregateArgs = parse_args(arguments)?;
    aggregate_identities(runtime, auth, args, &SESSION_FIELDS, "session_id").await
}

async fn list_users(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: AggregateArgs = parse_args(arguments)?;
    aggregate_identities(runtime, auth, args, &USER_FIELDS, "user_id").await
}

async fn aggregate_identities(
    runtime: &ToolRuntime,
    auth: &IamContext,
    args: AggregateArgs,
    candidates: &[&str],
    alias: &str,
) -> Result<ToolResult> {
    let Some(stream) = resolve_traces_stream(runtime, &auth.org_id).await? else {
        return Ok(unsupported(alias, "trace stream is unavailable"));
    };
    let Some(identity) = first_field(&stream.schema, candidates) else {
        return Ok(unsupported(alias, "identity field is unavailable"));
    };
    if !schema_has(&stream.schema, "_timestamp") {
        return Ok(unsupported(alias, "trace timestamp field is unavailable"));
    }
    let limit = args.limit.unwrap_or(50).clamp(1, 200);
    let mut filters = vec![format!(
        "{} IS NOT NULL AND {} != ''",
        quote_ident(identity),
        quote_ident(identity)
    )];
    if let Some(service) = args
        .service
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && schema_has(&stream.schema, "service_name")
    {
        filters.push(format!(
            "{} = '{}'",
            quote_ident("service_name"),
            sql_literal(service)
        ));
    }
    let identity_field = identity;
    let identity = quote_ident(identity_field);
    let statement = format!(
        "SELECT {identity} AS {alias}, COUNT(DISTINCT trace_id) AS trace_count, COUNT(*) AS span_count, MIN(_timestamp) AS first_seen_micros, MAX(_timestamp) AS last_seen_micros FROM {stream} WHERE {filters} GROUP BY {identity} ORDER BY last_seen_micros DESC LIMIT {limit}",
        alias = quote_ident(alias),
        stream = quote_ident(&stream.name),
        filters = filters.join(" AND "),
    );
    let result = run_query(
        runtime,
        &auth.org_id,
        QueryLanguage::Sql,
        statement,
        optional_time_range(args.time_range)?,
        Some((&stream.name, StreamType::Traces)),
        limit,
    )
    .await?;
    Ok(ToolResult::json(json!({
        "supported": true,
        "identity_field": identity_field,
        "items": rows_as_objects(&result),
        "scanned_rows": result.scanned_rows,
        "took_ms": result.took_ms,
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionArgs {
    session_id: String,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn get_session(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: SessionArgs = parse_args(arguments)?;
    let session_id = args.session_id.trim();
    if session_id.is_empty() || session_id.len() > 512 {
        return Err(Error::invalid("session_id must contain 1 to 512 bytes"));
    }
    let Some(stream) = resolve_traces_stream(runtime, &auth.org_id).await? else {
        return Ok(unsupported("session_id", "trace stream is unavailable"));
    };
    let Some(field) = first_field(&stream.schema, &SESSION_FIELDS) else {
        return Ok(unsupported("session_id", "session field is unavailable"));
    };
    if !schema_has(&stream.schema, "_timestamp") {
        return Ok(unsupported(
            "session_id",
            "trace timestamp field is unavailable",
        ));
    }
    require_field(&stream.schema, field)?;
    let limit = args.limit.unwrap_or(100).clamp(1, 500);
    let statement = format!(
        "SELECT trace_id, COUNT(*) AS span_count, MIN(_timestamp) AS started_at_micros, MAX(_timestamp) AS ended_at_micros FROM {} WHERE {} = '{}' GROUP BY trace_id ORDER BY started_at_micros DESC LIMIT {limit}",
        quote_ident(&stream.name),
        quote_ident(field),
        sql_literal(session_id),
    );
    let result = run_query(
        runtime,
        &auth.org_id,
        QueryLanguage::Sql,
        statement,
        optional_time_range(args.time_range)?,
        Some((&stream.name, StreamType::Traces)),
        limit,
    )
    .await?;
    Ok(ToolResult::json(json!({
        "supported": true,
        "session_id": session_id,
        "identity_field": field,
        "traces": rows_as_objects(&result),
        "scanned_rows": result.scanned_rows,
        "took_ms": result.took_ms,
    })))
}

fn first_field<'a>(
    schema: &crate::domain::stream::Schema,
    candidates: &'a [&str],
) -> Option<&'a str> {
    candidates
        .iter()
        .copied()
        .find(|field| schema_has(schema, field))
}

fn unsupported(identity: &str, reason: &str) -> ToolResult {
    ToolResult::json(json!({
        "supported": false,
        "identity": identity,
        "items": [],
        "reason": reason,
    }))
}
