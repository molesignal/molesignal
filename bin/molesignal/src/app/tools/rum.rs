// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::{ToolRuntime, common::*};
use crate::{
    app::{
        iam::IamContext,
        rum::reader::{Reader, Span},
    },
    domain::{rum::event::RumStream, stream::StreamType},
    infra::rum::query::{
        Condition, ListRequest, Scope, default_columns, list_sql, people_only, rows,
        timeline_columns,
    },
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

mod aggregate;
mod analytics;
mod flags;
mod frustration;
mod heatmap;
mod issues;
mod notes;
mod overview;
mod related_traces;
mod vitals;
mod waterfall;

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 500;
const RUM_LOOKBACK_MICROS: i64 = 12 * 60 * 60 * 1_000_000;
/// The range the tools that summarize an application read unless they are told another.
const SUMMARY_LOOKBACK_MICROS: i64 = 24 * 60 * 60 * 1_000_000;

/// The streams `list_rum_actions` reads, besides the user actions it is named for.
const EVENT_STREAMS: [RumStream; 6] = [
    RumStream::Actions,
    RumStream::Views,
    RumStream::Resources,
    RumStream::LongTasks,
    RumStream::Vitals,
    RumStream::Logs,
];

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::ListRumSessions => {
            list(runtime, auth, RumStream::Sessions, "sessions", arguments).await
        }
        BuiltinToolKind::ListRumActions => {
            list(runtime, auth, RumStream::Actions, "events", arguments).await
        }
        BuiltinToolKind::ListRumErrors => {
            list(runtime, auth, RumStream::Errors, "errors", arguments).await
        }
        BuiltinToolKind::GetRumSession => get_session(runtime, auth, arguments).await,
        BuiltinToolKind::GetRumRelatedTraces => related_traces::get(runtime, auth, arguments).await,
        BuiltinToolKind::GetRumVitals => vitals::summarize(runtime, auth, arguments).await,
        BuiltinToolKind::ListRumIssues => issues::list(runtime, auth, arguments).await,
        BuiltinToolKind::GetRumIssue => issues::get(runtime, auth, arguments).await,
        BuiltinToolKind::GetRumOverview => overview::summarize(runtime, auth, arguments).await,
        BuiltinToolKind::GetRumFrustration => {
            frustration::summarize(runtime, auth, arguments).await
        }
        BuiltinToolKind::GetRumViewWaterfall => waterfall::get(runtime, auth, arguments).await,
        BuiltinToolKind::AggregateRum => aggregate::run(runtime, auth, arguments).await,
        BuiltinToolKind::GetRumFunnel => analytics::funnel(runtime, auth, arguments).await,
        BuiltinToolKind::GetRumPaths => analytics::paths(runtime, auth, arguments).await,
        BuiltinToolKind::GetRumRetention => analytics::retention(runtime, auth, arguments).await,
        BuiltinToolKind::GetRumFeatureFlags => flags::summarize(runtime, auth, arguments).await,
        BuiltinToolKind::GetRumHeatmap => heatmap::get(runtime, auth, arguments).await,
        BuiltinToolKind::GetRumSessionNotes => notes::get(runtime, auth, arguments).await,
        _ => unreachable!("RUM handler received unrelated tool"),
    }
}

/// What to read of one RUM stream.
struct Read<'a> {
    stream: RumStream,
    range: TimeRange,
    conditions: &'a [Condition],
    columns: &'a [&'a str],
    limit: usize,
    oldest_first: bool,
}

/// Rows of one RUM stream, as the tools hand them to the model.
struct Listed {
    items: Vec<Value>,
    scanned_rows: u64,
    took_ms: u64,
}

/// The rows of a stream that meet the conditions, newest first or oldest first; `None` when
/// nothing was ever written to the stream. A view or a session is read at its latest revision.
async fn list_rows(
    runtime: &ToolRuntime,
    auth: &IamContext,
    read: &Read<'_>,
) -> Result<Option<Listed>> {
    let stream = read.stream;

    let statement = list_sql(&ListRequest {
        scope: Scope {
            stream,
            from: read.range.start.0,
            to: read.range.end.0,
            conditions: read.conditions,
        },
        columns: read.columns,
        limit: read.limit,
        seek: None,
        oldest_first: read.oldest_first,
    });

    let Some(result) = run_optional_stream_query(
        runtime,
        &auth.org_id,
        statement,
        read.range,
        stream.name(),
        StreamType::LOGS,
        read.limit,
    )
    .await?
    else {
        return Ok(None);
    };

    let (scanned_rows, took_ms) = (result.scanned_rows, result.took_ms);

    Ok(Some(Listed {
        items: rows(stream, result)
            .into_iter()
            .map(|row| Value::Object(row.into_item()))
            .collect(),
        scanned_rows,
        took_ms,
    }))
}

pub(super) fn equal_to(column: &str, value: Option<&str>) -> Option<Condition> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| Condition::equals(column, value))
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListRumArgs {
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    application: Option<String>,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    end_user_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    action_type: Option<String>,
    #[serde(default)]
    fingerprint: Option<String>,
    /// `user`, `synthetics` or `ci_test`: which kind of session to list. Sessions people ran unless
    /// it says, or a session is named.
    #[serde(default)]
    session_type: Option<String>,
    /// For the tool that lists user actions: which other rows to list instead.
    #[serde(default)]
    stream: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

impl ListRumArgs {
    /// The stream the tool reads: the one it is named for, or for the tool of user actions the
    /// one asked for.
    fn stream(&self, named: RumStream) -> Result<RumStream> {
        let Some(asked) = self
            .stream
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return Ok(named);
        };

        if named != RumStream::Actions {
            return Err(Error::invalid(
                "stream can only be given when listing actions",
            ));
        }

        RumStream::from_collection(asked)
            .filter(|stream| EVENT_STREAMS.contains(stream))
            .ok_or_else(|| {
                let known = EVENT_STREAMS.map(RumStream::collection).join(", ");

                Error::invalid(format!("stream must be one of {known}"))
            })
    }

    fn conditions(&self, stream: RumStream) -> Result<Vec<Condition>> {
        if self.fingerprint.is_some() && stream != RumStream::Errors {
            return Err(Error::invalid(
                "fingerprint can only be given when listing errors",
            ));
        }

        if self.action_type.is_some() && stream != RumStream::Actions {
            return Err(Error::invalid(
                "actionType can only be given when listing actions",
            ));
        }

        // A named session is read whatever ran it; a list is of what people did unless it says.
        let kind = match (self.session_type.as_deref(), self.session_id.as_deref()) {
            (Some(kind), _) => equal_to("session.type", Some(kind)),
            (None, Some(_)) => None,
            (None, None) => Some(people_only()),
        };

        Ok([
            equal_to("application.id", self.application.as_deref()),
            equal_to("env", self.environment.as_deref()),
            equal_to("session.id", self.session_id.as_deref()),
            equal_to("usr.id", self.end_user_id.as_deref()),
            equal_to("error.fingerprint", self.fingerprint.as_deref()),
            equal_to("action.type", self.action_type.as_deref()),
            kind,
        ]
        .into_iter()
        .flatten()
        .collect())
    }
}

async fn list(
    runtime: &ToolRuntime,
    auth: &IamContext,
    named: RumStream,
    result_key: &str,
    arguments: Value,
) -> Result<ToolResult> {
    let args: ListRumArgs = parse_args(arguments)?;
    let stream = args.stream(named)?;
    let conditions = args.conditions(stream)?;
    let range = optional_time_range(args.time_range)?;
    let limit = args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    let Some(listed) = list_rows(
        runtime,
        auth,
        &Read {
            stream,
            range,
            conditions: &conditions,
            columns: default_columns(stream),
            limit,
            oldest_first: false,
        },
    )
    .await?
    else {
        return Ok(ToolResult::json(json!({
            (result_key): [], "streamAvailable": false,
            "message": format!("{} has not received data yet", stream.name()),
        })));
    };

    Ok(ToolResult::json(json!({
        (result_key): listed.items, "type": stream.collection(), "streamAvailable": true,
        "scannedRows": listed.scanned_rows, "tookMs": listed.took_ms,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionArgs {
    session_id: String,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
    #[serde(default)]
    event_limit: Option<usize>,
}

async fn get_session(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: SessionArgs = parse_args(arguments)?;
    let range = rum_range(args.time_range)?;
    let limit = args.event_limit.unwrap_or(100).clamp(1, 500);
    let of_session = [Condition::equals("session.id", args.session_id.trim())];

    let session = session_row(runtime, auth, range, &of_session).await?;

    let Some(session) = session else {
        return Ok(ToolResult::error(format!(
            "RUM session {} not found",
            args.session_id
        )));
    };

    let mut events = Vec::new();

    for stream in [
        RumStream::Views,
        RumStream::Actions,
        RumStream::Errors,
        RumStream::Logs,
    ] {
        let found = list_rows(
            runtime,
            auth,
            &Read {
                stream,
                range,
                conditions: &of_session,
                columns: &timeline_columns(stream),
                limit,
                oldest_first: true,
            },
        )
        .await?
        .map(|listed| listed.items)
        .unwrap_or_default();

        events.push((stream, found));
    }

    let truncated = events.iter().any(|(_, found)| found.len() == limit);

    let mut answer = json!({ "session": session, "eventsTruncated": truncated });

    for (stream, found) in events {
        answer[stream.collection()] = Value::Array(found);
    }

    Ok(ToolResult::json(answer))
}

async fn session_row(
    runtime: &ToolRuntime,
    auth: &IamContext,
    range: TimeRange,
    of_session: &[Condition],
) -> Result<Option<Value>> {
    Ok(list_rows(
        runtime,
        auth,
        &Read {
            stream: RumStream::Sessions,
            range,
            conditions: of_session,
            columns: default_columns(RumStream::Sessions),
            limit: 1,
            oldest_first: false,
        },
    )
    .await?
    .and_then(|listed| listed.items.into_iter().next()))
}

/// A reader of the RUM streams for whoever the tool runs for.
fn reader<'a>(runtime: &'a ToolRuntime, auth: &'a IamContext) -> Reader<'a> {
    Reader::new(
        &runtime.observability.query,
        runtime.observability.streams.as_ref(),
        auth,
    )
}

/// The range the tools that summarize an application read: the one asked for, or the last day.
fn summary_span(value: Option<TimeRangeArg>) -> Result<Span> {
    let range = match value {
        Some(value) => time_range(value)?,
        None => {
            let end = TimestampMicros::now();

            TimeRange::new(
                TimestampMicros(end.0.saturating_sub(SUMMARY_LOOKBACK_MICROS)),
                end,
            )
        }
    };

    Ok(Span {
        from: range.start.0,
        to: range.end.0,
    })
}

fn rum_range(value: Option<TimeRangeArg>) -> Result<TimeRange> {
    value.map(time_range).transpose().map(|range| {
        range.unwrap_or_else(|| {
            let end = TimestampMicros::now();

            TimeRange::new(
                TimestampMicros(end.0.saturating_sub(RUM_LOOKBACK_MICROS)),
                end,
            )
        })
    })
}

fn value_i64(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(value: Value) -> ListRumArgs {
        serde_json::from_value(value).expect("arguments")
    }

    #[test]
    fn the_tool_of_user_actions_can_list_the_other_rows_of_a_session_too() {
        let none = args(json!({}));

        assert_eq!(none.stream(RumStream::Actions).unwrap(), RumStream::Actions);
        assert_eq!(none.stream(RumStream::Errors).unwrap(), RumStream::Errors);

        let views = args(json!({ "stream": "views" }));

        assert_eq!(views.stream(RumStream::Actions).unwrap(), RumStream::Views);
        assert!(views.stream(RumStream::Errors).is_err());
        assert!(
            args(json!({ "stream": "errors" }))
                .stream(RumStream::Actions)
                .is_err()
        );
        assert!(
            args(json!({ "stream": "sessions" }))
                .stream(RumStream::Actions)
                .is_err()
        );
    }

    #[test]
    fn arguments_become_conditions_on_the_columns_of_the_stream() {
        let found = args(json!({
            "application": "shop-web", "environment": " prod ", "endUserId": "u-1",
            "sessionId": "", "fingerprint": "abc",
        }))
        .conditions(RumStream::Errors)
        .unwrap();

        let sql = found
            .iter()
            .map(|condition| condition.sql.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            sql,
            vec![
                "\"application.id\" = 'shop-web'",
                "\"env\" = 'prod'",
                "\"usr.id\" = 'u-1'",
                "\"error.fingerprint\" = 'abc'",
            ]
        );
    }

    #[test]
    fn an_argument_of_another_stream_is_refused() {
        assert!(
            args(json!({ "fingerprint": "abc" }))
                .conditions(RumStream::Actions)
                .is_err()
        );
        assert!(
            args(json!({ "actionType": "click" }))
                .conditions(RumStream::Errors)
                .is_err()
        );
        assert!(
            args(json!({ "actionType": "click" }))
                .conditions(RumStream::Actions)
                .is_ok()
        );
    }

    #[test]
    fn unknown_arguments_are_refused() {
        assert!(serde_json::from_value::<ListRumArgs>(json!({ "page": "x" })).is_err());
    }

    #[test]
    fn a_number_may_come_as_text() {
        assert_eq!(value_i64(&json!(7)), Some(7));
        assert_eq!(value_i64(&json!("8")), Some(8));
        assert_eq!(value_i64(&json!("x")), None);
    }
}
