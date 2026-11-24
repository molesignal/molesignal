// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The looks a correlation report is made of: how many lines each service
//! wrote to each log stream and how many carried a trace id, whether a sample
//! of those trace ids is among the kept traces, and which service names the
//! metrics carry. Names are read as APM reads them, naming rules and aliases
//! applied.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    time::Duration,
};

use domain::{
    query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
    stream::{MOLESIGNAL_SYSTEM_STREAM, StreamDefinition, StreamType},
};
use kernel::{
    Error, Result,
    ids::Id,
    time::{TimeRange, TimestampMicros},
};
use serde_json::Value;

use super::OrgNames;
use crate::app::query::{
    QueryService,
    statement::{count, first_field, ident, literal},
};

/// The field a log stream names its service in, the first the stream has.
pub const LOG_SERVICE_FIELDS: [&str; 3] = ["service.name", "service_name", "service"];
/// The field a log stream keeps the level in, the first the stream has.
pub const LOG_LEVEL_FIELDS: [&str; 5] = [
    "severity_text",
    "level",
    "severity",
    "log.level",
    "loglevel",
];
/// Levels read as errors, lowercased.
pub const LOG_ERROR_LEVELS: &str =
    "'error', 'err', 'fatal', 'critical', 'crit', 'alert', 'emergency'";
/// The field a log stream keeps the trace id in, the first the stream has.
const LOG_TRACE_FIELDS: [&str; 3] = ["trace_id", "traceId", "trace.id"];
/// The label metrics name their service in.
const METRIC_SERVICE_LABEL: &str = "service.name";

const LOOK_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_STREAMS: usize = 8;
const MAX_TRACE_STREAMS: usize = 3;
/// The most services a log stream's count answers, the busiest first.
const MAX_COUNTED_NAMES: usize = 500;
const MAX_METRIC_NAMES: usize = 1_000;
/// Recent traced lines read from each stream to take trace ids from.
const SAMPLED_LINES: usize = 300;
const TRACES_PER_NAME: usize = 10;
const MAX_SAMPLED_TRACES: usize = 200;
/// A trace may be stamped a little before or after the lines that name it.
const TRACE_GRACE_MICROS: i64 = 5 * 60 * 1_000_000;

/// What the logs said under one name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct NameLines {
    pub streams: BTreeSet<String>,
    pub lines: u64,
    pub traced: u64,
    pub sampled: u64,
    pub matched: u64,
}

#[derive(Debug, Clone, Default)]
pub(super) struct LogLook {
    pub by_name: HashMap<String, NameLines>,
    /// A stream could not be counted: a name found nowhere may be in it.
    pub failed: bool,
}

#[derive(Debug, Clone, Default)]
pub(super) struct MetricLook {
    /// The metrics streams carrying each name.
    pub by_name: HashMap<String, BTreeSet<String>>,
    pub failed: bool,
}

pub(super) struct Looker<'a> {
    pub query: &'a QueryService,
    pub org_id: &'a Id,
    pub window: TimeRange,
    pub names: &'a OrgNames,
}

struct LogStream<'s> {
    name: &'s str,
    service: &'static str,
    trace: Option<&'static str>,
}

impl Looker<'_> {
    pub async fn logs(&self, streams: &[StreamDefinition], system_streams: bool) -> LogLook {
        let mut chosen = streams
            .iter()
            .filter(|stream| stream.stream_type == StreamType::LOGS)
            .filter(|stream| system_streams || stream.name != MOLESIGNAL_SYSTEM_STREAM)
            .filter_map(|stream| {
                Some(LogStream {
                    name: &stream.name,
                    service: first_field(stream, &LOG_SERVICE_FIELDS)?,
                    trace: first_field(stream, &LOG_TRACE_FIELDS),
                })
            })
            .collect::<Vec<_>>();

        chosen.sort_by_key(|stream| (stream.name != "default", stream.name));
        chosen.truncate(MAX_STREAMS);

        let looks = futures::future::join_all(chosen.iter().map(|stream| self.log_stream(stream)));
        let mut look = LogLook::default();
        let mut samples = HashMap::<String, Vec<String>>::new();

        for result in looks.await {
            let Ok((stream, counts, sample)) = result else {
                look.failed = true;
                continue;
            };

            for (name, lines, traced) in counts {
                let entry = look.by_name.entry(name).or_default();
                entry.streams.insert(stream.to_owned());
                entry.lines += lines;
                entry.traced += traced;
            }

            for (name, trace) in sample {
                let kept = samples.entry(name).or_default();

                if kept.len() < TRACES_PER_NAME && !kept.contains(&trace) {
                    kept.push(trace);
                }
            }
        }

        self.match_traces(streams, samples, &mut look).await;
        look
    }

    /// The counts by name of one log stream, and recent trace ids by name.
    async fn log_stream<'s>(
        &self,
        stream: &LogStream<'s>,
    ) -> Result<(&'s str, Vec<(String, u64, u64)>, Vec<(String, String)>)> {
        let service = ident(stream.service);
        let from = ident(stream.name);

        let traced = stream.trace.map_or_else(
            || "0".to_owned(),
            |trace| {
                let trace = ident(trace);
                format!(
                    "sum(CASE WHEN \"{trace}\" IS NOT NULL AND CAST(\"{trace}\" AS VARCHAR) <> '' \
                     THEN 1 ELSE 0 END)"
                )
            },
        );

        let statement = format!(
            "SELECT \"{service}\" AS svc, count(*) AS lines, {traced} AS traced FROM \"{from}\" \
             GROUP BY \"{service}\" ORDER BY lines DESC LIMIT {MAX_COUNTED_NAMES}"
        );

        let counted = self
            .sql(statement, MAX_COUNTED_NAMES, self.window, stream.name)
            .await?;

        let counts = counted
            .rows
            .iter()
            .filter_map(|row| {
                let name = self.name_of(row.first()?)?;
                Some((name, count(row.get(1)?), row.get(2).map_or(0, count)))
            })
            .collect();

        let Some(trace) = stream.trace else {
            return Ok((stream.name, counts, Vec::new()));
        };

        let trace = ident(trace);

        let statement = format!(
            "SELECT \"{service}\", CAST(\"{trace}\" AS VARCHAR) FROM \"{from}\" \
             WHERE \"{trace}\" IS NOT NULL AND CAST(\"{trace}\" AS VARCHAR) <> '' \
             ORDER BY _timestamp DESC LIMIT {SAMPLED_LINES}"
        );

        // Counts without a sample still say how many lines carry trace ids.
        let sample = match self
            .sql(statement, SAMPLED_LINES, self.window, stream.name)
            .await
        {
            Ok(sampled) => sampled
                .rows
                .iter()
                .filter_map(|row| {
                    let name = self.name_of(row.first()?)?;
                    let trace = row.get(1)?.as_str().map(str::trim)?;
                    plausible_trace_id(trace).then(|| (name, trace.to_owned()))
                })
                .collect(),
            Err(error) => {
                tracing::debug!(%error, stream = stream.name, "trace id sample failed");
                Vec::new()
            }
        };

        Ok((stream.name, counts, sample))
    }

    /// Looks for the sampled trace ids among the kept traces. When the traces cannot be read,
    /// nothing counts as sampled: there is nothing to tell.
    async fn match_traces(
        &self,
        streams: &[StreamDefinition],
        samples: HashMap<String, Vec<String>>,
        look: &mut LogLook,
    ) {
        let mut wanted = BTreeSet::new();

        for traces in samples.values() {
            for trace in traces {
                if wanted.len() < MAX_SAMPLED_TRACES {
                    wanted.insert(trace.clone());
                }
            }
        }

        let trace_streams = streams
            .iter()
            .filter(|stream| stream.stream_type == StreamType::TRACES)
            .filter(|stream| first_field(stream, &["trace_id"]).is_some())
            .take(MAX_TRACE_STREAMS)
            .collect::<Vec<_>>();

        if wanted.is_empty() || trace_streams.is_empty() {
            return;
        }

        let listed = wanted
            .iter()
            .map(|trace| format!("'{}'", literal(trace)))
            .collect::<Vec<_>>()
            .join(", ");

        let around = TimeRange::new(
            TimestampMicros(self.window.start.0 - TRACE_GRACE_MICROS),
            TimestampMicros(self.window.end.0 + TRACE_GRACE_MICROS),
        );

        let looks = trace_streams.iter().map(|stream| {
            let statement = format!(
                "SELECT DISTINCT trace_id FROM \"{}\" WHERE trace_id IN ({listed}) LIMIT {}",
                ident(&stream.name),
                wanted.len(),
            );

            self.query_stream(
                statement,
                wanted.len(),
                around,
                &stream.name,
                StreamType::TRACES,
            )
        });

        let mut found = HashSet::new();
        let mut read = false;

        for result in futures::future::join_all(looks).await {
            match result {
                Ok(kept) => {
                    read = true;
                    found.extend(
                        kept.rows.iter().filter_map(|row| {
                            row.first().and_then(Value::as_str).map(str::to_owned)
                        }),
                    );
                }
                Err(error) => tracing::debug!(%error, "trace match look failed"),
            }
        }

        if !read {
            return;
        }

        for (name, traces) in samples {
            let traces = traces
                .into_iter()
                .filter(|trace| wanted.contains(trace))
                .collect::<Vec<_>>();

            let entry = look.by_name.entry(name).or_default();
            entry.sampled = traces.len() as u64;
            entry.matched = traces.iter().filter(|trace| found.contains(*trace)).count() as u64;
        }
    }

    pub async fn metrics(&self, streams: &[StreamDefinition]) -> MetricLook {
        let chosen = streams
            .iter()
            .filter(|stream| stream.stream_type == StreamType::METRICS)
            .filter(|stream| first_field(stream, &[METRIC_SERVICE_LABEL]).is_some())
            .take(MAX_STREAMS)
            .collect::<Vec<_>>();

        let looks = chosen.iter().map(|stream| async move {
            let label = ident(METRIC_SERVICE_LABEL);

            let statement = format!(
                "SELECT DISTINCT \"{label}\" FROM \"{}\" LIMIT {MAX_METRIC_NAMES}",
                ident(&stream.name)
            );

            let names = self
                .query_stream(
                    statement,
                    MAX_METRIC_NAMES,
                    self.window,
                    &stream.name,
                    StreamType::METRICS,
                )
                .await?;

            Ok::<_, Error>((stream.name.as_str(), names))
        });

        let mut look = MetricLook::default();

        for result in futures::future::join_all(looks).await {
            let Ok((stream, names)) = result else {
                look.failed = true;
                continue;
            };

            for name in names
                .rows
                .iter()
                .filter_map(|row| self.name_of(row.first()?))
            {
                look.by_name
                    .entry(name)
                    .or_default()
                    .insert(stream.to_owned());
            }
        }

        look
    }

    /// The name APM reads a service by, from a value logs or metrics carry.
    fn name_of(&self, value: &Value) -> Option<String> {
        let name = value
            .as_str()
            .map(str::trim)
            .filter(|name| !name.is_empty())?;
        Some(self.names.service(name))
    }

    async fn sql(
        &self,
        statement: String,
        limit: usize,
        range: TimeRange,
        stream: &str,
    ) -> Result<QueryResult> {
        self.query_stream(statement, limit, range, stream, StreamType::LOGS)
            .await
    }

    async fn query_stream(
        &self,
        statement: String,
        limit: usize,
        range: TimeRange,
        stream: &str,
        stream_type: StreamType,
    ) -> Result<QueryResult> {
        let request = QueryRequest {
            org_id: self.org_id.clone(),
            language: QueryLanguage::Sql,
            statement,
            time_range: range,
            stream: Some(StreamHint {
                name: stream.to_owned(),
                stream_type,
            }),
            limit: Some(limit),
            step: None,
            federation_clusters: Vec::new(),
        };

        tokio::time::timeout(LOOK_TIMEOUT, self.query.run(request))
            .await
            .map_err(|_| Error::internal("correlation look ran out of time"))?
    }
}

/// Trace ids are hex, with dashes in some loggers; anything else is not looked for.
pub(super) fn plausible_trace_id(value: &str) -> bool {
    (16..=64).contains(&value.len())
        && value
            .chars()
            .all(|char| char.is_ascii_hexdigit() || char == '-')
}
