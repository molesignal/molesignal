// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Pieces of the SQL statements the app writes itself over a stream: names
//! and values quoted for their place, the first of several fields a stream
//! has, a field's value as text (a key of a JSON field included), and counts
//! as a query answers them.

use domain::stream::{FieldDef, FieldType, Schema, StreamDefinition};
use serde_json::Value;

/// An identifier for a double-quoted place in a statement.
pub fn ident(value: &str) -> String {
    value.replace('"', "\"\"")
}

/// A value for a single-quoted place in a statement.
pub fn literal(value: &str) -> String {
    value.replace('\'', "''")
}

/// The first of `wanted` that the stream's schema has.
pub fn first_field(stream: &StreamDefinition, wanted: &[&'static str]) -> Option<&'static str> {
    wanted.iter().copied().find(|wanted| {
        stream
            .schema
            .fields
            .iter()
            .any(|field| field.name == *wanted)
    })
}

/// What a request names in a stream that a statement can read as text: a field, or a key of a
/// JSON field written after its name, such as `payload.tenant`. Everything after the JSON field's
/// name is the key, dots and all; the longest JSON field that fits is the one meant.
#[derive(Debug, Clone)]
pub enum Named<'a> {
    Field(&'a FieldDef),
    Member { parent: &'a FieldDef, key: String },
}

impl<'a> Named<'a> {
    /// `None` when the schema has nothing of that name.
    pub fn resolve(schema: &'a Schema, name: &str) -> Option<Self> {
        if let Some(field) = schema.fields.iter().find(|field| field.name == name) {
            return Some(Self::Field(field));
        }

        schema
            .fields
            .iter()
            .filter(|field| field.data_type == FieldType::Json)
            .filter_map(|parent| {
                let key = name
                    .strip_prefix(parent.name.as_str())?
                    .strip_prefix('.')
                    .filter(|key| !key.is_empty() && !key.contains('\0'))?;

                Some((parent, key))
            })
            .max_by_key(|(parent, _)| parent.name.len())
            .map(|(parent, key)| Self::Member {
                parent,
                key: key.to_string(),
            })
    }

    /// The field the statement reads.
    pub fn field(&self) -> &'a FieldDef {
        match self {
            Self::Field(field) | Self::Member { parent: field, .. } => field,
        }
    }

    /// Whether its values group rows: a moment, or a whole JSON document, is all but different
    /// for every row.
    pub fn groups(&self) -> bool {
        match self {
            Self::Field(field) => {
                !matches!(field.data_type, FieldType::Timestamp | FieldType::Json)
            }
            Self::Member { .. } => true,
        }
    }

    /// Its value as text, `NULL` where a row has none, which an empty text is too.
    pub fn text(&self) -> String {
        match self {
            Self::Field(field) => {
                format!("NULLIF(CAST(\"{}\" AS VARCHAR), '')", ident(&field.name))
            }
            Self::Member { parent, key } => format!(
                "NULLIF(json_text(\"{}\", '{}'), '')",
                ident(&parent.name),
                literal(key)
            ),
        }
    }
}

/// A count as a query answers it: a number, or a number written out.
pub fn count(value: &Value) -> u64 {
    value
        .as_u64()
        .or_else(|| value.as_f64().map(|value| value.max(0.0) as u64))
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn quotes_are_doubled_for_their_place() {
        assert_eq!(ident(r#"we"ird"#), r#"we""ird"#);
        assert_eq!(literal("it's"), "it''s");
    }

    fn field(name: &str, data_type: FieldType) -> FieldDef {
        FieldDef {
            name: name.into(),
            data_type,
            nullable: true,
            index_type: domain::stream::StreamIndexType::None,
            encrypted: false,
        }
    }

    #[test]
    fn a_key_of_a_json_field_is_read_out_of_the_longest_one_that_fits() {
        let schema = Schema {
            fields: vec![
                field("tenant.id", FieldType::Utf8),
                field("body", FieldType::Json),
                field("body.extra", FieldType::Json),
                field("_timestamp", FieldType::Timestamp),
            ],
        };

        let plain = Named::resolve(&schema, "tenant.id").unwrap();
        assert_eq!(plain.text(), "NULLIF(CAST(\"tenant.id\" AS VARCHAR), '')");
        assert!(plain.groups());

        let member = Named::resolve(&schema, "body.extra.plan").unwrap();
        assert_eq!(member.field().name, "body.extra");
        assert_eq!(
            member.text(),
            "NULLIF(json_text(\"body.extra\", 'plan'), '')"
        );

        let quoted = Named::resolve(&schema, "body.it's").unwrap();
        assert_eq!(quoted.text(), "NULLIF(json_text(\"body\", 'it''s'), '')");

        assert!(!Named::resolve(&schema, "body").unwrap().groups());
        assert!(!Named::resolve(&schema, "_timestamp").unwrap().groups());
        assert!(Named::resolve(&schema, "body.").is_none());
        assert!(Named::resolve(&schema, "nothing").is_none());
    }

    #[test]
    fn counts_are_read_however_the_query_writes_them() {
        assert_eq!(count(&json!(12)), 12);
        assert_eq!(count(&json!(12.0)), 12);
        assert_eq!(count(&json!("12")), 12);
        assert_eq!(count(&json!(null)), 0);
    }
}
