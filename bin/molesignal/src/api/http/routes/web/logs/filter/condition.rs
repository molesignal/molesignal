// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The condition tree a log query says beside its lists: filters and words to
//! find, joined by AND and OR.
//!
//! Every filter and every word of the lists has to hold. The tree says the
//! alternatives, `{"or": [...]}`, and how they group, `{"and": [...]}`; a
//! filter is written as in the list, a word or phrase to find as
//! `{"text": "..."}`. The lists and the tree hold together.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::super::{LogFilter, clean_required, text_sql};
use crate::{
    domain::stream::FieldDef,
    shared::{Error, Result},
};

/// How deep groups nest in one tree.
const MAX_DEPTH: usize = 8;

const SHAPE: &str = "a log condition is a filter, {\"text\": …}, {\"and\": […]} or {\"or\": […]}";

/// A condition of a log query: a filter, a word or phrase to find, or a group
/// of conditions that all hold or one of which holds.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub(in super::super) enum LogCondition {
    All { and: Vec<LogCondition> },
    Any { or: Vec<LogCondition> },
    Text { text: String },
    Filter(LogFilter),
}

impl LogCondition {
    /// The parts that all hold: the items of a group that all hold, or the
    /// condition itself.
    pub(in super::super) fn conjuncts(&self) -> &[LogCondition] {
        match self {
            Self::All { and } => and,
            other => std::slice::from_ref(other),
        }
    }

    /// Every filter of the tree.
    pub(in super::super) fn filters(&self) -> Vec<&LogFilter> {
        let mut filters = Vec::new();
        self.collect_filters(&mut filters);
        filters
    }

    fn collect_filters<'a>(&'a self, filters: &mut Vec<&'a LogFilter>) {
        match self {
            Self::All { and: items } | Self::Any { or: items } => {
                items.iter().for_each(|item| item.collect_filters(filters));
            }
            Self::Text { .. } => {}
            Self::Filter(filter) => filters.push(filter),
        }
    }

    /// Whether the tree looks for a word anywhere.
    pub(in super::super) fn has_text(&self) -> bool {
        match self {
            Self::All { and: items } | Self::Any { or: items } => items.iter().any(Self::has_text),
            Self::Text { .. } => true,
            Self::Filter(_) => false,
        }
    }
}

/// The condition tree of a request, each filter in it as the list's filters
/// are read. `filters` and `texts` are how many filters and words it may hold
/// besides those of the lists.
pub(in super::super) fn read(
    value: Option<Value>,
    filters: usize,
    texts: usize,
) -> Result<Option<LogCondition>> {
    let Some(value) = value else {
        return Ok(None);
    };

    let mut reader = Reader { filters, texts };

    reader.condition(value, 1).map(Some)
}

/// Checks each filter of the tree against the stream's fields.
pub(in super::super) fn validate(condition: &LogCondition, fields: &[FieldDef]) -> Result<()> {
    let filters = condition.filters().into_iter().cloned().collect::<Vec<_>>();

    super::validate(&filters, fields)
}

/// The condition as SQL: each filter as a filter of the list is written, each
/// word as a word of the list, a group in brackets.
pub(in super::super) fn to_sql(condition: &LogCondition, fields: &[FieldDef]) -> Result<String> {
    let joined = |items: &[LogCondition], separator: &str| -> Result<String> {
        let parts = items
            .iter()
            .map(|item| to_sql(item, fields))
            .collect::<Result<Vec<_>>>()?;

        Ok(format!("({})", parts.join(separator)))
    };

    match condition {
        LogCondition::All { and } => joined(and, " AND "),
        LogCondition::Any { or } => joined(or, " OR "),
        LogCondition::Text { text } => Ok(text_sql(text)),
        LogCondition::Filter(filter) => super::to_sql(filter, fields),
    }
}

/// Reads a tree, counting down what it may still hold.
struct Reader {
    filters: usize,
    texts: usize,
}

impl Reader {
    fn condition(&mut self, value: Value, depth: usize) -> Result<LogCondition> {
        if depth > MAX_DEPTH {
            return Err(Error::invalid(format!(
                "a log condition nests at most {MAX_DEPTH} groups"
            )));
        }

        let Value::Object(object) = value else {
            return Err(Error::invalid(SHAPE));
        };

        if object.len() != 1 {
            return self.filter(object);
        }

        let (key, inner) = object.into_iter().next().expect("one entry");

        match key.as_str() {
            "and" => Ok(LogCondition::All {
                and: self.items(inner, depth)?,
            }),
            "or" => Ok(LogCondition::Any {
                or: self.items(inner, depth)?,
            }),
            "text" => self.text(inner),
            _ => Err(Error::invalid(SHAPE)),
        }
    }

    fn items(&mut self, value: Value, depth: usize) -> Result<Vec<LogCondition>> {
        let Value::Array(items) = value else {
            return Err(Error::invalid(SHAPE));
        };

        if items.is_empty() {
            return Err(Error::invalid(
                "an `and` or `or` of a log condition holds at least one condition",
            ));
        }

        items
            .into_iter()
            .map(|item| self.condition(item, depth + 1))
            .collect()
    }

    fn text(&mut self, value: Value) -> Result<LogCondition> {
        let Value::String(text) = value else {
            return Err(Error::invalid(SHAPE));
        };

        self.texts = self
            .texts
            .checked_sub(1)
            .ok_or_else(|| Error::invalid("too many log free-text clauses"))?;

        Ok(LogCondition::Text {
            text: clean_required(Some(text), "empty log search text", 512)?,
        })
    }

    fn filter(&mut self, object: Map<String, Value>) -> Result<LogCondition> {
        let filter = serde_json::from_value::<LogFilter>(Value::Object(object))
            .map_err(|error| Error::invalid(format!("invalid log condition: {error}")))?;

        self.filters = self
            .filters
            .checked_sub(1)
            .ok_or_else(|| Error::invalid("too many log filters"))?;

        super::normalize_filter(filter).map(LogCondition::Filter)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::stream::FieldType;

    fn field(name: &str, data_type: FieldType) -> FieldDef {
        FieldDef {
            name: name.into(),
            data_type,
            nullable: true,
            index_type: None,
            indexed: false,
            encrypted: false,
            exact: false,
        }
    }

    fn fields() -> Vec<FieldDef> {
        vec![
            field("level", FieldType::Utf8),
            field("service", FieldType::Utf8),
            field("status", FieldType::Int64),
            field("message", FieldType::Utf8),
        ]
    }

    fn condition(value: Value) -> LogCondition {
        read(Some(value), 32, 8)
            .expect("a condition")
            .expect("not empty")
    }

    #[test]
    fn writes_groups_in_brackets_and_each_part_as_the_lists_write_theirs() {
        let either = condition(json!({"and": [
            {"field": "level", "op": "eq", "value": "error", "quoted": true},
            {"or": [
                {"field": "service", "op": "=", "value": "checkout", "quoted": true},
                {"field": "status", "op": ">=", "value": "500"},
                {"text": "timeout"},
                {"text": "connection reset"}
            ]}
        ]}));

        validate(&either, &fields()).expect("every filter names a field of the stream");

        assert_eq!(
            to_sql(&either, &fields()).unwrap(),
            "(\"level\" = 'error' AND (\"service\" = 'checkout' OR \"status\" >= 500 OR \
             MATCH(message, 'timeout') OR \"message\" LIKE '%connection reset%'))"
        );
        assert_eq!(either.conjuncts().len(), 2);
        assert_eq!(either.filters().len(), 3);
        assert!(either.has_text());
    }

    #[test]
    fn a_cursor_reads_back_the_tree_it_kept() {
        let either = condition(json!({"or": [
            {"field": "level", "op": "=", "value": "", "quoted": true},
            {"text": "panic"}
        ]}));

        let kept = serde_json::to_string(&either).unwrap();

        assert_eq!(serde_json::from_str::<LogCondition>(&kept).unwrap(), either);
    }

    #[test]
    fn refuses_what_is_no_condition() {
        for refused in [
            json!("level = 'error'"),
            json!({"or": []}),
            json!({"and": {"text": "x"}}),
            json!({"text": " "}),
            json!({"text": 5}),
            json!({"not": [{"text": "x"}]}),
            json!({"field": "level", "op": "regex", "value": "e.*"}),
        ] {
            assert!(read(Some(refused.clone()), 32, 8).is_err(), "{refused}");
        }

        let mut nested = json!({"text": "deep"});

        for _ in 0..MAX_DEPTH {
            nested = json!({"or": [nested]});
        }

        assert!(read(Some(nested), 32, 8).is_err());
        assert_eq!(read(None, 32, 8).unwrap(), None);
    }

    #[test]
    fn a_filter_the_stream_lacks_is_refused_wherever_it_stands() {
        let either = condition(json!({"or": [
            {"field": "level", "op": "=", "value": "error"},
            {"field": "missing", "op": "=", "value": "x"}
        ]}));

        assert!(validate(&either, &fields()).is_err());
    }

    #[test]
    fn the_lists_and_the_tree_share_their_limits() {
        let two = json!({"or": [{"text": "a"}, {"text": "b"}]});

        assert!(read(Some(two.clone()), 32, 2).is_ok());
        assert!(read(Some(two), 32, 1).is_err());

        let filters = json!({"or": [
            {"field": "level", "op": "=", "value": "a"},
            {"field": "level", "op": "=", "value": "b"}
        ]});

        assert!(read(Some(filters), 1, 8).is_err());
    }
}
