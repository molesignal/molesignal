// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The root Span's own fields for a page of traces.
//!
//! A trace summary keeps a few fields of the Span it was written from: its id,
//! service, name, status and HTTP status. Any other field of it a reader puts
//! beside the traces is read for the page from the Spans themselves: each
//! trace's root by its trace and Span id, in the moments the page's traces
//! began. Only a page whose reader asked for such a field is read, at once,
//! and a reading that fails leaves the page as it is.

use std::collections::{BTreeSet, HashMap};

use super::TraceListRow;
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        query::{QueryLanguage, QueryRequest, StreamHint},
        stream::{FieldType, Schema, StreamDefinition, StreamType},
    },
    infra::query::escape_sql_ident,
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

/// The most fields of the root a page is asked for at once.
pub(super) const MAX_ROOT_FIELDS: usize = 12;

/// The fields a reader asked to see beside the traces, checked against the
/// stream: each is a field of a Span, not a document, named as the stream names it.
pub(super) fn requested_fields(raw: Option<&str>, schema: &Schema) -> Result<Vec<String>> {
    let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Ok(Vec::new());
    };

    let mut fields = Vec::new();

    for name in raw
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        if name.len() > 128 || name.starts_with("molesignal.trace.") {
            return Err(Error::invalid(format!("unsupported root field: {name}")));
        }

        let declared = schema
            .fields
            .iter()
            .find(|field| field.name == name)
            .ok_or_else(|| Error::invalid(format!("unsupported root field: {name}")))?;

        if declared.data_type == FieldType::Json {
            return Err(Error::invalid(format!(
                "root field `{name}` is a document; it is read in SQL mode"
            )));
        }

        if !fields.iter().any(|field| field == name) {
            fields.push(name.to_string());
        }
    }

    if fields.len() > MAX_ROOT_FIELDS {
        return Err(Error::invalid(format!(
            "at most {MAX_ROOT_FIELDS} root fields are read at once"
        )));
    }

    Ok(fields)
}

/// Reads the `requested` fields of the roots of `rows`, and writes them onto the rows.
pub(super) async fn attach(
    state: &AppState,
    ctx: &IamContext,
    definition: &StreamDefinition,
    requested: &[String],
    rows: &mut [TraceListRow],
) -> Result<()> {
    let columns = requested.iter().cloned().collect::<BTreeSet<_>>();

    let Some(statement) = statement(&definition.name, &columns, rows) else {
        return Ok(());
    };

    // A root begins when its trace does: the page's range is that of its starts.
    let starts = rows.iter().map(|row| row.item.start_ns.div_euclid(1_000));
    let from = starts.clone().min().unwrap_or_default();
    let to = starts.max().unwrap_or_default().saturating_add(1);

    let request = QueryRequest {
        org_id: ctx.org_id.clone(),
        language: QueryLanguage::Sql,
        statement,
        time_range: TimeRange::new(TimestampMicros(from), TimestampMicros(to)),
        stream: Some(StreamHint {
            name: definition.name.clone(),
            stream_type: StreamType::TRACES,
        }),
        limit: Some(rows.len().saturating_mul(2)),
        step: None,
        federation_clusters: Vec::new(),
    };

    let result = match state
        .query
        .run_tracked(request, ctx.user_id.clone(), ctx.organization_role_key())
        .await
    {
        Ok(result) => result,
        Err(error) => {
            // The page is still the page without them.
            tracing::debug!(%error, "root fields of a trace page were not read");
            return Ok(());
        }
    };

    let position = |name: &str| result.columns.iter().position(|column| column == name);
    let (Some(trace_column), Some(span_column)) = (position("trace_id"), position("span_id"))
    else {
        return Ok(());
    };

    let roots = result
        .rows
        .iter()
        .filter_map(|row| {
            let trace = row.get(trace_column)?.as_str()?;
            let span = row.get(span_column)?.as_str()?;
            Some(((trace, span), row))
        })
        .collect::<HashMap<_, _>>();

    for row in rows.iter_mut() {
        let Some(root) = roots.get(&(row.item.trace_id.as_str(), row.root_span_id.as_str())) else {
            continue;
        };

        let value_of = |name: &str| position(name).and_then(|index| root.get(index)).cloned();

        for name in requested {
            if let Some(value) = value_of(name).filter(|value| !value.is_null()) {
                row.item.fields.insert(name.clone(), value);
            }
        }
    }

    Ok(())
}

/// The statement that reads the roots of `rows`; `None` when there is nothing to read.
fn statement(stream: &str, columns: &BTreeSet<String>, rows: &[TraceListRow]) -> Option<String> {
    let roots = rows
        .iter()
        .filter(|row| !row.root_span_id.is_empty())
        .collect::<Vec<_>>();

    if columns.is_empty() || roots.is_empty() {
        return None;
    }

    let traces = roots
        .iter()
        .map(|row| literal(&row.item.trace_id))
        .collect::<BTreeSet<_>>();

    let spans = roots
        .iter()
        .map(|row| literal(&row.root_span_id))
        .collect::<BTreeSet<_>>();

    let selected = ["trace_id", "span_id"]
        .into_iter()
        .map(str::to_string)
        .chain(columns.iter().cloned())
        .map(|name| quoted(&name))
        .collect::<Vec<_>>();

    Some(format!(
        "SELECT {} FROM {} WHERE \"trace_id\" IN ({}) AND \"span_id\" IN ({})",
        selected.join(", "),
        quoted(stream),
        traces.into_iter().collect::<Vec<_>>().join(", "),
        spans.into_iter().collect::<Vec<_>>().join(", "),
    ))
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", escape_sql_ident(name))
}

fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        api::http::routes::web::trace::list::TraceListItem, domain::stream::FieldDef,
        shared::trace::normalization::canonical_trace_schema,
    };

    fn schema() -> Schema {
        let mut schema = canonical_trace_schema();

        for (name, data_type) in [
            ("http.route", FieldType::Utf8),
            ("http.response.status_code", FieldType::Int64),
        ] {
            schema.fields.push(FieldDef {
                name: name.into(),
                data_type,
                nullable: true,
                index_type: None,
                indexed: false,
                encrypted: false,
                exact: false,
            });
        }

        schema
    }

    fn row(trace_id: &str, root_span_id: &str) -> TraceListRow {
        TraceListRow {
            item: TraceListItem {
                trace_id: trace_id.into(),
                start_ns: 1_000_000,
                ..Default::default()
            },
            duration_ns: 0,
            root_span_id: root_span_id.into(),
        }
    }

    #[test]
    fn a_requested_field_is_a_field_of_the_stream_named_as_it_is() {
        let schema = schema();

        assert_eq!(
            requested_fields(Some("http.route, http.route ,name"), &schema).unwrap(),
            vec!["http.route".to_string(), "name".to_string()]
        );
        assert!(requested_fields(None, &schema).unwrap().is_empty());

        for refused in ["route", "attributes", "molesignal.trace.span_count"] {
            assert!(
                requested_fields(Some(refused), &schema).is_err(),
                "{refused}"
            );
        }
    }

    #[test]
    fn reads_the_roots_of_a_page_by_trace_and_span() {
        let columns = ["http.route".to_string()].into_iter().collect();
        let rows = [row("t1", "r1"), row("t'2", "r2"), row("t3", "")];

        assert_eq!(
            statement("traces", &columns, &rows).unwrap(),
            "SELECT \"trace_id\", \"span_id\", \"http.route\" FROM \"traces\" \
             WHERE \"trace_id\" IN ('t''2', 't1') AND \"span_id\" IN ('r1', 'r2')"
        );
        assert!(statement("traces", &BTreeSet::new(), &rows).is_none());
    }
}
