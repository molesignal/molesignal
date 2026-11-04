// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The statements RED figures are read with.
//!
//! Every result column is named with a prefix no field has. A result column
//! inherits the masking of what it is derived from, so the figures of a masked
//! field come back masked with it, and one named like a field would inherit
//! that field's masking too.

use super::RedScope;
pub(super) use crate::api::http::routes::web::trace::latency::latency_edges;
use crate::{
    api::http::routes::web::{
        histogram::BucketPlan,
        trace::{
            filter::{TraceCondition, TraceFilter, TraceFilterScope},
            latency::PREFERRED,
        },
    },
    domain::stream::{FieldDef, FieldType, Schema, StreamDefinition},
    infra::query::escape_sql_ident,
    shared::{Error, Result},
};

pub(super) const REQUESTS: &str = "__red_requests";
pub(super) const ERRORS: &str = "__red_errors";
pub(super) const P50: &str = "__red_p50";
pub(super) const P95: &str = "__red_p95";
pub(super) const P99: &str = "__red_p99";
pub(super) const BUCKET: &str = "__red_bucket";
pub(super) const BAR: &str = "__red_bar";
pub(super) const VALUE: &str = "__red_value";
pub(super) const PRESENT: &str = "__red_present";

/// When a Span began, in microseconds. `_timestamp` is stored as a moment,
/// which the engine does not compare with a whole number.
const MOMENT: &str = "CAST(_timestamp AS BIGINT)";

/// The fields free text is looked for in, as the trace list looks for it.
const TEXT_FIELDS: [&str; 5] = ["trace_id", "span_id", "service.name", "name", "status_code"];

/// The Spans the figures are counted over: what every one of them has to meet,
/// and how to tell whether one failed and how long it took. The Span list
/// reads its Spans with the same conditions.
pub(in super::super) struct Source {
    /// The stream, quoted.
    table: String,
    /// Conditions every Span has to meet; none reads every Span of the range.
    clauses: Vec<String>,
    /// 1 for a Span that failed, 0 for one that did not. `None` for a stream
    /// that keeps no status: its failures cannot be told.
    failed: Option<String>,
    /// How long a Span took, in whole nanoseconds.
    duration: String,
}

impl Source {
    /// The Spans of `scope` that meet `query`: the filters, the free text and
    /// the condition tree of a trace query (`TraceCondition::of`).
    pub(in super::super) fn new(
        definition: &StreamDefinition,
        scope: RedScope,
        query: &TraceCondition,
    ) -> Result<Self> {
        let schema = &definition.schema;
        let table = quoted(&definition.name);

        let failed = declared(schema, "status_code").map(|_| {
            "CASE WHEN UPPER(CAST(\"status_code\" AS VARCHAR)) = 'ERROR' THEN 1 ELSE 0 END"
                .to_string()
        });

        let duration = if declared(schema, "duration_ns").is_some() {
            quoted("duration_ns")
        } else {
            "(\"end_time_unix_nano\" - \"start_time_unix_nano\")".to_string()
        };

        let mut source = Self {
            table,
            clauses: Vec::new(),
            failed,
            duration,
        };

        if scope == RedScope::Entry
            && let Some(entry) = entry_clause(schema)
        {
            source.clauses.push(entry);
        }

        for part in query.conjuncts() {
            let clause = source.condition_clause(schema, part)?;
            source.clauses.push(clause);
        }

        Ok(source)
    }

    /// What a part of the query asks of a Span: each comparison as
    /// `filter_clause` asks it, free text in the fields the trace list looks
    /// in, and a group as the AND or the OR of its parts.
    fn condition_clause(&self, schema: &Schema, part: &TraceCondition) -> Result<String> {
        let joined = |items: &[TraceCondition], separator: &str| -> Result<String> {
            let clauses = items
                .iter()
                .map(|item| self.condition_clause(schema, item))
                .collect::<Result<Vec<_>>>()?;

            Ok(format!("({})", clauses.join(separator)))
        };

        match part {
            TraceCondition::All { and } => joined(and, " AND "),
            TraceCondition::Any { or } => joined(or, " OR "),
            TraceCondition::Text { text } => Ok(text_clause(schema, text)),
            TraceCondition::Compare(filter) => self.filter_clause(filter),
        }
    }

    /// What a filter of the trace list asks of a Span. A field of a Span is
    /// compared on the Span itself, and a Span without it is kept where the
    /// filter leaves a value out. Of the fields the list compares on the
    /// trace, `trace_id` and `duration_ns` are the Span's own; the span and
    /// error counts are of the trace the Span belongs to.
    fn filter_clause(&self, filter: &TraceFilter) -> Result<String> {
        if filter.scope == TraceFilterScope::Span || filter.field == "trace_id" {
            return compare(&quoted(&filter.field), filter);
        }

        match filter.field.as_str() {
            "duration_ns" => compare(&self.duration, filter),
            "span_count" => Ok(self.per_trace("COUNT(*)", filter)),
            "error_count" => {
                let failed = self.failed.as_deref().ok_or_else(|| {
                    Error::invalid("the trace stream keeps no status to count errors by")
                })?;

                Ok(self.per_trace(&format!("SUM({failed})"), filter))
            }
            other => Err(Error::invalid(format!(
                "unsupported trace filter field: {other}"
            ))),
        }
    }

    /// The Spans of the traces whose `count` meets `filter`.
    fn per_trace(&self, count: &str, filter: &TraceFilter) -> String {
        let condition = if filter.is_in() {
            format!("{count} IN ({})", filter.values.join(", "))
        } else {
            format!("{count} {} {}", filter.op, filter.value)
        };

        format!(
            "\"trace_id\" IN (SELECT \"trace_id\" FROM {} GROUP BY \"trace_id\" HAVING {condition})",
            self.table,
        )
    }

    /// `FROM` the stream `WHERE` every condition and the `extra` ones hold.
    pub(in super::super) fn reading_with(&self, extra: &[String]) -> String {
        let clauses = self
            .clauses
            .iter()
            .chain(extra)
            .map(String::as_str)
            .collect::<Vec<_>>();

        if clauses.is_empty() {
            format!("FROM {}", self.table)
        } else {
            format!("FROM {} WHERE {}", self.table, clauses.join(" AND "))
        }
    }

    /// How long a Span took, in whole nanoseconds, as SQL.
    pub(in super::super) fn duration(&self) -> &str {
        &self.duration
    }

    /// The stream, quoted, to read other Spans of it than the conditions keep.
    pub(in super::super) fn table(&self) -> &str {
        &self.table
    }

    fn reading(&self, extra: Option<&str>) -> String {
        let clauses = self
            .clauses
            .iter()
            .map(String::as_str)
            .chain(extra)
            .collect::<Vec<_>>();

        if clauses.is_empty() {
            format!("FROM {}", self.table)
        } else {
            format!("FROM {} WHERE {}", self.table, clauses.join(" AND "))
        }
    }

    /// How long a Span took, as a number a percentile can be taken of.
    fn duration_number(&self) -> String {
        format!("CAST({} AS DOUBLE)", self.duration)
    }

    /// The requests, the failures among them, and how long they took at the
    /// median and the 95th and 99th percentile. The percentiles are estimates:
    /// they are read in one pass, from a digest.
    fn figures(&self) -> Vec<String> {
        let mut columns = vec![format!("COUNT(*) AS {REQUESTS}")];
        columns.extend(self.failures());

        let duration = self.duration_number();

        columns.extend(
            [(0.5, P50), (0.95, P95), (0.99, P99)]
                .map(|(share, alias)| percentile(share, &duration, alias)),
        );

        columns
    }

    fn failures(&self) -> Option<String> {
        self.failed
            .as_ref()
            .map(|failed| format!("SUM({failed}) AS {ERRORS}"))
    }
}

/// A field the requests are grouped by, and its value as the groups name it.
pub(super) struct Grouping {
    name: String,
    /// The value as text: `NULL` where a Span has none, which an empty text is too.
    text: String,
}

impl Grouping {
    /// `None` for a field whose values do not group requests: a moment or a
    /// JSON document is all but different for every Span.
    pub(super) fn new(field: &FieldDef) -> Option<Self> {
        let name = quoted(&field.name);

        let text = match field.data_type {
            FieldType::Utf8 => format!("NULLIF({name}, '')"),
            FieldType::Int64 | FieldType::Float64 | FieldType::Bool => {
                format!("CAST({name} AS VARCHAR)")
            }
            FieldType::Timestamp | FieldType::Json => return None,
        };

        Some(Self { name, text })
    }
}

/// One row: the figures of the whole range.
pub(super) fn totals(source: &Source) -> String {
    format!(
        "SELECT {} {}",
        source.figures().join(", "),
        source.reading(None)
    )
}

/// A row for each bar of `plan` that holds a request.
pub(super) fn series(source: &Source, plan: &BucketPlan) -> String {
    format!(
        "SELECT {} AS {BUCKET}, {} {} GROUP BY 1",
        plan.bucket_sql(MOMENT),
        source.figures().join(", "),
        source.reading(None),
    )
}

/// A row for each bar of [`latency_edges`] that holds a request: how many
/// took that long and how many of them failed. The bar is told by whole
/// numbers compared, so a duration on an edge always falls in the bar it
/// begins: first the power of ten, then the step within it.
pub(super) fn latency(source: &Source) -> String {
    let edges = latency_edges();
    let steps = PREFERRED.len();
    let mut bar = String::from("CASE");

    for first in (0..edges.len() - 1).step_by(steps) {
        let mut within = String::from("CASE");

        for step in 1..steps {
            within.push_str(&format!(
                " WHEN {} < {} THEN {}",
                source.duration,
                edges[first + step],
                first + step - 1
            ));
        }

        within.push_str(&format!(" ELSE {} END", first + steps - 1));

        bar.push_str(&format!(
            " WHEN {} < {} THEN {within}",
            source.duration,
            edges[first + steps]
        ));
    }

    bar.push_str(&format!(" ELSE {} END", edges.len() - 1));

    let mut columns = vec![format!("{bar} AS {BAR}"), format!("COUNT(*) AS {REQUESTS}")];
    columns.extend(source.failures());

    format!(
        "SELECT {} {} GROUP BY 1",
        columns.join(", "),
        source.reading(None)
    )
}

/// A row for each value of the field, the commonest first: the figures of the
/// requests that have it. `limit` rows at most.
///
/// The count of values is taken over the field itself, so it comes back masked
/// where the field is: the values are then not the reader's to list.
pub(super) fn groups(source: &Source, grouping: &Grouping, limit: usize) -> String {
    format!(
        "SELECT {} AS {VALUE}, COUNT({}) AS {PRESENT}, {} {} GROUP BY 1 \
         ORDER BY {REQUESTS} DESC, {VALUE} ASC LIMIT {limit}",
        grouping.text,
        grouping.name,
        source.figures().join(", "),
        source.reading(None),
    )
}

/// A row for each value of `values` and each bar of `plan` that holds a
/// request with it: how many there were, how many failed and how long the
/// slowest twentieth took. `None` among `values` is the requests without one.
pub(super) fn group_series(
    source: &Source,
    grouping: &Grouping,
    values: &[Option<String>],
    plan: &BucketPlan,
) -> String {
    let listed = values
        .iter()
        .flatten()
        .map(|value| literal(value))
        .collect::<Vec<_>>();

    let mut matches = Vec::new();

    if !listed.is_empty() {
        matches.push(format!("{} IN ({})", grouping.text, listed.join(", ")));
    }

    if values.iter().any(Option::is_none) {
        matches.push(format!("{} IS NULL", grouping.text));
    }

    let mut columns = vec![
        format!("{} AS {VALUE}", grouping.text),
        format!("{} AS {BUCKET}", plan.bucket_sql(MOMENT)),
        format!("COUNT(*) AS {REQUESTS}"),
    ];

    columns.extend(source.failures());
    columns.push(percentile(0.95, &source.duration_number(), P95));

    let listed_only = format!("({})", matches.join(" OR "));

    format!(
        "SELECT {} {} GROUP BY 1, 2",
        columns.join(", "),
        source.reading(Some(listed_only.as_str())),
    )
}

/// Where a service took a request in: a Span of the server or consumer kind,
/// or the root of a trace that is not itself a call out. A client or producer
/// Span without a parent is a call made outside any request, not one taken in.
/// `None` for a stream that tells neither kind nor parent: every Span is then
/// counted.
fn entry_clause(schema: &Schema) -> Option<String> {
    let taking = kind_in(schema, [2, 5], ["SERVER", "CONSUMER"]);
    let making = kind_in(schema, [3, 4], ["CLIENT", "PRODUCER"]);

    let root = declared(schema, "parent_span_id")
        .is_some()
        .then(|| "(\"parent_span_id\" IS NULL OR \"parent_span_id\" = '')".to_string());

    match (taking, making, root) {
        (Some(taking), Some(making), Some(root)) => Some(format!(
            "({taking} OR ({root} AND (\"kind\" IS NULL OR NOT {making})))"
        )),
        (Some(taking), _, None) => Some(taking),
        (None, _, Some(root)) => Some(root),
        _ => None,
    }
}

/// Whether a Span is of one of two kinds, as the stream keeps kinds: the
/// numbers OTLP gives them, or text, which may hold those numbers or the
/// kinds' names. `None` for a stream that keeps no kind.
fn kind_in(schema: &Schema, numbers: [i64; 2], names: [&str; 2]) -> Option<String> {
    match declared(schema, "kind").map(|field| field.data_type) {
        Some(FieldType::Int64) => Some(format!("\"kind\" IN ({}, {})", numbers[0], numbers[1])),
        Some(FieldType::Utf8) => {
            let listed = numbers
                .iter()
                .map(i64::to_string)
                .chain(
                    names
                        .iter()
                        .flat_map(|name| [name.to_string(), format!("SPAN_KIND_{name}")]),
                )
                .map(|text| literal(&text))
                .collect::<Vec<_>>();

            Some(format!("UPPER(\"kind\") IN ({})", listed.join(", ")))
        }
        _ => None,
    }
}

/// Free text, looked for in the fields the trace list looks for it in.
fn text_clause(schema: &Schema, text: &str) -> String {
    let pattern = literal(&format!("%{}%", text.to_ascii_lowercase()));

    let matches = TEXT_FIELDS
        .into_iter()
        .filter(|field| declared(schema, field).is_some())
        .map(|field| {
            format!(
                "LOWER(COALESCE(CAST({} AS VARCHAR), '')) LIKE {pattern}",
                quoted(field)
            )
        })
        .collect::<Vec<_>>();

    if matches.is_empty() {
        "FALSE".to_string()
    } else {
        format!("({})", matches.join(" OR "))
    }
}

/// A filter's comparison on `expression`, whose value the filter has already
/// checked against the field's type.
fn compare(expression: &str, filter: &TraceFilter) -> Result<String> {
    let expression = match filter.data_type {
        FieldType::Timestamp => format!("CAST({expression} AS BIGINT)"),
        _ => expression.to_string(),
    };

    if filter.op == "contains" {
        return Ok(format!(
            "CAST({expression} AS VARCHAR) LIKE {}",
            literal(&format!("%{}%", filter.value))
        ));
    }

    if filter.is_in() {
        let listed = filter
            .values
            .iter()
            .map(|value| typed(filter, value))
            .collect::<Result<Vec<_>>>()?;

        return Ok(format!("{expression} IN ({})", listed.join(", ")));
    }

    let value = typed(filter, &filter.value)?;

    // A Span without the field does not have the value left out. The comparison
    // goes in brackets: the parser reads all that follows `IS DISTINCT FROM` as
    // its operand, an `AND` or `OR` after it included.
    if filter.excludes() {
        return Ok(format!("({expression} IS DISTINCT FROM {value})"));
    }

    Ok(format!("{expression} {} {value}", filter.op))
}

/// A value of `filter`'s field, already checked against its type, as SQL.
fn typed(filter: &TraceFilter, value: &str) -> Result<String> {
    match filter.data_type {
        FieldType::Utf8 => Ok(literal(value)),
        FieldType::Int64 | FieldType::Float64 | FieldType::Timestamp => Ok(value.to_string()),
        FieldType::Bool => Ok(value.to_ascii_uppercase()),
        FieldType::Json => Err(Error::invalid(format!(
            "trace field `{}` requires a JSON path query in SQL mode",
            filter.field
        ))),
    }
}

fn percentile(share: f64, number: &str, alias: &str) -> String {
    format!("approx_percentile_cont({share}) WITHIN GROUP (ORDER BY {number}) AS {alias}")
}

fn declared<'a>(schema: &'a Schema, name: &str) -> Option<&'a FieldDef> {
    schema.fields.iter().find(|field| field.name == name)
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", escape_sql_ident(name))
}

fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
