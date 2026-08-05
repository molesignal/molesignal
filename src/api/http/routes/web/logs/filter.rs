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
                ">" | ">=" | "<" | "<=" => op.as_str(),
                _ => return Err(Error::invalid("unsupported log filter operator")),
            }
            .to_string();
            let value = clean_required(Some(filter.value), "empty log filter value", 512)?;
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
        typed_value(filter, field_type)?;
    }
    Ok(())
}

pub(super) fn to_sql(filter: &LogFilter, fields: &[FieldDef]) -> Result<String> {
    let field_type = field_type(&filter.field, fields)
        .ok_or_else(|| Error::invalid(format!("unknown log filter field: {}", filter.field)))?;
    validate_operator(filter, field_type)?;
    let field = format!("\"{}\"", escape_sql_ident(&filter.field));
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
    let supported = match field_type {
        FieldType::Utf8 => matches!(filter.op.as_str(), "=" | "!=" | "contains"),
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
}
