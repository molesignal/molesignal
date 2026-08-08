// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Schema-aware validation and SQL rendering for structured log filters.

use chrono::DateTime;

use super::{LogFilter, clean_required, sql_literal};
use crate::{
    domain::{
        ingestion::EVENT_ID_FIELD,
        stream::{FieldDef, FieldType},
    },
    infra::query::escape_sql_ident,
    shared::{Error, Result},
};

pub(super) fn normalize(filters: Vec<LogFilter>, maximum: usize) -> Result<Vec<LogFilter>> {
    if filters.len() > maximum {
        return Err(Error::invalid("too many log filters"));
    }
    filters
        .into_iter()
        .map(|filter| {
            let field = clean_required(Some(filter.field), "empty log filter field", 128)?;
            let op = clean_required(Some(filter.op), "empty log filter operator", 16)?
                .to_ascii_lowercase();
            let op = match op.as_str() {
                "=" | "eq" => "=",
                "!=" | "ne" => "!=",
                "contains" | "like" => "contains",
                "match" => "match",
                "match_text" => "match_text",
                ">" | ">=" | "<" | "<=" => op.as_str(),
                _ => return Err(Error::invalid("unsupported log filter operator")),
            }
            .to_string();
            let value = if matches!(op.as_str(), "match" | "match_text") {
                let value = filter.value;
                if value.len() > 512 || value.contains('\0') {
                    return Err(Error::invalid("invalid log search value"));
                }
                value
            } else {
                clean_required(Some(filter.value), "empty log filter value", 512)?
            };
            Ok(LogFilter {
                field,
                op,
                value,
                quoted: filter.quoted,
            })
        })
        .collect()
}

pub(super) fn validate(filters: &[LogFilter], fields: &[FieldDef]) -> Result<()> {
    for filter in filters {
        let field_type = field_type(&filter.field, fields)
            .ok_or_else(|| Error::invalid(format!("unknown log filter field: {}", filter.field)))?;
        validate_operator(filter, field_type)?;
        validate_search_function(filter, fields)?;
        if !matches!(filter.op.as_str(), "match" | "match_text") {
            typed_value(filter, field_type)?;
        }
    }
    Ok(())
}

pub(super) fn to_sql(filter: &LogFilter, fields: &[FieldDef]) -> Result<String> {
    let field_type = field_type(&filter.field, fields)
        .ok_or_else(|| Error::invalid(format!("unknown log filter field: {}", filter.field)))?;
    validate_operator(filter, field_type)?;
    validate_search_function(filter, fields)?;
    let field = format!("\"{}\"", escape_sql_ident(&filter.field));
    if filter.op == "match" {
        if filter.value.is_empty() {
            return Ok("FALSE".to_string());
        }
        let pattern = format!("%{}%", escape_like_literal_text(&filter.value));
        return Ok(format!(
            "CAST({field} AS VARCHAR) ILIKE {}",
            sql_literal(&pattern)
        ));
    }
    if filter.op == "match_text" {
        if filter.value.trim().is_empty() {
            return Ok("FALSE".to_string());
        }
        let function_field = match_text_identifier(&filter.field)?;
        return Ok(format!(
            "MATCH_TEXT({function_field}, {})",
            sql_literal(&filter.value)
        ));
    }
    let expression = match field_type {
        FieldType::Timestamp => format!("CAST({field} AS BIGINT)"),
        _ => field,
    };
    if filter.op == "contains" {
        return Ok(format!(
            "CAST({expression} AS VARCHAR) LIKE {}",
            sql_literal(&format!("%{}%", filter.value)),
        ));
    }
    Ok(format!(
        "{expression} {} {}",
        filter.op,
        typed_value(filter, field_type)?,
    ))
}

fn field_type(name: &str, fields: &[FieldDef]) -> Option<FieldType> {
    match name {
        "_timestamp" => Some(FieldType::Timestamp),
        EVENT_ID_FIELD => Some(FieldType::Utf8),
        _ => fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.data_type),
    }
}

fn validate_operator(filter: &LogFilter, field_type: FieldType) -> Result<()> {
    if filter.op == "match" {
        return Ok(());
    }
    let supported = match field_type {
        FieldType::Utf8 => matches!(filter.op.as_str(), "=" | "!=" | "contains" | "match_text"),
        FieldType::Int64 | FieldType::Float64 | FieldType::Timestamp => {
            matches!(filter.op.as_str(), "=" | "!=" | ">" | ">=" | "<" | "<=")
        }
        FieldType::Bool => matches!(filter.op.as_str(), "=" | "!="),
        FieldType::Json => false,
    };
    if supported {
        return Ok(());
    }
    if field_type == FieldType::Json {
        return Err(Error::invalid(format!(
            "log field `{}` requires a JSON path query in SQL mode",
            filter.field
        )));
    }
    Err(Error::invalid(format!(
        "operator `{}` is not supported for log field `{}`",
        filter.op, filter.field
    )))
}

fn validate_search_function(filter: &LogFilter, fields: &[FieldDef]) -> Result<()> {
    if filter.op != "match_text" {
        return Ok(());
    }
    let configured = fields.iter().any(|field| {
        field.name == filter.field
            && field.data_type == FieldType::Utf8
            && field.indexed
            && !field.exact
    });
    if !configured {
        return Err(Error::invalid(format!(
            "MATCH_TEXT: field `{}` has no full-text index configured; configure `index_type = \
             full_text` on a string field to enable full-text search",
            filter.field
        )));
    }
    match_text_identifier(&filter.field)?;
    Ok(())
}

fn match_text_identifier(field: &str) -> Result<&str> {
    let mut chars = field.chars();
    let valid_start = chars
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_');
    if valid_start && chars.all(|character| character.is_ascii_alphanumeric() || character == '_') {
        return Ok(field);
    }
    Err(Error::invalid(format!(
        "MATCH_TEXT does not support the log field name `{field}` in Fields mode; use SQL mode"
    )))
}

fn escape_like_literal_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn typed_value(filter: &LogFilter, field_type: FieldType) -> Result<String> {
    match field_type {
        FieldType::Utf8 => Ok(sql_literal(&filter.value)),
        FieldType::Int64 => filter
            .value
            .parse::<i64>()
            .map(|value| value.to_string())
            .map_err(|_| {
                Error::invalid(format!(
                    "log field `{}` requires an integer value",
                    filter.field
                ))
            }),
        FieldType::Float64 => filter
            .value
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(|value| value.to_string())
            .ok_or_else(|| {
                Error::invalid(format!(
                    "log field `{}` requires a finite number",
                    filter.field
                ))
            }),
        FieldType::Bool => match filter.value.to_ascii_lowercase().as_str() {
            "true" => Ok("TRUE".to_string()),
            "false" => Ok("FALSE".to_string()),
            _ => Err(Error::invalid(format!(
                "log field `{}` requires true or false",
                filter.field
            ))),
        },
        FieldType::Timestamp => timestamp_micros(&filter.value)
            .map(|value| value.to_string())
            .ok_or_else(|| {
                Error::invalid(format!(
                    "log field `{}` requires epoch microseconds or an RFC 3339 timestamp",
                    filter.field
                ))
            }),
        FieldType::Json => Err(Error::invalid(format!(
            "log field `{}` requires a JSON path query in SQL mode",
            filter.field
        ))),
    }
}

fn timestamp_micros(value: &str) -> Option<i64> {
    value.parse::<i64>().ok().or_else(|| {
        DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|value| value.timestamp_micros())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, data_type: FieldType) -> FieldDef {
        FieldDef {
            name: name.into(),
            data_type,
            nullable: true,
            indexed: false,
            encrypted: false,
            exact: false,
        }
    }

    fn filter(field: &str, op: &str, value: &str) -> LogFilter {
        LogFilter {
            field: field.into(),
            op: op.into(),
            value: value.into(),
            quoted: false,
        }
    }

    #[test]
    fn renders_numeric_boolean_and_timestamp_filters_by_schema_type() {
        let fields = [
            field("status", FieldType::Int64),
            field("sampled", FieldType::Bool),
        ];
        assert_eq!(
            to_sql(&filter("status", ">=", "500"), &fields).unwrap(),
            "\"status\" >= 500"
        );
        assert_eq!(
            to_sql(&filter("sampled", "=", "true"), &fields).unwrap(),
            "\"sampled\" = TRUE"
        );
        assert_eq!(
            to_sql(&filter("_timestamp", ">=", "1970-01-01T00:00:01Z"), &fields,).unwrap(),
            "CAST(\"_timestamp\" AS BIGINT) >= 1000000"
        );
    }

    #[test]
    fn rejects_string_operators_on_numbers_and_direct_json_filters() {
        let fields = [
            field("status", FieldType::Int64),
            field("payload", FieldType::Json),
        ];
        assert!(validate(&[filter("status", "contains", "5")], &fields).is_err());
        assert!(validate(&[filter("payload", "=", "{}")], &fields).is_err());
    }

    #[test]
    fn renders_match_as_a_safe_case_insensitive_literal_substring() {
        let fields = [field("level", FieldType::Utf8)];
        assert_eq!(
            normalize(vec![filter("level", "match", "")], 1)
                .unwrap()
                .first()
                .map(|filter| filter.value.as_str()),
            Some("")
        );
        assert_eq!(
            to_sql(&filter("level", "match", "INFO"), &fields).unwrap(),
            "CAST(\"level\" AS VARCHAR) ILIKE '%INFO%'"
        );
        assert_eq!(
            to_sql(&filter("level", "match", "100%_"), &fields).unwrap(),
            "CAST(\"level\" AS VARCHAR) ILIKE '%100\\%\\_%'"
        );
        assert_eq!(
            to_sql(&filter("level", "match", ""), &fields).unwrap(),
            "FALSE"
        );
    }

    #[test]
    fn match_text_requires_a_full_text_indexed_string_field() {
        let mut message = field("message", FieldType::Utf8);
        assert!(
            to_sql(
                &filter("message", "match_text", "panic"),
                &[message.clone()]
            )
            .is_err()
        );

        message.indexed = true;
        assert_eq!(
            to_sql(&filter("message", "match_text", "panic"), &[message]).unwrap(),
            "MATCH_TEXT(message, 'panic')"
        );
    }
}
