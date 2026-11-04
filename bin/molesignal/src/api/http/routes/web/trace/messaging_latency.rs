// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How long the messages of a topic waited between being published and
//! being consumed, read from a sample: the latest consumptions of the topic
//! (at most [`SAMPLE`]), each told against the span that published its
//! message, the one its links name, else its parent. The wait runs from the
//! end of publishing to the start of consuming.

use std::collections::HashMap;

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    filter::{self, TraceCondition},
    list::MAX_TRACE_FILTERS,
    red::{RedScope, RedUnavailable, run_tracked, settled, statements::Source},
    resolve_traces_stream_definition,
    time_spent::spend::{SpanKind, kind_of},
};
use crate::{
    api::{
        AppState,
        http::routes::web::histogram::{BucketPlan, within_budget},
    },
    app::iam::IamContext,
    domain::{iam::permission, query::QueryResult, stream::Schema},
    shared::Result,
};

/// The most consumptions a reading samples.
const SAMPLE: usize = 200;
/// The fields that name the topic a span published to or consumed from.
const DESTINATION_FIELDS: [&str; 2] = [
    "messaging.destination.name",
    "messaging.destination.template",
];

#[derive(Debug, Deserialize)]
pub(super) struct MessagingLatencyQuery {
    from: Option<i64>,
    to: Option<i64>,
    topic: String,
    /// The consuming service; every one when absent.
    consumer: Option<String>,
}

#[derive(Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MessagingLatencyResponse {
    /// Consumptions read.
    sampled: usize,
    /// Consumptions whose publishing was found.
    matched: usize,
    p50_time: Option<u64>,
    p95_time: Option<u64>,
    max_time: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable: Option<RedUnavailable>,
}

/// A consumption read, and the span that published its message.
struct Consumption {
    at_micros: i64,
    publisher: (String, String),
}

#[permission(any("traces.read", "sys.telemetry.read"))]
pub(super) async fn messaging_latency(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<MessagingLatencyQuery>,
) -> Result<Json<MessagingLatencyResponse>> {
    let plan = BucketPlan::new(query.from, query.to, None, None)?;
    let mut response = MessagingLatencyResponse::default();

    let Some(definition) = resolve_traces_stream_definition(&state, &ctx.org_id).await else {
        return Ok(Json(response));
    };

    let schema = &definition.schema;
    let Some(destination) = DESTINATION_FIELDS
        .into_iter()
        .find(|field| declared(schema, field))
    else {
        return Ok(Json(response));
    };

    // Consumptions of the topic, by the consumer asked for.
    let mut wanted =
        vec![serde_json::json!({"field": destination, "op": "=", "value": query.topic})];
    wanted.extend(
        query
            .consumer
            .iter()
            .map(|name| serde_json::json!({"field": "service.name", "op": "=", "value": name})),
    );
    let filters = filter::parse(
        Some(&Value::Array(wanted).to_string()),
        schema,
        MAX_TRACE_FILTERS,
    )?;

    let source = Source::new(
        &definition,
        RedScope::Entry,
        &TraceCondition::of(&filters, None, None),
    )?;

    let run = |statement| run_tracked(&state, &ctx, &definition.name, &plan, statement);

    let reading = async {
        let consumptions = consumptions(&run(consumed_statement(&source, schema)).await?);

        if consumptions.is_empty() {
            return Ok::<_, crate::shared::Error>((consumptions, HashMap::new()));
        }

        let statement = published_statement(&source, &consumptions);
        let published = published(&run(statement).await?);

        Ok((consumptions, published))
    };

    match settled(within_budget(reading).await)? {
        Ok((consumptions, published)) => {
            response.sampled = consumptions.len();

            let mut waits = consumptions
                .iter()
                .filter_map(|consumption| {
                    published
                        .get(&consumption.publisher)
                        .map(|end| u64::try_from(consumption.at_micros - end).unwrap_or_default())
                })
                .collect::<Vec<_>>();

            waits.sort_unstable();
            response.matched = waits.len();
            response.p50_time = percentile(&waits, 0.50);
            response.p95_time = percentile(&waits, 0.95);
            response.max_time = waits.last().copied();
        }
        Err(reason) => response.unavailable = Some(reason),
    }

    Ok(Json(response))
}

fn consumed_statement(source: &Source, schema: &Schema) -> String {
    let mut columns = vec![
        "\"trace_id\" AS __ml_trace".to_owned(),
        "_timestamp AS __ml_at".to_owned(),
    ];

    for (field, alias) in [
        ("parent_span_id", "__ml_parent"),
        ("links", "__ml_links"),
        ("kind", "__ml_kind"),
    ] {
        if declared(schema, field) {
            columns.push(format!("\"{field}\" AS {alias}"));
        }
    }

    format!(
        "SELECT {} {} ORDER BY _timestamp DESC LIMIT {SAMPLE}",
        columns.join(", "),
        source.reading_with(&[]),
    )
}

fn published_statement(source: &Source, consumptions: &[Consumption]) -> String {
    let quoted = |values: Vec<&str>| {
        let mut values = values
            .into_iter()
            .map(|value| format!("'{}'", value.replace('\'', "''")))
            .collect::<Vec<_>>();
        values.sort();
        values.dedup();
        values.join(", ")
    };
    let traces = quoted(
        consumptions
            .iter()
            .map(|item| item.publisher.0.as_str())
            .collect(),
    );
    let spans = quoted(
        consumptions
            .iter()
            .map(|item| item.publisher.1.as_str())
            .collect(),
    );

    format!(
        "SELECT \"trace_id\" AS __ml_trace, \"span_id\" AS __ml_span, _timestamp AS __ml_at, \
         {} AS __ml_duration FROM {} WHERE \"trace_id\" IN ({traces}) AND \"span_id\" IN ({spans}) \
         LIMIT {}",
        source.duration(),
        source.table(),
        SAMPLE * 4,
    )
}

fn declared(schema: &Schema, field: &str) -> bool {
    schema
        .fields
        .iter()
        .any(|candidate| candidate.name == field)
}

fn column(result: &QueryResult, name: &str) -> Option<usize> {
    result.columns.iter().position(|column| column == name)
}

fn text(row: &[Value], index: Option<usize>) -> Option<String> {
    match row.get(index?)? {
        Value::String(value) if !value.is_empty() => Some(value.clone()),
        _ => None,
    }
}

fn number(row: &[Value], index: Option<usize>) -> Option<i64> {
    let value = row.get(index?)?;

    value
        .as_i64()
        .or_else(|| value.as_f64().map(|number| number as i64))
}

/// The consumer spans read, each with the span its links name, else its parent.
fn consumptions(result: &QueryResult) -> Vec<Consumption> {
    let index = |name: &str| column(result, name);

    result
        .rows
        .iter()
        .filter(|row| {
            index("__ml_kind").is_none()
                || kind_of(row.get(index("__ml_kind").unwrap_or(usize::MAX))) == SpanKind::Consumer
        })
        .filter_map(|row| {
            let trace = text(row, index("__ml_trace"))?;
            let linked = text(row, index("__ml_links")).and_then(|links| first_link(&links));
            let publisher = linked.or_else(|| {
                text(row, index("__ml_parent")).map(|parent| (trace.clone(), parent))
            })?;

            Some(Consumption {
                at_micros: number(row, index("__ml_at"))?,
                publisher,
            })
        })
        .collect()
}

/// The trace and span of the first link of a span, as the store keeps its links.
fn first_link(links: &str) -> Option<(String, String)> {
    let links = serde_json::from_str::<Vec<Value>>(links).ok()?;
    let link = links.first()?;
    let field = |name: &str| {
        link.get(name)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };

    Some((field("trace_id")?, field("span_id")?))
}

/// When each publishing span ended, in microseconds, by its trace and span.
fn published(result: &QueryResult) -> HashMap<(String, String), i64> {
    let index = |name: &str| column(result, name);

    result
        .rows
        .iter()
        .filter_map(|row| {
            let key = (
                text(row, index("__ml_trace"))?,
                text(row, index("__ml_span"))?,
            );
            let start = number(row, index("__ml_at"))?;
            let duration = number(row, index("__ml_duration")).unwrap_or_default() / 1_000;

            Some((key, start + duration.max(0)))
        })
        .collect()
}

fn percentile(sorted: &[u64], quantile: f64) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }

    let rank = ((sorted.len() as f64) * quantile).ceil() as usize;

    sorted
        .get(rank.saturating_sub(1).min(sorted.len() - 1))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_names_the_publishing_span() {
        assert_eq!(
            first_link(r#"[{"trace_id":"t1","span_id":"s1","attributes":{}}]"#),
            Some(("t1".into(), "s1".into())),
        );
        assert_eq!(first_link("[]"), None);
        assert_eq!(first_link("not json"), None);
    }

    #[test]
    fn the_waits_are_read_at_their_ranks() {
        let waits = (1..=100).collect::<Vec<u64>>();

        assert_eq!(percentile(&waits, 0.50), Some(50));
        assert_eq!(percentile(&waits, 0.95), Some(95));
        assert_eq!(percentile(&[], 0.95), None);
    }
}
