// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Value, json};

use crate::domain::dashboard::authoring::AuthoringQuery;

pub(super) struct CompiledQuery {
    pub data_source_type: &'static str,
    pub payload: Value,
    pub legend: Option<String>,
    pub format: String,
}

pub(super) fn compile_query(query: &AuthoringQuery) -> CompiledQuery {
    match query {
        AuthoringQuery::Promql {
            expression,
            stream,
            legend,
            format,
            step,
        } => {
            let mut payload = json!({
                "kind": "promql",
                "language": "promql",
                "expression": expression
            });
            insert_optional(&mut payload, "streamName", stream.as_deref());
            insert_optional(&mut payload, "step", step.as_deref());
            CompiledQuery {
                data_source_type: "metrics",
                payload,
                legend: legend.clone(),
                format: format.clone().unwrap_or_else(|| "time_series".into()),
            }
        }
        AuthoringQuery::Sql {
            stream,
            statement,
            time_column,
            legend,
            format,
        } => {
            let format = format.clone().unwrap_or_else(|| "table".into());
            let mut payload = json!({
                "kind": "sql",
                "language": "sql",
                "statement": statement,
                "streamName": stream
            });
            insert_optional(&mut payload, "timeColumn", time_column.as_deref());
            CompiledQuery {
                data_source_type: if format == "logs" { "logs" } else { "sql" },
                payload,
                legend: legend.clone(),
                format,
            }
        }
        AuthoringQuery::Trace {
            stream,
            query,
            limit,
            legend,
        } => {
            let statement = if query
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("select")
            {
                query.clone()
            } else {
                format!(
                    "SELECT * FROM \"{}\" WHERE {}",
                    stream.replace('"', "\"\""),
                    query
                )
            };
            CompiledQuery {
                data_source_type: "traces",
                payload: json!({
                    "kind": "trace",
                    "language": "sql",
                    "statement": statement,
                    "filter": query,
                    "streamName": stream,
                    "streamType": "traces",
                    "limit": limit.unwrap_or(100).min(1000)
                }),
                legend: legend.clone(),
                format: "traces".into(),
            }
        }
        AuthoringQuery::Profile {
            stream,
            query,
            profile_type,
            aggregate,
            legend,
        } => CompiledQuery {
            data_source_type: "profiles",
            payload: json!({
                "kind": "profile",
                "streamName": stream,
                "filter": query,
                "label": query,
                "profileType": profile_type,
                "aggregate": aggregate.as_deref().unwrap_or("sum")
            }),
            legend: legend.clone(),
            format: "profiles".into(),
        },
    }
}

fn insert_optional(target: &mut Value, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        target[key] = Value::String(value.to_string());
    }
}
