// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! One tool call as a trajectory reads it: what it is for, what it names, and how it came back.
//!
//! A call made through `tools_call` is read as the tool it forwards to. A result is the JSON the
//! history keeps (`[{"type": "json", "json": …}]`); when it was cut for length it no longer parses,
//! and its beginning is read instead.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::MANY_ROWS;
use crate::runs::TranscriptCall;

/// Keys under which a result lists what it found, when it has no `rows`.
const LIST_KEYS: [&str; 17] = [
    "traces",
    "spans",
    "services",
    "items",
    "results",
    "values",
    "series",
    "errors",
    "transactions",
    "dependencies",
    "patterns",
    "logs",
    "data",
    "streams",
    "metrics",
    "labels",
    "incidents",
];

/// Labels that name a service in PromQL matchers.
static SERVICE_MATCHER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\b(?:service|service_name|app|app_name|application|job)\s*=~?\s*"([^"]+)""#)
        .expect("service matcher pattern")
});

static NAME_MATCHER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"__name__\s*=\s*"([^"]+)""#).expect("name matcher pattern"));

/// What says nothing of a metric's name: label matchers, ranges and quoted text.
static NOT_A_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\{[^}]*\}|\[[^\]]*\]|"[^"]*""#).expect("PromQL noise pattern"));

/// Label lists of grouping and matching clauses.
static GROUPING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:by|without|on|ignoring|group_left|group_right)\s*\([^)]*\)")
        .expect("PromQL grouping pattern")
});

static IDENTIFIER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z_:][A-Za-z0-9_:]*").expect("identifier pattern"));

/// SQL that sums rows up rather than returning them.
static AGGREGATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\bgroup\s+by\b|\b(?:count|sum|avg|min|max|median|stddev|histogram|approx_distinct|approx_percentile_cont|percentile_cont)\s*\(",
    )
    .expect("aggregate pattern")
});

static SQL_LITERAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"'((?:[^']|'')*)'").expect("SQL literal pattern"));

const PROMQL_WORDS: [&str; 11] = [
    "and", "or", "unless", "bool", "offset", "by", "without", "on", "ignoring", "inf", "nan",
];

/// The kind of data a call reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Signal {
    Logs,
    Metrics,
    Traces,
    Apm,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Role {
    /// Reads data; `target` is the stream, the metric of a PromQL query, `traces`, or for APM the
    /// service.
    Read {
        signal: Signal,
        target: String,
    },
    /// Looks at what data there is and how it is shaped.
    Lookup {
        stream: Option<String>,
    },
    /// Sums data up before the model reads it.
    Feature,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    Failed,
    Empty,
    /// Rows came back: how many when they could be counted, and whether the result was cut for
    /// length.
    Data {
        rows: Option<usize>,
        cut: bool,
    },
    Unknown,
}

impl Outcome {
    pub(super) fn is_data(self) -> bool {
        matches!(self, Self::Data { .. })
    }

    pub(super) fn many_rows(self) -> bool {
        match self {
            Self::Data { rows, cut } => cut || rows.is_some_and(|rows| rows >= MANY_ROWS),
            _ => false,
        }
    }
}

pub(super) struct ToolUse {
    pub(super) name: String,
    pub(super) role: Role,
    pub(super) outcome: Outcome,
    /// Returns rows as they are rather than summing them up: a log query without aggregation, the
    /// logs around a moment, a metric range.
    pub(super) raw: bool,
    /// Services the call names in its arguments or in PromQL label matchers.
    pub(super) services: Vec<String>,
    /// The quoted values of its SQL: what a log query filtered on.
    pub(super) literals: Vec<String>,
}

impl ToolUse {
    pub(super) fn of(call: &TranscriptCall) -> Self {
        let (name, arguments) = forwarded(&call.name, &call.arguments);
        let text = |key: &str| {
            arguments
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        };

        let mut services = text("service").into_iter().collect::<Vec<_>>();
        let mut literals = Vec::new();

        for key in ["sql", "filterSql"] {
            if let Some(sql) = text(key) {
                literals.extend(sql_literals(&sql));
            }
        }

        if let Some(promql) = text("promql") {
            services.extend(promql_services(&promql));
        }

        let raw = match name {
            "query_logs" => text("sql").is_none_or(|sql| !AGGREGATE.is_match(&sql)),
            "search_around" | "query_metrics" => true,
            _ => false,
        };

        Self {
            name: name.to_string(),
            role: role(name, arguments, &text),
            outcome: outcome(call),
            raw,
            services,
            literals,
        }
    }

    /// Whether the call names the service (lowercase): as an argument, or as a quoted value.
    pub(super) fn names_service(&self, service: &str) -> bool {
        self.services
            .iter()
            .chain(self.literals.iter())
            .any(|value| value.to_lowercase() == service)
    }
}

/// The tool a call is for: the one `tools_call` forwards to, with its arguments.
fn forwarded<'a>(name: &'a str, arguments: &'a Value) -> (&'a str, &'a Value) {
    if name != "tools_call" {
        return (name, arguments);
    }

    match (
        arguments.get("name").and_then(Value::as_str),
        arguments.get("arguments"),
    ) {
        (Some(inner), Some(inner_arguments)) => (inner, inner_arguments),
        _ => (name, arguments),
    }
}

fn role(name: &str, arguments: &Value, text: &dyn Fn(&str) -> Option<String>) -> Role {
    let read = |signal: Signal, target: Option<String>, fallback: &str| Role::Read {
        signal,
        target: target.unwrap_or_else(|| fallback.to_string()),
    };

    match name {
        "query_logs" | "search_around" => read(Signal::Logs, text("stream"), "logs"),
        "search_field_values" => {
            let signal = match arguments.get("streamType").and_then(Value::as_str) {
                Some("metrics") => Signal::Metrics,
                Some("traces") => Signal::Traces,
                _ => Signal::Logs,
            };

            read(signal, text("stream"), "logs")
        }
        "query_metrics" => read(
            Signal::Metrics,
            text("promql").and_then(|promql| metric_name(&promql)),
            "promql",
        ),
        "list_traces" | "get_trace" | "get_trace_dag" => read(Signal::Traces, None, "traces"),
        "apm_overview" | "compare_apm_versions" => read(Signal::Apm, text("service"), "apm"),
        name if name.starts_with("get_apm_") || name.starts_with("list_apm_") => {
            read(Signal::Apm, text("service"), "apm")
        }
        "get_stream_schema" | "get_stream_settings" => Role::Lookup {
            stream: text("stream"),
        },
        "list_streams"
        | "list_metric_names"
        | "list_metric_labels"
        | "list_metric_label_values"
        | "list_metric_series" => Role::Lookup { stream: None },
        "analyze_log_patterns" | "analyze_metric_trend" => Role::Feature,
        _ => Role::Other,
    }
}

/// How a call came back.
fn outcome(call: &TranscriptCall) -> Outcome {
    if call.is_error {
        return Outcome::Failed;
    }

    let Some(text) = call.result.as_deref() else {
        return Outcome::Unknown;
    };

    match serde_json::from_str::<Value>(text) {
        Ok(value) => from_value(content_of(&value).as_ref(), call.truncated),
        Err(_) => from_text(text, call.truncated),
    }
}

/// The JSON a stored result carries: its first JSON content (or a text content holding JSON), or
/// the value itself when it is not a list of contents.
fn content_of(value: &Value) -> Option<Value> {
    let Some(items) = value.as_array() else {
        return Some(value.clone());
    };

    if !items.iter().all(|item| item.get("type").is_some()) {
        return Some(value.clone());
    }

    items
        .iter()
        .find_map(|item| match item.get("type").and_then(Value::as_str) {
            Some("json") => item.get("json").cloned(),
            Some("text") => item
                .get("text")
                .and_then(Value::as_str)
                .and_then(|text| serde_json::from_str(text).ok()),
            _ => None,
        })
}

fn from_value(value: Option<&Value>, cut: bool) -> Outcome {
    let Some(value) = value else {
        return Outcome::Unknown;
    };

    if let Some(rows) = value.get("rows").and_then(Value::as_array) {
        return counted(rows.len(), cut);
    }

    if let Some(items) = value.as_array() {
        return counted(items.len(), cut);
    }

    let Some(object) = value.as_object() else {
        return Outcome::Unknown;
    };

    let lists = LIST_KEYS
        .iter()
        .filter_map(|key| object.get(*key).and_then(Value::as_array))
        .collect::<Vec<_>>();

    if !lists.is_empty() {
        return counted(lists.iter().map(|list| list.len()).sum(), cut);
    }

    if object.is_empty() {
        Outcome::Empty
    } else {
        Outcome::Data { rows: None, cut }
    }
}

fn counted(rows: usize, cut: bool) -> Outcome {
    if rows == 0 && !cut {
        Outcome::Empty
    } else {
        Outcome::Data {
            rows: Some(rows),
            cut,
        }
    }
}

/// A result that does not parse: the beginning of rows, or a long result cut for length.
fn from_text(text: &str, cut: bool) -> Outcome {
    let compact = text
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();

    if compact.contains("\"rows\":[]") {
        Outcome::Empty
    } else if compact.contains("\"rows\":[[") || compact.contains("\"rows\":[{") || cut {
        Outcome::Data { rows: None, cut }
    } else {
        Outcome::Unknown
    }
}

/// The values of service-like labels a PromQL query matches on, left out when they are patterns.
fn promql_services(promql: &str) -> Vec<String> {
    SERVICE_MATCHER
        .captures_iter(promql)
        .filter_map(|captures| captures.get(1))
        .map(|value| value.as_str().to_string())
        .filter(|value| !value.contains(['*', '|', '[', '(', '+', '?']))
        .collect()
}

/// The name of the metric a PromQL query reads: a `__name__` matcher, or the first identifier
/// that is neither a function, a keyword nor a label of a grouping clause.
pub(super) fn metric_name(promql: &str) -> Option<String> {
    if let Some(name) = NAME_MATCHER
        .captures(promql)
        .and_then(|captures| captures.get(1))
    {
        return Some(name.as_str().to_string());
    }

    let plain = NOT_A_NAME.replace_all(promql, " ");
    let plain = GROUPING.replace_all(&plain, " ");

    IDENTIFIER.find_iter(&plain).find_map(|found| {
        let word = found.as_str();
        let called = plain[found.end()..].trim_start().starts_with('(');
        let keyword = PROMQL_WORDS
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(word));

        (!called && !keyword).then(|| word.to_string())
    })
}

/// The quoted values of SQL, with `LIKE` wildcards taken off.
fn sql_literals(sql: &str) -> Vec<String> {
    SQL_LITERAL
        .captures_iter(sql)
        .filter_map(|captures| captures.get(1))
        .map(|value| {
            value
                .as_str()
                .replace("''", "'")
                .trim_matches('%')
                .to_string()
        })
        .filter(|value| !value.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn call(name: &str, arguments: Value, result: Option<&str>) -> TranscriptCall {
        TranscriptCall {
            id: "c1".into(),
            name: name.into(),
            arguments,
            result: result.map(str::to_string),
            truncated: false,
            is_error: false,
        }
    }

    #[test]
    fn metric_names_skip_functions_keywords_and_grouping_labels() {
        assert_eq!(
            metric_name("sum by (job) (rate(http_requests_total{job=\"api\"}[5m]))").as_deref(),
            Some("http_requests_total")
        );
        assert_eq!(
            metric_name("histogram_quantile(0.95, sum(rate(latency_seconds_bucket[5m])) by (le))")
                .as_deref(),
            Some("latency_seconds_bucket")
        );
        assert_eq!(
            metric_name("{__name__=\"up\", job=\"node\"}").as_deref(),
            Some("up")
        );
        assert_eq!(metric_name("vector(1)"), None);
    }

    #[test]
    fn services_come_from_arguments_and_exact_label_matchers() {
        let used = ToolUse::of(&call(
            "query_metrics",
            json!({ "promql": "rate(http_requests_total{service=\"Checkout-API\", app=~\"pay.*\"}[5m])" }),
            Some(r#"[{"type":"json","json":{"rows":[[1,2]]}}]"#),
        ));

        assert_eq!(used.services, vec!["Checkout-API".to_string()]);
        assert!(used.names_service("checkout-api"));
        assert!(used.raw);
    }

    #[test]
    fn forwarded_calls_are_read_as_the_tool_they_call() {
        let used = ToolUse::of(&call(
            "tools_call",
            json!({ "name": "query_logs", "arguments": { "stream": "nginx", "sql": "SELECT count(*) FROM nginx WHERE service = 'checkout'" } }),
            Some(r#"[{"type":"json","json":{"columns":[],"rows":[]}}]"#),
        ));

        assert_eq!(used.name, "query_logs");
        assert_eq!(
            used.role,
            Role::Read {
                signal: Signal::Logs,
                target: "nginx".into()
            }
        );
        assert_eq!(used.outcome, Outcome::Empty);
        assert!(!used.raw);
        assert!(used.names_service("checkout"));
    }

    #[test]
    fn results_are_read_whole_cut_or_as_errors() {
        let rows = (0..600).map(|index| json!([index])).collect::<Vec<_>>();
        let whole = json!([{ "type": "json", "json": { "rows": rows } }]).to_string();
        assert_eq!(
            outcome(&call("query_logs", json!({}), Some(&whole))),
            Outcome::Data {
                rows: Some(600),
                cut: false
            }
        );
        assert!(outcome(&call("query_logs", json!({}), Some(&whole))).many_rows());

        let mut cut = call(
            "query_logs",
            json!({}),
            Some(r#"[{"type":"json","json":{"columns":["a"],"rows":[["x"],["y"]"#),
        );
        cut.truncated = true;
        assert_eq!(
            outcome(&cut),
            Outcome::Data {
                rows: None,
                cut: true
            }
        );

        let mut failed = call("query_logs", json!({}), Some("stream not found"));
        failed.is_error = true;
        assert_eq!(outcome(&failed), Outcome::Failed);

        let apm = call(
            "get_apm_service",
            json!({ "service": "checkout" }),
            Some(r#"[{"type":"json","json":{"service":{"name":"checkout"},"errors":[]}}]"#),
        );
        assert_eq!(
            outcome(&apm),
            Outcome::Empty,
            "a result listing nothing is empty"
        );

        let text = call("query_logs", json!({}), Some("plain words"));
        assert_eq!(outcome(&text), Outcome::Unknown);
    }
}
