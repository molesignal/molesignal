// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::DateTime;
use promql_parser::parser as promql_parser;

use crate::{
    app::query::QueryService,
    domain::{
        dashboard::authoring::{
            AuthoringElement, AuthoringQuery, DashboardAuthoringSpec, DashboardQueryPreflight,
            PanelAuthoringSpec, PanelPreflight, PreflightReport, PreflightStatus, PreflightWarning,
            SectionElement, visualization_manifest,
        },
        query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
        stream::{StreamDefinition, StreamRepository, StreamType},
    },
    infra::query::parser::{extract_referenced_tables, prepare_flight_sql_select},
    shared::{
        Error, Result,
        contracts::ContractIssue,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

pub struct RuntimeDashboardQueryPreflight {
    query: Arc<QueryService>,
    streams: Arc<dyn StreamRepository>,
}

impl RuntimeDashboardQueryPreflight {
    pub fn new(query: Arc<QueryService>, streams: Arc<dyn StreamRepository>) -> Self {
        Self { query, streams }
    }
}

#[async_trait]
impl DashboardQueryPreflight for RuntimeDashboardQueryPreflight {
    async fn preflight(
        &self,
        org_id: &Id,
        _actor: &Id,
        spec: &DashboardAuthoringSpec,
    ) -> Result<PreflightReport> {
        let mut report = PreflightReport::default();
        let now = TimestampMicros::now();
        let time_range = match authoring_time_range(spec, now) {
            Ok(range) => range,
            Err(issue) => {
                report.issues.push(issue);
                return Ok(report);
            }
        };
        let available_streams = self.streams.list(org_id).await?;
        for (panel_path, panel) in authoring_panels(spec) {
            for (query_index, query) in panel.queries.iter().enumerate() {
                let path = format!("{panel_path}/queries/{query_index}");
                match self
                    .preflight_query(org_id, &available_streams, query, &path, time_range)
                    .await
                {
                    Ok(result) => {
                        if result.rows.is_empty() {
                            report.warnings.push(PreflightWarning {
                                code: "PREFLIGHT_EMPTY_RESULT".into(),
                                path: path.clone(),
                                message: format!(
                                    "panel '{}' returned no rows in the tested time range",
                                    panel.title
                                ),
                            });
                        }
                        report.panels.push(PanelPreflight {
                            path,
                            title: panel.title.clone(),
                            query_kind: query.kind().into(),
                            status: if result.rows.is_empty() {
                                PreflightStatus::Empty
                            } else {
                                PreflightStatus::Passed
                            },
                            tested_from_micros: time_range.start.0,
                            tested_to_micros: time_range.end.0,
                            returned_rows: result.rows.len(),
                            scanned_rows: result.scanned_rows,
                            took_ms: result.took_ms,
                        });
                    }
                    Err(issue) => report.issues.push(issue),
                }
            }
        }
        Ok(report)
    }
}

impl RuntimeDashboardQueryPreflight {
    async fn preflight_query(
        &self,
        org_id: &Id,
        available_streams: &[StreamDefinition],
        query: &AuthoringQuery,
        path: &str,
        time_range: TimeRange,
    ) -> std::result::Result<crate::domain::query::QueryResult, ContractIssue> {
        let request = build_query_request(org_id, available_streams, query, path, time_range)?;
        let limits = &visualization_manifest().limits;
        let timeout = Duration::from_millis(limits.preflight_timeout_ms);
        let result = tokio::time::timeout(timeout, async {
            if request.language == QueryLanguage::Sql {
                self.query.explain(request.clone()).await?;
            }
            self.query.run(request).await
        })
        .await
        .map_err(|_| {
            issue(
                "PREFLIGHT_TIMEOUT",
                path,
                format!(
                    "query preflight exceeded the {}ms timeout",
                    limits.preflight_timeout_ms
                ),
            )
        })?
        .map_err(|error| query_error(path, error))?;
        enforce_result_budget(&result, path)?;
        Ok(result)
    }
}

fn enforce_result_budget(
    result: &QueryResult,
    path: &str,
) -> std::result::Result<(), ContractIssue> {
    let limits = &visualization_manifest().limits;
    let result_bytes = serde_json::to_vec(&result.rows).map_or(usize::MAX, |bytes| bytes.len());
    if result.rows.len() > limits.preflight_max_rows || result_bytes > limits.preflight_max_bytes {
        return Err(issue(
            "PREFLIGHT_BUDGET_EXCEEDED",
            path,
            format!(
                "query preflight exceeded the {} row or {} byte budget",
                limits.preflight_max_rows, limits.preflight_max_bytes
            ),
        ));
    }
    Ok(())
}

fn build_query_request(
    org_id: &Id,
    streams: &[StreamDefinition],
    query: &AuthoringQuery,
    path: &str,
    time_range: TimeRange,
) -> std::result::Result<QueryRequest, ContractIssue> {
    let limits = &visualization_manifest().limits;
    let (language, statement, stream) = match query {
        AuthoringQuery::Promql {
            expression, stream, ..
        } => {
            promql_parser::parse(expression)
                .map_err(|error| issue("INVALID_QUERY", path, error))?;
            let hint = match stream {
                Some(stream) => Some(resolve_stream(
                    streams,
                    stream,
                    Some(StreamType::Metrics),
                    path,
                )?),
                None => None,
            };
            (QueryLanguage::Promql, expression.clone(), hint)
        }
        AuthoringQuery::Sql {
            stream, statement, ..
        } => {
            let prepared =
                prepare_flight_sql_select(statement).map_err(|error| query_error(path, error))?;
            let references =
                extract_referenced_tables(statement).map_err(|error| query_error(path, error))?;
            if references.iter().any(|reference| reference.name != *stream) {
                return Err(issue(
                    "QUERY_STREAM_SCOPE_MISMATCH",
                    path,
                    "SQL may reference only its declared stream",
                ));
            }
            let hint = resolve_stream(streams, stream, None, path)?;
            (QueryLanguage::Sql, prepared.sql, Some(hint))
        }
        AuthoringQuery::Trace {
            stream,
            query,
            limit: _,
            ..
        } => {
            let hint = resolve_stream(streams, stream, Some(StreamType::Traces), path)?;
            let statement = if query
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("select")
            {
                prepare_flight_sql_select(query)
                    .map_err(|error| query_error(path, error))?
                    .sql
            } else {
                format!(
                    "SELECT * FROM \"{}\" WHERE {}",
                    stream.replace('"', "\"\""),
                    query
                )
            };
            (QueryLanguage::Sql, statement, Some(hint))
        }
        AuthoringQuery::Profile {
            stream,
            query,
            profile_type: _,
            ..
        } => {
            validate_read_only_filter(query, path)?;
            let hint = resolve_stream(streams, stream, Some(StreamType::Profiles), path)?;
            (
                QueryLanguage::Sql,
                format!(
                    "SELECT * FROM \"{}\" LIMIT {}",
                    stream.replace('"', "\"\""),
                    limits.preflight_max_rows
                ),
                Some(hint),
            )
        }
    };
    Ok(QueryRequest {
        org_id: org_id.clone(),
        language,
        statement,
        time_range,
        stream,
        limit: Some(limits.preflight_max_rows),
        federation_clusters: Vec::new(),
    })
}

fn resolve_stream(
    streams: &[StreamDefinition],
    name: &str,
    expected_type: Option<StreamType>,
    path: &str,
) -> std::result::Result<StreamHint, ContractIssue> {
    let matches = streams
        .iter()
        .filter(|stream| {
            stream.name == name && expected_type.is_none_or(|kind| stream.stream_type == kind)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [stream] => Ok(StreamHint {
            name: stream.name.clone(),
            stream_type: stream.stream_type,
        }),
        [] => Err(issue(
            "STREAM_NOT_FOUND",
            path,
            format!("stream '{name}' was not found in the current organization"),
        )),
        _ => Err(issue(
            "AMBIGUOUS_STREAM",
            path,
            format!("stream '{name}' exists with multiple signal types"),
        )),
    }
}

fn authoring_time_range(
    spec: &DashboardAuthoringSpec,
    now: TimestampMicros,
) -> std::result::Result<TimeRange, ContractIssue> {
    let from = spec
        .time_range
        .as_ref()
        .map_or("now-6h", |range| range.from.as_str());
    let to = spec
        .time_range
        .as_ref()
        .map_or("now", |range| range.to.as_str());
    let start = parse_time(from, now).ok_or_else(|| {
        issue(
            "INVALID_TIME_RANGE",
            "/timeRange/from",
            "timeRange.from must be an RFC3339 timestamp or now-<duration>",
        )
    })?;
    let end = parse_time(to, now).ok_or_else(|| {
        issue(
            "INVALID_TIME_RANGE",
            "/timeRange/to",
            "timeRange.to must be an RFC3339 timestamp or now-<duration>",
        )
    })?;
    if start >= end {
        return Err(issue(
            "INVALID_TIME_RANGE",
            "/timeRange",
            "time range start must be before end",
        ));
    }
    let max_lookback = visualization_manifest()
        .limits
        .max_lookback_seconds
        .saturating_mul(1_000_000) as i64;
    if end.0.saturating_sub(start.0) > max_lookback {
        return Err(issue(
            "PREFLIGHT_LOOKBACK_EXCEEDED",
            "/timeRange",
            "time range exceeds the Dashboard authoring preflight lookback budget",
        ));
    }
    Ok(TimeRange::new(start, end))
}

fn parse_time(value: &str, now: TimestampMicros) -> Option<TimestampMicros> {
    let value = value.trim();
    if value == "now" {
        return Some(now);
    }
    if let Some(duration) = value.strip_prefix("now-") {
        let split = duration
            .find(|character: char| !character.is_ascii_digit())
            .unwrap_or(duration.len());
        let amount = duration[..split].parse::<i64>().ok()?;
        let unit = &duration[split..];
        let seconds = match unit {
            "s" => amount,
            "m" => amount.saturating_mul(60),
            "h" => amount.saturating_mul(3_600),
            "d" => amount.saturating_mul(86_400),
            "w" => amount.saturating_mul(604_800),
            _ => return None,
        };
        return Some(TimestampMicros(
            now.0.saturating_sub(seconds.saturating_mul(1_000_000)),
        ));
    }
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| TimestampMicros(value.timestamp_micros()))
}

fn authoring_panels(spec: &DashboardAuthoringSpec) -> Vec<(String, &PanelAuthoringSpec)> {
    let mut panels = Vec::new();
    for (index, element) in spec.elements.iter().enumerate() {
        match element {
            AuthoringElement::Panel(panel) => panels.push((format!("/elements/{index}"), panel)),
            AuthoringElement::Section(section) => {
                for (child_index, child) in section.elements.iter().enumerate() {
                    if let SectionElement::Panel(panel) = child {
                        panels.push((format!("/elements/{index}/elements/{child_index}"), panel));
                    }
                }
            }
            AuthoringElement::Text(_) => {}
        }
    }
    panels
}

fn validate_read_only_filter(filter: &str, path: &str) -> std::result::Result<(), ContractIssue> {
    let normalized = filter.to_ascii_lowercase();
    if filter.contains(';')
        || [
            " insert ", " update ", " delete ", " drop ", " alter ", " create ",
        ]
        .iter()
        .any(|keyword| format!(" {normalized} ").contains(keyword))
    {
        return Err(issue(
            "QUERY_NOT_READ_ONLY",
            path,
            "profile filter must be a single read-only expression",
        ));
    }
    Ok(())
}

fn query_error(path: &str, error: Error) -> ContractIssue {
    let message = error.to_string();
    issue(
        "INVALID_QUERY",
        path,
        message.chars().take(500).collect::<String>(),
    )
}

fn issue(code: &str, path: &str, message: impl Into<String>) -> ContractIssue {
    ContractIssue::new(code, path, message, true)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_bounded_relative_time_ranges() {
        let now = TimestampMicros(10_000_000_000);
        assert_eq!(parse_time("now", now), Some(now));
        assert_eq!(
            parse_time("now-1h", now),
            Some(TimestampMicros(6_400_000_000))
        );
        assert_eq!(parse_time("now-1fortnight", now), None);
    }

    #[test]
    fn rejects_mutating_profile_filters() {
        assert!(validate_read_only_filter("service = 'api'", "/query").is_ok());
        assert!(validate_read_only_filter("x = 1; DROP TABLE x", "/query").is_err());
    }

    #[test]
    fn rejects_preflight_lookback_and_result_budgets() {
        let spec: DashboardAuthoringSpec = serde_json::from_value(json!({
            "authoringVersion": 1,
            "title": "Over budget",
            "timeRange": {"from": "now-31d", "to": "now"},
            "elements": [{"kind": "text", "content": "Budget check"}]
        }))
        .unwrap();
        let lookback = authoring_time_range(&spec, TimestampMicros(4_000_000_000_000)).unwrap_err();
        assert_eq!(lookback.code, "PREFLIGHT_LOOKBACK_EXCEEDED");

        let limits = &visualization_manifest().limits;
        let too_many_rows = QueryResult {
            columns: vec!["value".into()],
            rows: vec![vec![json!(1)]; limits.preflight_max_rows + 1],
            scanned_rows: 0,
            took_ms: 0,
            federation: None,
        };
        assert_eq!(
            enforce_result_budget(&too_many_rows, "/elements/0/queries/0")
                .unwrap_err()
                .code,
            "PREFLIGHT_BUDGET_EXCEEDED"
        );

        let too_many_bytes = QueryResult {
            columns: vec!["value".into()],
            rows: vec![vec![json!("x".repeat(limits.preflight_max_bytes))]],
            scanned_rows: 0,
            took_ms: 0,
            federation: None,
        };
        assert_eq!(
            enforce_result_budget(&too_many_bytes, "/elements/0/queries/0")
                .unwrap_err()
                .code,
            "PREFLIGHT_BUDGET_EXCEEDED"
        );
    }
}
