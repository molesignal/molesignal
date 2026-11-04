// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a request can name in a stream: one of its columns, or a value inside a JSON column.
//!
//! The feature flags, the custom attributes and the custom timings of an event are not known ahead
//! of time, so they are kept in JSON columns. `feature_flags.checkout_v2` is the value of that key
//! in `feature_flags`: it can be filtered, grouped and measured by like a column, because the
//! statement reads it with `json_text`. Everything after the name of the column is the key, dots
//! and all, since an SDK takes a key as it is given.

use super::sql::{ident, literal};
use crate::{
    domain::{
        rum::event::{RumField, RumStream, field},
        stream::FieldType,
    },
    shared::{Error, Result},
};

/// The JSON columns whose keys can be asked for.
pub const MEMBER_COLUMNS: [&str; 4] = [
    "feature_flags",
    "context",
    "view.custom_timings",
    "usr.extra",
];

/// Longest key a request names.
const MAX_KEY_LEN: usize = 256;

/// One thing a request names in a stream.
#[derive(Debug, Clone)]
pub enum Column {
    Stored(&'static RumField),
    Member {
        parent: &'static RumField,
        key: String,
        name: String,
    },
}

impl Column {
    /// What a request names in `stream`; `None` when the stream has nothing of that name.
    pub fn resolve(stream: RumStream, name: &str) -> Result<Option<Self>> {
        if let Some(column) = field(stream, name) {
            return Ok(Some(Self::Stored(column)));
        }

        for parent in MEMBER_COLUMNS {
            let Some(key) = name
                .strip_prefix(parent)
                .and_then(|rest| rest.strip_prefix('.'))
            else {
                continue;
            };

            let Some(parent) = field(stream, parent) else {
                continue;
            };

            if key.is_empty() || key.len() > MAX_KEY_LEN || key.contains('\0') {
                return Err(Error::invalid(format!("`{name}` is not a key of a column")));
            }

            return Ok(Some(Self::Member {
                parent,
                key: key.to_string(),
                name: name.to_string(),
            }));
        }

        Ok(None)
    }

    /// What it is called by the request that named it.
    pub fn name(&self) -> &str {
        match self {
            Self::Stored(column) => column.name,
            Self::Member { name, .. } => name,
        }
    }

    /// The column of the stream a statement has to read for it.
    pub fn stored_name(&self) -> &'static str {
        match self {
            Self::Stored(column) | Self::Member { parent: column, .. } => column.name,
        }
    }

    /// What its values are. A key of a JSON column is text, but for the custom timings.
    pub fn data_type(&self) -> FieldType {
        match self {
            Self::Stored(column) => column.data_type,
            Self::Member { parent, .. } if parent.name == "view.custom_timings" => FieldType::Int64,
            Self::Member { .. } => FieldType::Utf8,
        }
    }

    /// The unit its numbers are in.
    pub fn unit(&self) -> &'static str {
        match self {
            Self::Stored(column) => column.unit.as_str(),
            Self::Member { parent, .. } if parent.name == "view.custom_timings" => "ns",
            Self::Member { .. } => "none",
        }
    }

    /// Whether a statement can measure it as a number. A key of a JSON column may hold one; a value
    /// that is not a number counts as no value.
    pub fn is_numeric(&self) -> bool {
        match self {
            Self::Stored(column) => {
                matches!(column.data_type, FieldType::Int64 | FieldType::Float64)
            }
            Self::Member { .. } => true,
        }
    }

    /// Its value as a statement reads it.
    pub fn value(&self) -> String {
        match self {
            Self::Stored(column) => ident(column.name),
            Self::Member { parent, key, .. } => {
                format!("json_text({}, {})", ident(parent.name), literal(key))
            }
        }
    }

    /// Its value as a number.
    pub fn number(&self) -> String {
        match self {
            Self::Stored(_) => self.value(),
            Self::Member { .. } => format!("TRY_CAST({} AS DOUBLE)", self.value()),
        }
    }

    /// Its value as text, empty text being no value.
    pub fn text(&self) -> String {
        match self {
            Self::Stored(column) => format!("NULLIF(CAST({} AS VARCHAR), '')", ident(column.name)),
            Self::Member { .. } => format!("NULLIF({}, '')", self.value()),
        }
    }
}

/// The name of a column, for the messages that say a request named something a stream lacks.
pub fn resolved(stream: RumStream, name: &str) -> Result<Column> {
    Column::resolve(stream, name)?
        .ok_or_else(|| Error::invalid(format!("`{name}` is not a field of {}", stream.name())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_of_the_stream_is_read_as_it_is() {
        let column = resolved(RumStream::Views, "env").unwrap();

        assert_eq!(column.name(), "env");
        assert_eq!(column.value(), "\"env\"");
        assert_eq!(column.text(), "NULLIF(CAST(\"env\" AS VARCHAR), '')");
        assert!(!column.is_numeric());
    }

    #[test]
    fn a_key_of_a_json_column_is_read_out_of_it() {
        let column = resolved(RumStream::Errors, "feature_flags.checkout.v2").unwrap();

        assert_eq!(column.name(), "feature_flags.checkout.v2");
        assert_eq!(column.stored_name(), "feature_flags");
        assert_eq!(
            column.value(),
            "json_text(\"feature_flags\", 'checkout.v2')"
        );
        assert_eq!(
            column.text(),
            "NULLIF(json_text(\"feature_flags\", 'checkout.v2'), '')"
        );
    }

    #[test]
    fn a_key_is_quoted() {
        let column = resolved(RumStream::Views, "context.it's").unwrap();

        assert_eq!(column.value(), "json_text(\"context\", 'it''s')");
    }

    #[test]
    fn a_custom_timing_is_a_number_of_nanoseconds() {
        let column = resolved(RumStream::Views, "view.custom_timings.hero").unwrap();

        assert_eq!(column.data_type(), FieldType::Int64);
        assert_eq!(column.unit(), "ns");
        assert_eq!(
            column.number(),
            "TRY_CAST(json_text(\"view.custom_timings\", 'hero') AS DOUBLE)"
        );
    }

    #[test]
    fn what_a_stream_lacks_is_refused() {
        assert!(resolved(RumStream::Views, "nope").is_err());
        assert!(resolved(RumStream::Sessions, "feature_flags.x").is_err());
        assert!(resolved(RumStream::Actions, "view.custom_timings.hero").is_err());
        assert!(resolved(RumStream::Views, "feature_flags.").is_err());
    }
}
