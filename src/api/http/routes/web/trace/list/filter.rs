// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Schema-aware trace filter contract shared by summary and raw-span scans.

use chrono::DateTime;
use serde::{Deserialize, Serialize};

use crate::{
    domain::stream::{FieldType, Schema},
    shared::{Error, Result, trace::summary::*},
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TraceFilterScope {
    Summary,
    Span,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(super) struct TraceFilter {
    pub(super) field: String,
    pub(super) op: String,
    pub(super) value: String,
    pub(super) data_type: FieldType,
    pub(super) scope: TraceFilterScope,
}

#[derive(Debug, Deserialize)]
struct TraceFilterInput {
    field: String,
    op: String,
    value: String,
}

impl TraceFilter {
    pub(super) fn is_span_filter(&self) -> bool {
        self.scope == TraceFilterScope::Span
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
        normalize_value(&filter.field, &filter.value, filter.data_type)?;
    }
    Ok(())
}

fn normalize(input: TraceFilterInput, schema: &Schema) -> Result<TraceFilter> {
    let field = clean(&input.field, 128, "trace filter field must not be empty")?;
    let resolved = resolve_field(&field, schema)?;
    let op = clean(&input.op, 16, "trace filter operator must not be empty")?.to_ascii_lowercase();
    let op = match op.as_str() {
        "=" | "eq" => "=",
        "!=" | "ne" => "!=",
        "contains" | "like" => "contains",
        ">" | ">=" | "<" | "<=" => op.as_str(),
        _ => {
            return Err(Error::invalid(format!(
                "unsupported trace filter operator: {op}"
            )));
        }
    }
    .to_string();
    validate_operator(&resolved.name, &op, resolved.data_type)?;
    let raw_value = clean(&input.value, 512, "trace filter value must not be empty")?;
    let value = normalize_value(&resolved.name, &raw_value, resolved.data_type)?;
    Ok(TraceFilter {
        field: resolved.name,
        op,
        value,
        data_type: resolved.data_type,
        scope: resolved.scope,
    })
}

struct ResolvedField {
    name: String,
    data_type: FieldType,
    scope: TraceFilterScope,
}

fn resolve_field(name: &str, schema: &Schema) -> Result<ResolvedField> {
    let canonical = match name {
        "service" | "service_name" => "service.name",
        "operation" | "operation_name" => "name",
        "status" => "status_code",
        other => other,
    };
    let summary = match canonical {
        "trace_id" => Some(FieldType::Utf8),
        "duration_ns" | "span_count" | "error_count" => Some(FieldType::Int64),
        _ => None,
    };
    if let Some(data_type) = summary {
        return Ok(ResolvedField {
            name: canonical.to_string(),
            data_type,
            scope: TraceFilterScope::Summary,
        });
    }
    if canonical.starts_with("molesignal.trace.") {
        return Err(Error::invalid(format!(
            "internal trace summary field is not directly queryable: {canonical}"
        )));
    }
    let field = schema
        .fields
        .iter()
        .find(|field| field.name == canonical)
        .ok_or_else(|| Error::invalid(format!("unsupported trace filter field: {canonical}")))?;
    if field.data_type == FieldType::Json {
        return Err(Error::invalid(format!(
            "trace field `{canonical}` requires a JSON path query in SQL mode"
        )));
    }
    Ok(ResolvedField {
        name: canonical.to_string(),
        data_type: field.data_type,
        scope: TraceFilterScope::Span,
    })
}

fn validate_operator(field: &str, op: &str, data_type: FieldType) -> Result<()> {
    let supported = match data_type {
        FieldType::Utf8 => matches!(op, "=" | "!=" | "contains"),
        FieldType::Int64 | FieldType::Float64 | FieldType::Timestamp => {
            matches!(op, "=" | "!=" | ">" | ">=" | "<" | "<=")
        }
        FieldType::Bool => matches!(op, "=" | "!="),
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
            {"field": "service_name", "op": "contains", "value": "api"},
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
}
