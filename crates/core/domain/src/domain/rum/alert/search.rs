// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The figure of a rule made from the explorer: one number over the rows of one type that a query
//! matches, as the explorer asks for it. The query is kept as it was written, for people to read
//! and change, and as the filters and words it was read into, which are what is evaluated.

use serde::{Deserialize, Serialize};

use crate::{
    domain::{
        rum::event::{RumStream, RumUnit, field},
        stream::FieldType,
    },
    shared::{Error, Result},
};

/// As many filters and words as the explorer takes.
const MAX_FILTERS: usize = 32;
const MAX_TERMS: usize = 8;
const MAX_STATEMENT_BYTES: usize = 4096;
const MAX_FIELD_BYTES: usize = 256;
const MAX_VALUE_BYTES: usize = 4096;
const MAX_TERM_BYTES: usize = 512;

/// The comparisons a filter of the explorer makes.
const OPERATORS: [&str; 13] = [
    "=",
    "!=",
    ">",
    ">=",
    "<",
    "<=",
    "contains",
    "match",
    "match_text",
    "in",
    "not_in",
    "exists",
    "missing",
];

/// What can be taken of the rows: the measures of the explorer.
const FUNCTIONS: [&str; 11] = [
    "count",
    "count_distinct",
    "sum",
    "avg",
    "min",
    "max",
    "p50",
    "p75",
    "p90",
    "p95",
    "p99",
];

/// The JSON columns whose keys a query names as fields, as `feature_flags.<key>`; the custom
/// timings are numbers of nanoseconds, the others text.
const DOCUMENTS: [&str; 4] = [
    "feature_flags",
    "context",
    "view.custom_timings",
    "usr.extra",
];

/// One comparison of the query, as the explorer reads it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RumAlertFilter {
    pub field: String,
    pub op: String,
    #[serde(default)]
    pub value: String,
}

/// The number taken of the rows: a function, and the field it takes unless it counts rows.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RumAlertMeasure {
    #[serde(rename = "fn")]
    pub function: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RumAlertSearch {
    /// The rows, as the explorer names their type: `views`, `errors`.
    #[serde(rename = "type")]
    pub kind: String,
    /// The query as it was written.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub statement: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<RumAlertFilter>,
    /// Words looked for in the columns text is looked for in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub free_text: Vec<String>,
    pub measure: RumAlertMeasure,
}

/// What a field a query names holds: a column of the stream, or a key of one of its documents.
fn kind_of(stream: RumStream, name: &str) -> Option<(FieldType, RumUnit)> {
    if let Some(column) = field(stream, name) {
        return Some((column.data_type, column.unit));
    }

    let document = DOCUMENTS.iter().find(|document| {
        name.strip_prefix(**document)
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|key| !key.is_empty())
    })?;

    field(stream, document)?;

    Some(if *document == "view.custom_timings" {
        (FieldType::Int64, RumUnit::Nanoseconds)
    } else {
        (FieldType::Utf8, RumUnit::None)
    })
}

impl RumAlertSearch {
    /// The stream of the rows the query is of.
    pub fn stream(&self) -> Result<RumStream> {
        RumStream::from_collection(self.kind.trim())
            .ok_or_else(|| Error::invalid(format!("`{}` is not a type of RUM rows", self.kind)))
    }

    /// Whether the number adds up, a count or a sum: it is nought where nothing happened, and it
    /// needs no number of rows before it says anything.
    pub fn adds_up(&self) -> bool {
        self.counts() || self.measure.function == "sum"
    }

    /// Whether the number counts rows or values, whatever the field is in.
    fn counts(&self) -> bool {
        matches!(self.measure.function.as_str(), "count" | "count_distinct")
    }

    /// The unit the number is in, as rule pages say it: a duration in milliseconds.
    pub fn unit(&self) -> &'static str {
        if self.counts() {
            return "count";
        }

        let unit = self
            .stream()
            .ok()
            .zip(self.measure.field.as_deref())
            .and_then(|(stream, name)| kind_of(stream, name))
            .map_or(RumUnit::None, |(_, unit)| unit);

        match unit {
            RumUnit::Nanoseconds | RumUnit::Milliseconds => "milliseconds",
            RumUnit::Percent => "percent",
            RumUnit::Score => "score",
            RumUnit::Bytes => "bytes",
            RumUnit::Count => "count",
            RumUnit::None | RumUnit::PerSecond | RumUnit::Pixels => "none",
        }
    }

    /// Whether the number of a field is in nanoseconds, which a rule says in milliseconds.
    pub fn in_nanoseconds(&self) -> bool {
        !self.counts()
            && self
                .stream()
                .ok()
                .zip(self.measure.field.as_deref())
                .and_then(|(stream, name)| kind_of(stream, name))
                .is_some_and(|(_, unit)| unit == RumUnit::Nanoseconds)
    }

    pub fn validate(&self) -> Result<()> {
        let stream = self.stream()?;

        if self.statement.len() > MAX_STATEMENT_BYTES || self.statement.contains('\0') {
            return Err(Error::invalid(format!(
                "a RUM query is at most {MAX_STATEMENT_BYTES} bytes"
            )));
        }

        if self.filters.len() > MAX_FILTERS || self.free_text.len() > MAX_TERMS {
            return Err(Error::invalid(format!(
                "a RUM query has at most {MAX_FILTERS} filters and {MAX_TERMS} words"
            )));
        }

        let named = |name: &str| -> Result<(FieldType, RumUnit)> {
            let name = name.trim();

            if name.is_empty() || name.len() > MAX_FIELD_BYTES {
                return Err(Error::invalid("a field of a RUM query is invalid"));
            }

            kind_of(stream, name)
                .ok_or_else(|| Error::invalid(format!("`{name}` is not a field of {}", self.kind)))
        };

        for filter in &self.filters {
            named(&filter.field)?;

            if !OPERATORS.contains(&filter.op.trim()) {
                return Err(Error::invalid(format!(
                    "`{}` is not a comparison of a RUM query",
                    filter.op
                )));
            }

            if filter.value.len() > MAX_VALUE_BYTES || filter.value.contains('\0') {
                return Err(Error::invalid("a value of a RUM query is too long"));
            }
        }

        for term in &self.free_text {
            if term.trim().is_empty() || term.len() > MAX_TERM_BYTES || term.contains('\0') {
                return Err(Error::invalid("a word of a RUM query is invalid"));
            }
        }

        let function = self.measure.function.as_str();

        if !FUNCTIONS.contains(&function) {
            return Err(Error::invalid(format!(
                "`{function}` is not a measure of a RUM query"
            )));
        }

        match (function, self.measure.field.as_deref()) {
            ("count", None) => Ok(()),
            (_, None) => Err(Error::invalid(format!("{function} needs a field"))),
            ("count" | "count_distinct", Some(name)) => match named(name)? {
                (FieldType::Json, _) => Err(Error::invalid(format!(
                    "`{name}` has no values to tell apart"
                ))),
                _ => Ok(()),
            },
            (_, Some(name)) => {
                let (data_type, _) = named(name)?;

                // A key of a document may hold a number; a value that is not one counts as none.
                let key = field(stream, name.trim()).is_none();

                if key || matches!(data_type, FieldType::Int64 | FieldType::Float64) {
                    Ok(())
                } else {
                    Err(Error::invalid(format!("`{name}` is not a number")))
                }
            }
        }
    }

    /// What the rule watches, for the statement field of rule lists.
    pub fn describe(&self) -> String {
        let measure = match &self.measure.field {
            Some(field) => format!("{}({field})", self.measure.function),
            None => self.measure.function.clone(),
        };

        let statement = self.statement.trim();

        if statement.is_empty() {
            format!("{} {measure}", self.kind)
        } else {
            format!("{} {measure} {statement}", self.kind)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search(kind: &str, function: &str, field: Option<&str>) -> RumAlertSearch {
        RumAlertSearch {
            kind: kind.into(),
            statement: "view.url_path_group = '/checkout'".into(),
            filters: vec![RumAlertFilter {
                field: "view.url_path_group".into(),
                op: "=".into(),
                value: "/checkout".into(),
            }],
            free_text: Vec::new(),
            measure: RumAlertMeasure {
                function: function.into(),
                field: field.map(str::to_string),
            },
        }
    }

    #[test]
    fn a_query_names_fields_of_its_type_and_a_measure_the_explorer_has() {
        assert!(
            search("views", "p75", Some("view.loading_time"))
                .validate()
                .is_ok()
        );
        assert!(search("views", "count", None).validate().is_ok());
        assert!(
            search("views", "count_distinct", Some("session.id"))
                .validate()
                .is_ok()
        );
        assert!(
            search("views", "max", Some("view.custom_timings.hero"))
                .validate()
                .is_ok()
        );
        assert!(
            search("views", "count", Some("feature_flags.checkout"))
                .validate()
                .is_ok()
        );

        assert!(search("view", "count", None).validate().is_err());
        assert!(
            search("views", "median", Some("view.loading_time"))
                .validate()
                .is_err()
        );
        assert!(search("views", "p75", None).validate().is_err());
        assert!(
            search("views", "p75", Some("view.url_path"))
                .validate()
                .is_err()
        );
        assert!(
            search("views", "p75", Some("view.nope"))
                .validate()
                .is_err()
        );
        assert!(
            search("sessions", "count", None).validate().is_err(),
            "a session is on no one page"
        );

        let mut odd = search("views", "count", None);

        odd.filters[0].op = "~".into();
        assert!(odd.validate().is_err());

        let mut long = search("views", "count", None);

        long.free_text = vec!["x".into(); MAX_TERMS + 1];
        assert!(long.validate().is_err());
    }

    #[test]
    fn a_count_is_a_count_and_a_duration_is_in_milliseconds() {
        assert_eq!(search("views", "count", None).unit(), "count");
        assert_eq!(
            search("views", "count_distinct", Some("session.id")).unit(),
            "count"
        );
        assert_eq!(
            search("views", "p75", Some("view.loading_time")).unit(),
            "milliseconds"
        );
        assert!(search("views", "p75", Some("view.loading_time")).in_nanoseconds());
        assert_eq!(
            search("views", "avg", Some("view.performance.cls.score")).unit(),
            "score"
        );
        assert!(!search("views", "count", None).in_nanoseconds());
        assert!(search("views", "count", None).adds_up());
        assert!(search("views", "sum", Some("view.error.count")).adds_up());
        assert_eq!(
            search("views", "sum", Some("view.error.count")).unit(),
            "count"
        );
        assert!(!search("views", "p95", Some("view.loading_time")).adds_up());
    }

    #[test]
    fn describes_what_it_watches() {
        assert_eq!(
            search("views", "p75", Some("view.loading_time")).describe(),
            "views p75(view.loading_time) view.url_path_group = '/checkout'"
        );
    }
}
