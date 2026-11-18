// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What one stream holds: how many of its rows carry a service, an environment and a trace id,
//! how many messages are JSON left as text, and which services and environments it names.
//!
//! Two queries a stream at most, each cut short when it takes too long or when the time of the
//! whole check runs out.

use std::time::{Duration, Instant};

use domain::{
    query::{QueryLanguage, QueryResult},
    stream::StreamDefinition,
};
use kernel::{ids::Id, time::TimeRange};
use serde_json::Value;

use super::{
    super::super::{
        ToolRuntime,
        common::{quote_ident, run_query},
    },
    fields::{Detected, detect},
    names::Signal,
};

/// One query waits no longer than this.
const QUERY_TIMEOUT: Duration = Duration::from_secs(8);

/// The most combinations of service and environment read of one stream.
const MAX_VALUES: usize = 200;

/// What the rows of a stream carry, counted over the range.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Coverage {
    pub(super) total: u64,
    pub(super) service: u64,
    pub(super) environment: u64,
    pub(super) trace_id: u64,
    /// Messages that begin as a JSON object: data left as text.
    pub(super) json_messages: u64,
    /// One value of the text time field.
    pub(super) time_sample: Option<String>,
}

/// How many rows name one service in one environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ValueCount {
    pub(super) service: Option<String>,
    pub(super) environment: Option<String>,
    pub(super) count: u64,
}

/// What was found of one stream.
#[derive(Debug, Clone)]
pub(super) struct Probe {
    pub(super) name: String,
    pub(super) signal: Signal,
    pub(super) detected: Detected,
    pub(super) coverage: Option<Coverage>,
    pub(super) values: Vec<ValueCount>,
    /// Why the stream could not be read in full.
    pub(super) error: Option<String>,
}

pub(super) fn coverage_sql(stream: &str, detected: &Detected) -> String {
    let mut columns = vec!["COUNT(*) AS total".to_string()];

    let counted = [
        (&detected.service, "service"),
        (&detected.environment, "environment"),
        (&detected.trace_id, "trace_id"),
    ];

    for (field, alias) in counted {
        if let Some(field) = field {
            columns.push(format!("COUNT({}) AS {alias}", quote_ident(field)));
        }
    }

    if let Some(message) = detected
        .message
        .as_ref()
        .filter(|_| detected.message_is_text)
    {
        columns.push(format!(
            "SUM(CASE WHEN {} LIKE '{{%' THEN 1 ELSE 0 END) AS json_messages",
            quote_ident(message)
        ));
    }

    if let Some(time) = &detected.text_time {
        columns.push(format!("MAX({}) AS time_sample", quote_ident(time)));
    }

    format!("SELECT {} FROM {}", columns.join(", "), quote_ident(stream))
}

/// The services and environments the stream names, the most rows first; `None` when it records
/// neither.
pub(super) fn values_sql(stream: &str, detected: &Detected) -> Option<String> {
    let fields = [
        (&detected.service, "service"),
        (&detected.environment, "environment"),
    ]
    .into_iter()
    .filter_map(|(field, alias)| field.as_ref().map(|field| (quote_ident(field), alias)))
    .collect::<Vec<_>>();

    if fields.is_empty() {
        return None;
    }

    let selected = fields
        .iter()
        .map(|(field, alias)| format!("{field} AS {alias}"))
        .collect::<Vec<_>>()
        .join(", ");

    let grouped = fields
        .iter()
        .map(|(field, _)| field.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    Some(format!(
        "SELECT {selected}, COUNT(*) AS count FROM {} GROUP BY {grouped} ORDER BY count DESC LIMIT {MAX_VALUES}",
        quote_ident(stream)
    ))
}

fn column<'a>(result: &QueryResult, row: &'a [Value], name: &str) -> Option<&'a Value> {
    result
        .columns
        .iter()
        .position(|column| column == name)
        .and_then(|index| row.get(index))
}

fn text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
    .filter(|text| !text.trim().is_empty())
}

fn count(value: Option<&Value>) -> u64 {
    value
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_f64().map(|number| number as u64))
        })
        .unwrap_or_default()
}

pub(super) fn read_coverage(result: &QueryResult) -> Coverage {
    let Some(row) = result.rows.first() else {
        return Coverage::default();
    };

    Coverage {
        total: count(column(result, row, "total")),
        service: count(column(result, row, "service")),
        environment: count(column(result, row, "environment")),
        trace_id: count(column(result, row, "trace_id")),
        json_messages: count(column(result, row, "json_messages")),
        time_sample: text(column(result, row, "time_sample")),
    }
}

pub(super) fn read_values(result: &QueryResult) -> Vec<ValueCount> {
    result
        .rows
        .iter()
        .map(|row| ValueCount {
            service: text(column(result, row, "service")),
            environment: text(column(result, row, "environment")),
            count: count(column(result, row, "count")),
        })
        .collect()
}

/// Reads what `stream` holds over `range`, stopping at `deadline`.
pub(super) async fn probe(
    runtime: &ToolRuntime,
    org_id: &Id,
    stream: &StreamDefinition,
    signal: Signal,
    range: TimeRange,
    deadline: Instant,
) -> Probe {
    let detected = detect(&stream.schema, signal == Signal::Metrics);

    let mut probe = Probe {
        name: stream.name.clone(),
        signal,
        coverage: None,
        values: Vec::new(),
        error: None,
        detected,
    };

    let statement = coverage_sql(&stream.name, &probe.detected);

    match ask(runtime, org_id, stream, statement, range, deadline).await {
        Ok(result) => probe.coverage = Some(read_coverage(&result)),
        Err(reason) => {
            probe.error = Some(reason);
            return probe;
        }
    }

    if let Some(statement) = values_sql(&stream.name, &probe.detected) {
        match ask(runtime, org_id, stream, statement, range, deadline).await {
            Ok(result) => probe.values = read_values(&result),
            Err(reason) => probe.error = Some(reason),
        }
    }

    probe
}

async fn ask(
    runtime: &ToolRuntime,
    org_id: &Id,
    stream: &StreamDefinition,
    statement: String,
    range: TimeRange,
    deadline: Instant,
) -> Result<QueryResult, String> {
    let left = deadline.saturating_duration_since(Instant::now());

    if left.is_zero() {
        return Err("not checked: the time for the check ran out".into());
    }

    let query = run_query(
        runtime,
        org_id,
        QueryLanguage::Sql,
        statement,
        range,
        Some((&stream.name, stream.stream_type)),
        MAX_VALUES,
    );

    match tokio::time::timeout(left.min(QUERY_TIMEOUT), query).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(error)) => Err(error.to_string()),
        Err(_) => Err("the query took too long".into()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn detected() -> Detected {
        Detected {
            service: Some("service.name".into()),
            environment: Some("env".into()),
            trace_id: Some("trace_id".into()),
            message: Some("message".into()),
            message_is_text: true,
            text_time: Some("time".into()),
            own_fields: 6,
        }
    }

    #[test]
    fn coverage_is_counted_in_one_query_over_the_fields_found() {
        assert_eq!(
            coverage_sql("app_logs", &detected()),
            "SELECT COUNT(*) AS total, COUNT(\"service.name\") AS service, COUNT(\"env\") AS \
             environment, COUNT(\"trace_id\") AS trace_id, SUM(CASE WHEN \"message\" LIKE '{%' \
             THEN 1 ELSE 0 END) AS json_messages, MAX(\"time\") AS time_sample FROM \"app_logs\""
        );

        assert_eq!(
            coverage_sql("bare", &Detected::default()),
            "SELECT COUNT(*) AS total FROM \"bare\""
        );
    }

    #[test]
    fn values_are_grouped_by_the_service_and_environment_fields_found() {
        assert_eq!(
            values_sql("app_logs", &detected()).unwrap(),
            "SELECT \"service.name\" AS service, \"env\" AS environment, COUNT(*) AS count FROM \
             \"app_logs\" GROUP BY \"service.name\", \"env\" ORDER BY count DESC LIMIT 200"
        );

        assert_eq!(values_sql("bare", &Detected::default()), None);
    }

    #[test]
    fn counts_and_values_are_read_by_column_name() {
        let coverage = QueryResult {
            columns: vec!["total".into(), "service".into(), "time_sample".into()],
            rows: vec![vec![json!(120), json!(90), json!("2026-11-11 08:00:00")]],
            scanned_rows: 0,
            took_ms: 0,
            federation: None,
        };

        assert_eq!(
            read_coverage(&coverage),
            Coverage {
                total: 120,
                service: 90,
                time_sample: Some("2026-11-11 08:00:00".into()),
                ..Coverage::default()
            }
        );

        let values = QueryResult {
            columns: vec!["service".into(), "environment".into(), "count".into()],
            rows: vec![
                vec![json!("checkout"), json!("prod"), json!(70)],
                vec![Value::Null, json!(""), json!(5)],
            ],
            scanned_rows: 0,
            took_ms: 0,
            federation: None,
        };

        assert_eq!(
            read_values(&values),
            [
                ValueCount {
                    service: Some("checkout".into()),
                    environment: Some("prod".into()),
                    count: 70,
                },
                ValueCount {
                    service: None,
                    environment: None,
                    count: 5,
                },
            ]
        );
    }
}
