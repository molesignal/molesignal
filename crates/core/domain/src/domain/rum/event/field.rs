// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use crate::domain::stream::{FieldDef, FieldType, StreamIndexType};

/// Longest text a column keeps unless it says otherwise, in bytes.
pub const DEFAULT_TEXT_LEN: usize = 1024;

/// The unit a column is read in. Storage never converts: a duration is kept in the nanoseconds the
/// SDK sent, and the unit travels with the column so a page can format it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RumUnit {
    None,
    Nanoseconds,
    Milliseconds,
    Bytes,
    Count,
    Score,
    Percent,
    PerSecond,
    Pixels,
}

impl RumUnit {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Nanoseconds => "ns",
            Self::Milliseconds => "ms",
            Self::Bytes => "bytes",
            Self::Count => "count",
            Self::Score => "score",
            Self::Percent => "percent",
            Self::PerSecond => "per_second",
            Self::Pixels => "px",
        }
    }
}

/// One column of a RUM stream. The name is the flattened path of the field in the event
/// (`view.performance.lcp.timestamp`), so the stream is queried by the names the event is sent in.
#[derive(Debug, Clone, Copy)]
pub struct RumField {
    pub name: &'static str,
    pub data_type: FieldType,
    pub index: StreamIndexType,
    pub unit: RumUnit,
    /// Longest text kept, in bytes; longer text is cut at a character boundary.
    pub max_len: usize,
}

impl RumField {
    const fn of(name: &'static str, data_type: FieldType) -> Self {
        Self {
            name,
            data_type,
            index: StreamIndexType::None,
            unit: RumUnit::None,
            max_len: DEFAULT_TEXT_LEN,
        }
    }

    pub const fn text(name: &'static str) -> Self {
        Self::of(name, FieldType::Utf8)
    }

    pub const fn int(name: &'static str) -> Self {
        Self::of(name, FieldType::Int64)
    }

    pub const fn float(name: &'static str) -> Self {
        Self::of(name, FieldType::Float64)
    }

    pub const fn flag(name: &'static str) -> Self {
        Self::of(name, FieldType::Bool)
    }

    pub const fn json(name: &'static str) -> Self {
        Self::of(name, FieldType::Json)
    }

    /// A duration or a time relative to the start of a view, in nanoseconds.
    pub const fn ns(name: &'static str) -> Self {
        Self::of(name, FieldType::Int64).unit(RumUnit::Nanoseconds)
    }

    /// A counter that only grows.
    pub const fn count(name: &'static str) -> Self {
        Self::of(name, FieldType::Int64).unit(RumUnit::Count)
    }

    pub const fn exact(mut self) -> Self {
        self.index = StreamIndexType::Exact;
        self
    }

    pub const fn full_text(mut self) -> Self {
        self.index = StreamIndexType::FullText;
        self
    }

    /// A JSON column filtered by the keys in it: `feature_flags.checkout_v2 = 'on'`.
    pub const fn key_value(mut self) -> Self {
        self.index = StreamIndexType::KeyValue;
        self
    }

    pub const fn unit(mut self, unit: RumUnit) -> Self {
        self.unit = unit;
        self
    }

    pub const fn long(mut self, max_len: usize) -> Self {
        self.max_len = max_len;
        self
    }

    /// The segments of the path in the event.
    pub fn path(&self) -> std::str::Split<'static, char> {
        self.name.split('.')
    }

    /// The column that collects the fields an event carries under the parent of this one
    /// without a column of their own: `extra` at the top, `usr.extra` under `usr`.
    pub fn is_catch_all(&self) -> bool {
        self.name == "extra" || self.name.ends_with(".extra")
    }

    pub fn field_def(&self) -> FieldDef {
        FieldDef {
            name: self.name.to_string(),
            data_type: self.data_type,
            nullable: true,
            index_type: Some(self.index),
            indexed: self.index != StreamIndexType::None,
            encrypted: false,
            exact: self.index == StreamIndexType::Exact,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builders_compose_in_const_context() {
        const FIELD: RumField = RumField::ns("view.loading_time").long(64);

        assert_eq!(FIELD.data_type, FieldType::Int64);
        assert_eq!(FIELD.unit, RumUnit::Nanoseconds);
        assert_eq!(FIELD.max_len, 64);
        assert_eq!(FIELD.index, StreamIndexType::None);
    }

    #[test]
    fn exact_columns_become_exact_indexes() {
        let def = RumField::text("session.id").exact().field_def();

        assert!(def.indexed);
        assert!(def.exact);
        assert_eq!(def.effective_index_type(), StreamIndexType::Exact);
    }

    #[test]
    fn catch_all_columns_are_named_extra() {
        assert!(RumField::json("extra").is_catch_all());
        assert!(RumField::json("usr.extra").is_catch_all());
        assert!(!RumField::json("context").is_catch_all());
        assert!(!RumField::text("extra_info").is_catch_all());
    }
}
