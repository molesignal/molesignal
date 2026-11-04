// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Schema-aware trace filter contract shared by summary and raw-span scans,
//! and by the RED figures of the Spans a trace query matches.

use chrono::DateTime;
use serde::{Deserialize, Serialize};

pub(super) use self::condition::TraceCondition;
use crate::{
    domain::stream::{FieldType, Schema},
    shared::{Error, Result, trace::summary::*},
};

pub(super) mod condition;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TraceFilterScope {
    Summary,
    Span,
}

/// The most values one `in` filter lists.
pub(super) const MAX_IN_VALUES: usize = 64;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TraceFilter {
    pub(super) field: String,
    pub(super) op: String,
    /// The value compared with; empty for `in`, which compares with `values`.
    pub(super) value: String,
    /// The values `in` holds for, each checked against the field's type.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) values: Vec<String>,
    pub(super) data_type: FieldType,
    pub(super) scope: TraceFilterScope,
}

#[derive(Debug, Deserialize)]
struct TraceFilterInput {
    field: String,
    op: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    values: Vec<String>,
}

impl TraceFilter {
    pub(super) fn is_span_filter(&self) -> bool {
        self.scope == TraceFilterScope::Span
    }

    /// Whether the filter leaves its value out. On a field of a Span it holds
    /// for a trace none of whose Spans has the value, the opposite of `=`; a
    /// Span without the field does not have it.
    pub(super) fn excludes(&self) -> bool {
        self.op == "!="
    }

    /// Whether the filter holds for any of several values, listed in `values`.
    pub(super) fn is_in(&self) -> bool {
        self.op == "in"
    }

    /// Whether a text holds the filter. An `in` holds for a text among its values.
    pub(super) fn matches_text(&self, value: &str) -> bool {
        match self.op.as_str() {
            "=" => value == self.value,
            "!=" => value != self.value,
            "contains" => value.contains(&self.value),
            "in" => self.values.iter().any(|listed| listed == value),
            _ => false,
        }
    }

    /// Whether a whole number holds the filter.
    pub(super) fn matches_integer(&self, value: i64) -> bool {
        if self.is_in() {
            return self
                .values
                .iter()
                .any(|listed| listed.parse::<i64>().ok() == Some(value));
        }

        self.integer_value()
            .is_some_and(|right| compare(value.cmp(&right), &self.op))
    }

    /// Whether a number holds the filter.
    pub(super) fn matches_float(&self, value: f64) -> bool {
        if self.is_in() {
            return self
                .values
                .iter()
                .any(|listed| listed.parse::<f64>().ok() == Some(value));
        }

        self.float_value()
            .and_then(|right| value.partial_cmp(&right))
            .is_some_and(|ordering| compare(ordering, &self.op))
    }

    /// Whether a truth value holds the filter.
    pub(super) fn matches_bool(&self, value: bool) -> bool {
        match self.op.as_str() {
            "=" => self.bool_value() == Some(value),
            "!=" => self.bool_value().is_some_and(|right| right != value),
            "in" => self
                .values
                .iter()
                .any(|listed| listed.parse::<bool>().ok() == Some(value)),
            _ => false,
        }
    }

    pub(super) fn summary_column(&self) -> Option<&'static str> {
        match (self.scope, self.field.as_str()) {
            (TraceFilterScope::Summary, "trace_id") => Some("trace_id"),
            (TraceFilterScope::Summary, "duration_ns") => Some(TRACE_SUMMARY_DURATION_NS_FIELD),
            (TraceFilterScope::Summary, "span_count") => Some(TRACE_SUMMARY_SPAN_COUNT_FIELD),
            (TraceFilterScope::Summary, "error_count") => Some(TRACE_SUMMARY_ERROR_COUNT_FIELD),
            _ => None,
        }
    }

    pub(super) fn integer_value(&self) -> Option<i64> {
        self.value.parse().ok()
    }

    pub(super) fn float_value(&self) -> Option<f64> {
        self.value.parse().ok()
    }

    pub(super) fn bool_value(&self) -> Option<bool> {
        self.value.parse().ok()
    }
}

pub(super) fn parse(
    raw: Option<&str>,
    schema: &Schema,
    maximum: usize,
) -> Result<Vec<TraceFilter>> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(Vec::new());
    };

    if raw.len() > 32 * 1024 || raw.contains('\0') {
        return Err(Error::invalid("invalid trace filters"));
    }

    let parsed = serde_json::from_str::<Vec<TraceFilterInput>>(raw)
        .map_err(|error| Error::invalid(format!("invalid trace filters: {error}")))?;

    if parsed.len() > maximum {
        return Err(Error::invalid(format!(
            "trace filters exceed maximum of {maximum}"
        )));
    }

    parsed
        .into_iter()
        .map(|filter| normalize(filter, schema))
        .collect()
}

pub(super) fn validate(filters: &[TraceFilter], schema: &Schema) -> Result<()> {
    for filter in filters {
        let expected = resolve_field(&filter.field, schema)?;

        if expected.data_type != filter.data_type || expected.scope != filter.scope {
            return Err(Error::invalid(format!(
                "trace filter field changed since the cursor was issued: {}",
                filter.field
            )));
        }

        validate_operator(&filter.field, &filter.op, filter.data_type)?;

        if filter.is_in() {
            normalize_values(&filter.field, &filter.values, filter.data_type)?;
        } else {
            normalize_value(&filter.field, &filter.value, filter.data_type)?;
        }
    }

    Ok(())
}

/// Whether an ordering of a value against a filter's value meets its operator.
fn compare(ordering: std::cmp::Ordering, op: &str) -> bool {
    use std::cmp::Ordering::{Equal, Greater, Less};

    match op {
        "=" => ordering == Equal,
        "!=" => ordering != Equal,
        ">" => ordering == Greater,
        ">=" => ordering != Less,
        "<" => ordering == Less,
        "<=" => ordering != Greater,
        _ => false,
    }
}

fn normalize(input: TraceFilterInput, schema: &Schema) -> Result<TraceFilter> {
    let field = clean(&input.field, 128, "trace filter field must not be empty")?;
    let resolved = resolve_field(&field, schema)?;
    let op = clean(&input.op, 16, "trace filter operator must not be empty")?.to_ascii_lowercase();

    let op = match op.as_str() {
        "=" | "eq" => "=",
        "!=" | "ne" => "!=",
        "contains" | "like" => "contains",
        "in" => "in",
        ">" | ">=" | "<" | "<=" => op.as_str(),
        _ => {
            return Err(Error::invalid(format!(
                "unsupported trace filter operator: {op}"
            )));
        }
    }
    .to_string();

    validate_operator(&resolved.name, &op, resolved.data_type)?;

    if op == "in" {
        let values = normalize_values(&resolved.name, &input.values, resolved.data_type)?;

        return Ok(TraceFilter {
            field: resolved.name,
            op,
            value: String::new(),
            values,
            data_type: resolved.data_type,
            scope: resolved.scope,
        });
    }

    let raw_value = clean(&input.value, 512, "trace filter value must not be empty")?;
    let value = normalize_value(&resolved.name, &raw_value, resolved.data_type)?;

    Ok(TraceFilter {
        field: resolved.name,
        op,
        value,
        values: Vec::new(),
        data_type: resolved.data_type,
        scope: resolved.scope,
    })
}

/// The values of an `in`, each checked against the field's type, in the order
/// given and without repeats.
fn normalize_values(field: &str, values: &[String], data_type: FieldType) -> Result<Vec<String>> {
    if values.is_empty() || values.len() > MAX_IN_VALUES {
        return Err(Error::invalid(format!(
            "`in` on trace field `{field}` takes from 1 to {MAX_IN_VALUES} values"
        )));
    }

    let mut normalized = Vec::with_capacity(values.len());

    for value in values {
        let value = clean(value, 512, "trace filter value must not be empty")?;
        let value = normalize_value(field, &value, data_type)?;

        if !normalized.contains(&value) {
            normalized.push(value);
        }
    }

    Ok(normalized)
}

struct ResolvedField {
    name: String,
    data_type: FieldType,
    scope: TraceFilterScope,
}

/// The field a filter names, as the trace summary or the stream names it: no
/// other name stands for a field.
fn resolve_field(name: &str, schema: &Schema) -> Result<ResolvedField> {
    let summary = match name {
        "trace_id" => Some(FieldType::Utf8),
        "duration_ns" | "span_count" | "error_count" => Some(FieldType::Int64),
        _ => None,
    };

    if let Some(data_type) = summary {
        return Ok(ResolvedField {
            name: name.to_string(),
            data_type,
            scope: TraceFilterScope::Summary,
        });
    }

    if name.starts_with("molesignal.trace.") {
        return Err(Error::invalid(format!(
            "internal trace summary field is not directly queryable: {name}"
        )));
    }

    let field = schema
        .fields
        .iter()
        .find(|field| field.name == name)
        .ok_or_else(|| Error::invalid(format!("unsupported trace filter field: {name}")))?;

    if field.data_type == FieldType::Json {
        return Err(Error::invalid(format!(
            "trace field `{name}` requires a JSON path query in SQL mode"
        )));
    }

    Ok(ResolvedField {
        name: name.to_string(),
        data_type: field.data_type,
        scope: TraceFilterScope::Span,
    })
}

fn validate_operator(field: &str, op: &str, data_type: FieldType) -> Result<()> {
    let supported = match data_type {
        FieldType::Utf8 => matches!(op, "=" | "!=" | "contains" | "in"),
        FieldType::Int64 | FieldType::Float64 | FieldType::Timestamp => {
            matches!(op, "=" | "!=" | ">" | ">=" | "<" | "<=" | "in")
        }
        FieldType::Bool => matches!(op, "=" | "!=" | "in"),
        FieldType::Json => false,
    };

    if supported {
        return Ok(());
    }

    Err(Error::invalid(format!(
        "operator `{op}` is not supported for trace field `{field}`"
    )))
}

fn normalize_value(field: &str, value: &str, data_type: FieldType) -> Result<String> {
    match data_type {
        FieldType::Utf8 => Ok(value.to_string()),
        FieldType::Int64 => value
            .parse::<i64>()
            .map(|value| value.to_string())
            .map_err(|_| Error::invalid(format!("trace field `{field}` requires an integer"))),
        FieldType::Float64 => value
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(|value| value.to_string())
            .ok_or_else(|| {
                Error::invalid(format!("trace field `{field}` requires a finite number"))
            }),
        FieldType::Bool => match value.to_ascii_lowercase().as_str() {
            "true" => Ok("true".to_string()),
            "false" => Ok("false".to_string()),
            _ => Err(Error::invalid(format!(
                "trace field `{field}` requires true or false"
            ))),
        },
        FieldType::Timestamp => value
            .parse::<i64>()
            .ok()
            .or_else(|| {
                DateTime::parse_from_rfc3339(value)
                    .ok()
                    .map(|value| value.timestamp_micros())
            })
            .map(|value| value.to_string())
            .ok_or_else(|| {
                Error::invalid(format!(
                    "trace field `{field}` requires epoch microseconds or an RFC 3339 timestamp"
                ))
            }),
        FieldType::Json => Err(Error::invalid(format!(
            "trace field `{field}` requires a JSON path query in SQL mode"
        ))),
    }
}

fn clean(value: &str, maximum: usize, message: &str) -> Result<String> {
    let value = value.trim();

    if value.is_empty() || value.len() > maximum || value.contains('\0') {
        return Err(Error::invalid(message));
    }

    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::stream::FieldDef;

    fn schema() -> Schema {
        let field = |name: &str, data_type| FieldDef {
            name: name.into(),
            data_type,
            nullable: true,
            index_type: None,
            indexed: false,
            encrypted: false,
            exact: false,
        };
        Schema {
            fields: vec![
                field("trace_id", FieldType::Utf8),
                field("service.name", FieldType::Utf8),
                field("http.status_code", FieldType::Int64),
                field("conflict", FieldType::Bool),
                field("attributes", FieldType::Json),
            ],
        }
    }

    #[test]
    fn normalizes_summary_and_any_span_fields_with_typed_operators() {
        let raw = serde_json::json!([
            {"field": "span_count", "op": ">=", "value": "3"},
            {"field": "service.name", "op": "contains", "value": "api"},
            {"field": "http.status_code", "op": ">", "value": "499"},
            {"field": "conflict", "op": "=", "value": "true"}
        ])
        .to_string();
        let filters = parse(Some(&raw), &schema(), 32).unwrap();
        assert_eq!(filters[0].scope, TraceFilterScope::Summary);
        assert_eq!(filters[1].field, "service.name");
        assert_eq!(filters[1].scope, TraceFilterScope::Span);
        assert_eq!(filters[2].data_type, FieldType::Int64);
        assert_eq!(filters[3].data_type, FieldType::Bool);
    }

    #[test]
    fn rejects_wrong_operator_and_direct_json_comparison() {
        let wrong_operator =
            serde_json::json!([{"field": "span_count", "op": "contains", "value": "3"}])
                .to_string();
        assert!(parse(Some(&wrong_operator), &schema(), 32).is_err());

        let json =
            serde_json::json!([{"field": "attributes", "op": "=", "value": "{}"}]).to_string();
        assert!(parse(Some(&json), &schema(), 32).is_err());
    }

    #[test]
    fn leaving_a_value_out_is_one_operator_however_it_is_written() {
        let raw = serde_json::json!([
            {"field": "service.name", "op": "ne", "value": "checkout"},
            {"field": "http.status_code", "op": "!=", "value": "500"},
            {"field": "conflict", "op": "NE", "value": "TRUE"},
            {"field": "span_count", "op": "!=", "value": "1"},
            {"field": "service.name", "op": "=", "value": "checkout"}
        ])
        .to_string();

        let filters = parse(Some(&raw), &schema(), 32).unwrap();

        assert!(filters[..4].iter().all(TraceFilter::excludes));
        assert!(filters[..4].iter().all(|filter| filter.op == "!="));
        assert!(!filters[4].excludes());
        assert_eq!(filters[2].value, "true");
        assert_eq!(filters[3].scope, TraceFilterScope::Summary);
    }

    #[test]
    fn a_filter_names_its_field_as_it_is() {
        for name in [
            "service",
            "service_name",
            "operation",
            "operation_name",
            "status",
        ] {
            let raw = serde_json::json!([{"field": name, "op": "=", "value": "x"}]).to_string();
            let error = parse(Some(&raw), &schema(), 32).unwrap_err();

            assert!(
                error.to_string().contains("unsupported trace filter field"),
                "{name}"
            );
        }
    }
}
