// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A query enrich step: a SQL or PromQL statement with values of the event in it, run when the
//! incident opens; the rows it returns, up to a limit, go into an annotation as text.

use serde_json::{Map, Value};

use crate::{
    domain::{
        pipeline::realtime::public_stream,
        query::{QueryLanguage, QueryResult, StreamHint},
        stream::{AUDIT_STREAM, StreamType},
    },
    shared::{Error, Result},
};

/// The annotation a query enrich step writes unless it names another.
pub const DEFAULT_QUERY_ANNOTATION: &str = "query_result";

/// How far back a query looks unless the step says.
pub const DEFAULT_QUERY_WINDOW_SECS: u32 = 300;

/// How far back a query may look.
pub const MAX_QUERY_WINDOW_SECS: u32 = 86_400;

/// How many rows go into the annotation unless the step says.
pub const DEFAULT_QUERY_ROWS: u32 = 5;

/// The most rows a step may put into an annotation.
pub const MAX_QUERY_ROWS: u32 = 50;

/// The longest statement a step may hold.
const MAX_STATEMENT_LENGTH: usize = 8 * 1024;

/// The longest text a query leaves in an annotation.
const MAX_ANNOTATION_CHARS: usize = 4_000;

/// What a query enrich step runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrichQuery {
    pub language: QueryLanguage,
    pub stream: String,
    pub stream_type: StreamType,
    /// `{{labels.host}}` and the like stand for values of the event, inside a string of the
    /// statement.
    pub statement: String,
    pub window_secs: u32,
    pub max_rows: u32,
    pub annotation: String,
}

impl EnrichQuery {
    pub fn stream_hint(&self) -> StreamHint {
        StreamHint {
            name: self.stream.clone(),
            stream_type: self.stream_type,
        }
    }

    /// What a saved step has to hold. The statement's own rules are the query language's, checked
    /// where a pipeline is saved.
    pub(super) fn checked(self) -> Result<Self> {
        public_stream(&self.stream)?;

        if self.stream == AUDIT_STREAM {
            return Err(Error::invalid(
                "a query step cannot read `_audit`: it runs in the background, where nobody's \
                 audit.read applies",
            ));
        }

        if !matches!(
            self.stream_type,
            StreamType::LOGS | StreamType::METRICS | StreamType::TRACES
        ) {
            return Err(Error::invalid(
                "a query step reads a stream of logs, metrics or traces",
            ));
        }

        if self.language == QueryLanguage::Promql && self.stream_type != StreamType::METRICS {
            return Err(Error::invalid("a PromQL query reads a metrics stream"));
        }

        if self.statement.trim().is_empty() {
            return Err(Error::invalid("a query step needs a statement"));
        }

        if self.statement.len() > MAX_STATEMENT_LENGTH {
            return Err(Error::invalid(format!(
                "a query step's statement is at most {MAX_STATEMENT_LENGTH} bytes"
            )));
        }

        if !(1..=MAX_QUERY_WINDOW_SECS).contains(&self.window_secs) {
            return Err(Error::invalid(format!(
                "a query step looks back 1 to {MAX_QUERY_WINDOW_SECS} seconds"
            )));
        }

        if !(1..=MAX_QUERY_ROWS).contains(&self.max_rows) {
            return Err(Error::invalid(format!(
                "a query step keeps 1 to {MAX_QUERY_ROWS} rows"
            )));
        }

        Ok(self)
    }
}

/// The value a placeholder stands for: `summary`, `severity`, `priority`, `urgency`, `rule_id`,
/// `rule_name`, `source`, `labels.<key>` or `annotations.<key>`.
pub fn placeholder_value(
    event: &Value,
    metadata: &Map<String, Value>,
    path: &str,
) -> Option<String> {
    let text = |value: Option<&Value>| match value? {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    };

    if let Some(key) = path.strip_prefix("labels.") {
        return text(event.get("labels").and_then(|labels| labels.get(key)));
    }

    if let Some(key) = path.strip_prefix("annotations.") {
        return text(event.get("annotations").and_then(|notes| notes.get(key)));
    }

    match path {
        "summary" | "severity" | "priority" | "urgency" => text(event.get(path)),
        "rule_id" | "rule_name" | "source" => text(metadata.get(path)),
        _ => None,
    }
}

/// Whether a placeholder names something [`placeholder_value`] can read.
pub fn known_placeholder(path: &str) -> bool {
    matches!(
        path,
        "summary" | "severity" | "priority" | "urgency" | "rule_id" | "rule_name" | "source"
    ) || path
        .strip_prefix("labels.")
        .or_else(|| path.strip_prefix("annotations."))
        .is_some_and(|key| !key.trim().is_empty())
}

/// The rows of a result as an annotation reads: one line per row, a lone value by itself and several
/// as `column=value, column=value`. No rows is `None`: the annotation is left out.
pub fn rows_text(result: &QueryResult, max_rows: u32) -> Option<String> {
    let rows = result.rows.iter().take(max_rows as usize);

    let lines = rows
        .map(|row| {
            if let [value] = row.as_slice() {
                return value_text(value);
            }

            result
                .columns
                .iter()
                .zip(row)
                .map(|(column, value)| format!("{column}={}", value_text(value)))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .collect::<Vec<_>>();

    if lines.is_empty() {
        return None;
    }

    let text = lines.join("\n");

    if text.chars().count() <= MAX_ANNOTATION_CHARS {
        return Some(text);
    }

    let mut cut: String = text.chars().take(MAX_ANNOTATION_CHARS).collect();
    cut.push('…');
    Some(cut)
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn result(columns: &[&str], rows: Vec<Vec<Value>>) -> QueryResult {
        QueryResult {
            columns: columns.iter().map(|name| (*name).to_owned()).collect(),
            rows,
            scanned_rows: 0,
            took_ms: 0,
            federation: None,
        }
    }

    #[test]
    fn rows_read_as_lines_up_to_the_limit() {
        let one = result(&["count"], vec![vec![json!(12)]]);
        assert_eq!(rows_text(&one, 5).as_deref(), Some("12"));

        let several = result(
            &["host", "errors"],
            vec![
                vec![json!("web-1"), json!(3)],
                vec![json!("web-2"), json!(null)],
                vec![json!("web-3"), json!(1)],
            ],
        );
        assert_eq!(
            rows_text(&several, 2).as_deref(),
            Some("host=web-1, errors=3\nhost=web-2, errors=")
        );

        assert_eq!(rows_text(&result(&["x"], Vec::new()), 5), None);

        let long = result(
            &["x"],
            vec![vec![json!("y".repeat(MAX_ANNOTATION_CHARS + 9))]],
        );
        assert_eq!(
            rows_text(&long, 1).unwrap().chars().count(),
            MAX_ANNOTATION_CHARS + 1
        );
    }

    #[test]
    fn a_placeholder_reads_the_event_and_where_it_came_from() {
        let event = json!({
            "summary": "Disk full",
            "severity": "error",
            "priority": null,
            "labels": { "host": "web-1", "k8s.pod": "a", "n": 3 },
            "annotations": { "runbook": "x" },
        });
        let metadata = json!({ "rule_name": "Disk", "source": "rule" });
        let metadata = metadata.as_object().unwrap();
        let read = |path: &str| placeholder_value(&event, metadata, path);

        assert_eq!(read("labels.host").as_deref(), Some("web-1"));
        assert_eq!(read("labels.k8s.pod").as_deref(), Some("a"));
        assert_eq!(read("labels.n").as_deref(), Some("3"));
        assert_eq!(read("annotations.runbook").as_deref(), Some("x"));
        assert_eq!(read("rule_name").as_deref(), Some("Disk"));
        assert_eq!(read("severity").as_deref(), Some("error"));
        assert_eq!(read("priority"), None);
        assert_eq!(read("labels.gone"), None);
        assert_eq!(read("fingerprint"), None);

        assert!(known_placeholder("labels.host") && known_placeholder("rule_id"));
        assert!(!known_placeholder("labels.") && !known_placeholder("fingerprint"));
    }
}
