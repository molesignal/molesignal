// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Who a stretch of trouble touched in business terms: for each business key the Spans carry,
//! the values whose requests to some services failed most over a window, such as the tenants or
//! the plans an incident hit, with the values the organization marked important told as such.
//!
//! Requests are read as the Traces page counts them: Spans where a service took a request in.

use domain::{
    business_context::{BusinessContext, BusinessSignal},
    query::{QueryLanguage, QueryRequest, StreamHint},
    stream::{StreamDefinition, StreamRepository, StreamType},
};
use kernel::{Result, ids::Id, time::TimeRange};
use serde::Serialize;
use serde_json::Value;

use crate::app::{
    query::{
        QueryService,
        statement::{Named, count, ident, literal},
    },
    trace::reading::{choose_traces_stream, entry_clause, failed_expression},
};

/// The most values told of a key.
pub const TOP_VALUES: usize = 5;

const VALUE: &str = "__impact_value";
const REQUESTS: &str = "__impact_requests";
const ERRORS: &str = "__impact_errors";

/// One business key and the values its requests had.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyImpact {
    pub key: String,
    pub label: String,
    /// The Span field read.
    pub field: String,
    /// The values with the most failed requests, then the most requests.
    pub values: Vec<ValueImpact>,
    /// More values had requests than are told.
    pub more: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueImpact {
    pub value: String,
    pub requests: u64,
    pub errors: u64,
    /// The organization marked this value important.
    pub important: bool,
}

/// What to read the impact of.
pub struct ImpactQuery<'a> {
    pub org_id: &'a Id,
    pub context: &'a BusinessContext,
    /// The services whose requests are read; none reads every service's.
    pub services: &'a [String],
    pub window: TimeRange,
}

/// The impact over each business key the traces stream carries. A key the stream lacks, or keeps
/// encrypted, is left out; so is a key no request had a value of.
pub async fn business_impact(
    query: &QueryService,
    streams: &dyn StreamRepository,
    asked: &ImpactQuery<'_>,
) -> Result<Vec<KeyImpact>> {
    if asked
        .context
        .carried_by(BusinessSignal::Span)
        .next()
        .is_none()
    {
        return Ok(Vec::new());
    }

    let traces = streams
        .list(asked.org_id)
        .await?
        .into_iter()
        .filter(|stream| stream.stream_type == StreamType::TRACES)
        .collect();

    let Some(definition) = choose_traces_stream(traces) else {
        return Ok(Vec::new());
    };

    let mut impact = Vec::new();

    for (key, field) in asked.context.carried_by(BusinessSignal::Span) {
        let Some(statement) = statement(&definition, field, asked.services) else {
            continue;
        };

        let rows = run(query, asked, &definition.name, statement).await?;
        let mut values = rows
            .iter()
            .filter_map(|row| {
                let value = text(row.first()?)?;
                let important = key.is_important(&value);

                Some(ValueImpact {
                    value,
                    requests: row.get(1).map(count).unwrap_or(0),
                    errors: row.get(2).map(count).unwrap_or(0),
                    important,
                })
            })
            .collect::<Vec<_>>();

        if values.is_empty() {
            continue;
        }

        let more = values.len() > TOP_VALUES;
        values.truncate(TOP_VALUES);

        impact.push(KeyImpact {
            key: key.key.clone(),
            label: key.label.clone(),
            field: field.to_string(),
            values,
            more,
        });
    }

    Ok(impact)
}

/// A row for each value of `field` among the requests of `services`: how many there were and how
/// many failed, the most failed first. One more row than is told, to know whether there are more.
fn statement(definition: &StreamDefinition, field: &str, services: &[String]) -> Option<String> {
    let schema = &definition.schema;
    let named = Named::resolve(schema, field).filter(|named| named.groups())?;

    if named.field().encrypted {
        return None;
    }

    let value = named.text();
    let failed = failed_expression(schema).unwrap_or_else(|| "0".to_string());
    let mut clauses = vec![format!("{value} IS NOT NULL")];

    clauses.extend(entry_clause(schema));

    if !services.is_empty() {
        let listed = services
            .iter()
            .map(|service| format!("'{}'", literal(service)))
            .collect::<Vec<_>>();

        clauses.push(format!("\"service.name\" IN ({})", listed.join(", ")));
    }

    Some(format!(
        "SELECT {value} AS {VALUE}, COUNT(*) AS {REQUESTS}, SUM({failed}) AS {ERRORS} \
         FROM \"{table}\" WHERE {where_clause} GROUP BY 1 \
         ORDER BY {ERRORS} DESC, {REQUESTS} DESC, {VALUE} ASC LIMIT {limit}",
        table = ident(&definition.name),
        where_clause = clauses.join(" AND "),
        limit = TOP_VALUES + 1,
    ))
}

async fn run(
    query: &QueryService,
    asked: &ImpactQuery<'_>,
    stream: &str,
    statement: String,
) -> Result<Vec<Vec<Value>>> {
    let result = query
        .run(QueryRequest {
            org_id: asked.org_id.clone(),
            language: QueryLanguage::Sql,
            statement,
            time_range: asked.window,
            stream: Some(StreamHint {
                name: stream.to_string(),
                stream_type: StreamType::TRACES,
            }),
            limit: Some(TOP_VALUES + 1),
            step: None,
            federation_clusters: Vec::new(),
        })
        .await?;

    Ok(result.rows)
}

fn text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use domain::stream::{FieldDef, FieldType, Schema, StreamIndexType};
    use kernel::time::TimestampMicros;
    use signals::trace::normalization::canonical_trace_schema;

    use super::*;

    fn definition(extra: &[(&str, bool)]) -> StreamDefinition {
        let mut schema: Schema = canonical_trace_schema();

        schema
            .fields
            .extend(extra.iter().map(|(name, encrypted)| FieldDef {
                name: (*name).into(),
                data_type: FieldType::Utf8,
                nullable: true,
                index_type: StreamIndexType::None,
                encrypted: *encrypted,
            }));

        StreamDefinition {
            id: Id::from_string("spans"),
            org_id: Id::from_string("org"),
            name: "default".into(),
            stream_type: StreamType::TRACES,
            schema,
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn the_values_of_the_services_requests_are_read_the_most_failed_first() {
        let statement = statement(
            &definition(&[("tenant.id", false)]),
            "tenant.id",
            &["checkout".into(), "it's".into()],
        )
        .expect("statement");

        assert!(statement.starts_with(
            "SELECT NULLIF(CAST(\"tenant.id\" AS VARCHAR), '') AS __impact_value, COUNT(*)"
        ));
        assert!(statement.contains("\"service.name\" IN ('checkout', 'it''s')"));
        assert!(statement.contains("\"kind\" IN (2, 5)"));
        assert!(statement.contains("ORDER BY __impact_errors DESC"));
        assert!(statement.ends_with(&format!("LIMIT {}", TOP_VALUES + 1)));
    }

    #[test]
    fn a_key_the_stream_lacks_or_keeps_encrypted_is_not_read() {
        let definition = definition(&[("plan", true)]);

        assert_eq!(statement(&definition, "tenant.id", &[]), None);
        assert_eq!(statement(&definition, "plan", &[]), None);
        assert!(statement(&definition, "attributes.tenant", &[]).is_some());
    }
}
