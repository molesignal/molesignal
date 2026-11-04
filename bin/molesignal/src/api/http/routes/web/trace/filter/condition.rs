// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The condition tree a trace query says beside its list of filters and its
//! free text: comparisons and text joined by AND and OR.
//!
//! The list holds what every matching trace has to meet. The tree says the
//! alternatives, `{"or": [...]}`, and how they group, `{"and": [...]}`; a
//! comparison is written as in the list, text as `{"text": "..."}`. The list,
//! the free text and the tree all hold together.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{TraceFilter, TraceFilterInput, normalize};
use crate::{
    api::http::routes::web::trace::list::{MAX_TRACE_FILTERS, clean_input},
    domain::stream::Schema,
    shared::{Error, Result},
};

/// How deep groups nest in one tree.
const MAX_DEPTH: usize = 8;

/// How many pieces of text one tree looks for.
const MAX_TEXTS: usize = 8;

const SHAPE: &str =
    "a trace condition is a comparison, {\"text\": …}, {\"and\": […]} or {\"or\": […]}";

/// A condition of a trace query: a comparison, text to look for, or a group
/// of conditions that all hold or one of which holds.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub(in super::super) enum TraceCondition {
    All { and: Vec<TraceCondition> },
    Any { or: Vec<TraceCondition> },
    Text { text: String },
    Compare(TraceFilter),
}

impl TraceCondition {
    /// The whole query: each filter of the list, the free text and the tree,
    /// all of which hold. A query that asks nothing is an empty `All`.
    pub(in super::super) fn of(
        filters: &[TraceFilter],
        text: Option<&str>,
        condition: Option<&TraceCondition>,
    ) -> Self {
        let mut and = filters
            .iter()
            .cloned()
            .map(Self::Compare)
            .collect::<Vec<_>>();

        and.extend(text.map(|text| Self::Text {
            text: text.to_string(),
        }));

        and.extend(
            condition
                .map(|condition| condition.conjuncts().to_vec())
                .into_iter()
                .flatten(),
        );

        Self::All { and }
    }

    /// The parts that all hold: the items of a group that all hold, or the
    /// condition itself.
    pub(in super::super) fn conjuncts(&self) -> &[TraceCondition] {
        match self {
            Self::All { and } => and,
            other => std::slice::from_ref(other),
        }
    }

    /// Every comparison of the tree.
    pub(in super::super) fn filters(&self) -> Vec<&TraceFilter> {
        let mut filters = Vec::new();
        self.collect_filters(&mut filters);
        filters
    }

    fn collect_filters<'a>(&'a self, filters: &mut Vec<&'a TraceFilter>) {
        match self {
            Self::All { and: items } | Self::Any { or: items } => {
                items.iter().for_each(|item| item.collect_filters(filters));
            }
            Self::Text { .. } => {}
            Self::Compare(filter) => filters.push(filter),
        }
    }

    /// Whether the tree looks for text anywhere.
    pub(in super::super) fn has_text(&self) -> bool {
        match self {
            Self::All { and: items } | Self::Any { or: items } => items.iter().any(Self::has_text),
            Self::Text { .. } => true,
            Self::Compare(_) => false,
        }
    }

    /// Whether a comparison of the tree is on a field of a Span.
    pub(in super::super) fn has_span_filter(&self) -> bool {
        self.filters().into_iter().any(TraceFilter::is_span_filter)
    }

    /// Whether the condition holds, given what each comparison and each piece
    /// of text answers. An empty `All` holds.
    pub(in super::super) fn holds(
        &self,
        compare: &impl Fn(&TraceFilter) -> bool,
        text: &impl Fn(&str) -> bool,
    ) -> bool {
        match self {
            Self::All { and } => and.iter().all(|item| item.holds(compare, text)),
            Self::Any { or } => or.iter().any(|item| item.holds(compare, text)),
            Self::Text { text: value } => text(value),
            Self::Compare(filter) => compare(filter),
        }
    }
}

/// The condition tree of a request, as JSON text: each comparison checked
/// against the stream as a filter of the list is. `room` is how many
/// comparisons it may hold besides those of the list.
pub(in super::super) fn parse(
    raw: Option<&str>,
    schema: &Schema,
    room: usize,
) -> Result<Option<TraceCondition>> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    if raw.len() > 32 * 1024 || raw.contains('\0') {
        return Err(Error::invalid("invalid trace condition"));
    }

    let value = serde_json::from_str::<Value>(raw)
        .map_err(|error| Error::invalid(format!("invalid trace condition: {error}")))?;

    let mut reader = Reader {
        schema,
        filters: room,
        texts: MAX_TEXTS,
    };

    reader.condition(value, 1).map(Some)
}

/// Checks every comparison of a tree a cursor kept against the stream as it
/// is now, as the filters of the list are.
pub(in super::super) fn validate(condition: &TraceCondition, schema: &Schema) -> Result<()> {
    let filters = condition.filters().into_iter().cloned().collect::<Vec<_>>();

    super::validate(&filters, schema)
}

/// Reads a tree, counting down what it may still hold.
struct Reader<'a> {
    schema: &'a Schema,
    filters: usize,
    texts: usize,
}

impl Reader<'_> {
    fn condition(&mut self, value: Value, depth: usize) -> Result<TraceCondition> {
        if depth > MAX_DEPTH {
            return Err(Error::invalid(format!(
                "a trace condition nests at most {MAX_DEPTH} groups"
            )));
        }

        let Value::Object(object) = value else {
            return Err(Error::invalid(SHAPE));
        };

        if object.len() != 1 {
            return self.comparison(object);
        }

        let (key, inner) = object.into_iter().next().expect("one entry");

        match key.as_str() {
            "and" => Ok(TraceCondition::All {
                and: self.items(inner, depth)?,
            }),
            "or" => Ok(TraceCondition::Any {
                or: self.items(inner, depth)?,
            }),
            "text" => self.text(inner),
            _ => Err(Error::invalid(SHAPE)),
        }
    }

    fn items(&mut self, value: Value, depth: usize) -> Result<Vec<TraceCondition>> {
        let Value::Array(items) = value else {
            return Err(Error::invalid(SHAPE));
        };

        if items.is_empty() {
            return Err(Error::invalid(
                "an `and` or `or` of a trace condition holds at least one condition",
            ));
        }

        items
            .into_iter()
            .map(|item| self.condition(item, depth + 1))
            .collect()
    }

    fn text(&mut self, value: Value) -> Result<TraceCondition> {
        let Some(text) = value.as_str().and_then(clean_input) else {
            return Err(Error::invalid(
                "a trace condition looks for text that is not empty",
            ));
        };

        self.texts = self.texts.checked_sub(1).ok_or_else(|| {
            Error::invalid(format!(
                "a trace condition looks for at most {MAX_TEXTS} pieces of text"
            ))
        })?;

        Ok(TraceCondition::Text { text })
    }

    fn comparison(&mut self, object: Map<String, Value>) -> Result<TraceCondition> {
        let input = serde_json::from_value::<TraceFilterInput>(Value::Object(object))
            .map_err(|error| Error::invalid(format!("invalid trace condition: {error}")))?;

        self.filters = self.filters.checked_sub(1).ok_or_else(|| {
            Error::invalid(format!(
                "trace filters exceed maximum of {MAX_TRACE_FILTERS}"
            ))
        })?;

        normalize(input, self.schema).map(TraceCondition::Compare)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        domain::stream::{FieldDef, FieldType},
        shared::trace::normalization::canonical_trace_schema,
    };

    fn schema() -> Schema {
        let mut schema = canonical_trace_schema();

        schema.fields.push(FieldDef {
            name: "http.status_code".into(),
            data_type: FieldType::Int64,
            nullable: true,
            index_type: None,
            indexed: false,
            encrypted: false,
            exact: false,
        });

        schema
    }

    fn read(value: Value) -> Result<Option<TraceCondition>> {
        parse(Some(&value.to_string()), &schema(), 32)
    }

    #[test]
    fn reads_groups_text_and_comparisons_checked_as_filters_are() {
        let condition = read(json!({"or": [
            {"field": "service.name", "op": "eq", "value": "checkout"},
            {"and": [{"text": " timeout "}, {"field": "span_count", "op": ">", "value": "5"}]}
        ]}))
        .unwrap()
        .unwrap();

        let TraceCondition::Any { or } = &condition else {
            panic!("an or: {condition:?}");
        };

        let TraceCondition::Compare(service) = &or[0] else {
            panic!("a comparison: {:?}", or[0]);
        };

        assert_eq!(service.op, "=");
        assert!(service.is_span_filter());
        assert!(condition.has_text());
        assert!(condition.has_span_filter());
        assert_eq!(condition.filters().len(), 2);

        let TraceCondition::All { and } = &or[1] else {
            panic!("an and: {:?}", or[1]);
        };

        assert_eq!(
            and[0],
            TraceCondition::Text {
                text: "timeout".into()
            }
        );
    }

    #[test]
    fn a_cursor_reads_back_the_tree_it_kept() {
        let condition = read(json!({"or": [
            {"field": "service.name", "op": "in", "values": ["a", "b"]},
            {"text": "x"}
        ]}))
        .unwrap()
        .unwrap();

        let kept = serde_json::to_string(&condition).unwrap();
        let back = serde_json::from_str::<TraceCondition>(&kept).unwrap();

        assert_eq!(back, condition);
        validate(&back, &schema()).unwrap();
    }

    #[test]
    fn refuses_what_is_no_condition() {
        for refused in [
            json!([]),
            json!({"or": []}),
            json!({"and": {"text": "x"}}),
            json!({"text": "  "}),
            json!({"not": [{"text": "x"}]}),
            json!({"field": "service_name", "op": "=", "value": "x"}),
            json!({"field": "span_count", "op": "contains", "value": "3"}),
        ] {
            assert!(read(refused.clone()).is_err(), "{refused}");
        }

        let mut nested = json!({"text": "deep"});

        for _ in 0..MAX_DEPTH {
            nested = json!({"or": [nested]});
        }

        assert!(read(nested).is_err());
        assert_eq!(parse(Some("  "), &schema(), 32).unwrap(), None);
    }

    #[test]
    fn the_list_and_the_tree_share_one_limit() {
        let three = json!({"or": [
            {"field": "service.name", "op": "=", "value": "a"},
            {"field": "service.name", "op": "=", "value": "b"},
            {"field": "service.name", "op": "=", "value": "c"}
        ]});

        assert!(parse(Some(&three.to_string()), &schema(), 3).is_ok());
        assert!(parse(Some(&three.to_string()), &schema(), 2).is_err());
    }

    #[test]
    fn the_whole_query_holds_the_list_the_text_and_the_tree() {
        let filters = super::super::parse(
            Some(r#"[{"field":"span_count","op":">=","value":"2"}]"#),
            &schema(),
            32,
        )
        .unwrap();
        let tree = read(json!({"and": [
            {"field": "service.name", "op": "=", "value": "a"},
            {"or": [{"text": "x"}, {"text": "y"}]}
        ]}))
        .unwrap();

        let whole = TraceCondition::of(&filters, Some("free"), tree.as_ref());

        // The tree's own AND is the query's: its parts join the list's.
        assert_eq!(whole.conjuncts().len(), 4);
        assert!(matches!(whole.conjuncts()[1], TraceCondition::Text { .. }));
        assert!(matches!(whole.conjuncts()[3], TraceCondition::Any { .. }));

        let answers = |service: &'static str, text: &'static str| {
            whole.holds(
                &|filter: &TraceFilter| filter.field == "span_count" || filter.value == service,
                &|value: &str| value == "free" || value == text,
            )
        };

        assert!(answers("a", "y"));
        assert!(!answers("b", "y"));
        assert!(!answers("a", "z"));
        assert!(TraceCondition::of(&[], None, None).holds(&|_| false, &|_| false));
    }
}
