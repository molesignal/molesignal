// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Which sessions an application keeps.
//!
//! An application keeps a share of its sessions. The share is the one of the first filter that
//! describes the session, or else the share left for the others (`session_sample_rate`). A filter
//! describes a session by facts that stay the same while it lasts, because every event of a
//! session has to meet the same fate: where it was run (environment, service, version, platform,
//! kind of session), and who ran it (country, browser, system, kind of device).
//!
//! Every row that is kept says the share it was kept with, so that a total can be worked out of
//! the sessions that are left.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result};

/// Most filters an application has.
pub const MAX_RETENTION_FILTERS: usize = 10;
/// Most conditions a filter has; a session has to meet all of them.
pub const MAX_RETENTION_CONDITIONS: usize = 5;
/// Most values a condition lists; a session has to have one of them.
pub const MAX_RETENTION_VALUES: usize = 20;

const MAX_NAME_LEN: usize = 64;
const MAX_VALUE_LEN: usize = 128;
const MAX_ID_LEN: usize = 32;

/// What a row says it was kept by when no filter described its session.
pub const DEFAULT_FILTER: &str = "default";
/// What a row says it was kept by when it is an error of a session that was not kept.
pub const ERRORS_FILTER: &str = "errors";

/// What a condition can look at: facts that are the same for every event of a session.
pub const RETENTION_FIELDS: [&str; 9] = [
    "env",
    "service",
    "version",
    "source",
    "session.type",
    "geo.country_iso_code",
    "browser.name",
    "os.name",
    "device.type",
];

/// How a condition compares a session with its values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionOp {
    /// The session has one of the values.
    Is,
    /// The session has none of them, or has no value at all.
    IsNot,
}

/// One thing a filter asks of a session. `*` in a value stands for any text; case does not matter.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RetentionCondition {
    pub field: String,
    pub op: RetentionOp,
    pub values: Vec<String>,
}

/// A share of the sessions that meet some conditions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RetentionFilter {
    /// What the rows kept by this filter say it was. Made from the name when a change leaves it
    /// out, and kept when the filter is renamed.
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// The share of the sessions it describes that is kept, in percent.
    pub rate: u32,
    /// What a session has to be to be described. A filter without any describes every session.
    #[serde(default)]
    pub conditions: Vec<RetentionCondition>,
}

/// The filters as they are stored: checked, with the text trimmed and every filter named by an id
/// of its own.
pub fn normalize_filters(filters: Vec<RetentionFilter>) -> Result<Vec<RetentionFilter>> {
    if filters.len() > MAX_RETENTION_FILTERS {
        return Err(Error::invalid(format!(
            "an application has at most {MAX_RETENTION_FILTERS} retention filters"
        )));
    }

    let mut checked = filters
        .into_iter()
        .map(check_filter)
        .collect::<Result<Vec<_>>>()?;

    let mut names = HashSet::new();

    for filter in &checked {
        if !names.insert(filter.name.to_lowercase()) {
            return Err(Error::invalid(format!(
                "two retention filters are called `{}`",
                filter.name
            )));
        }
    }

    // The ids that were given are kept; the others are made from the names around them.
    let mut taken = HashSet::new();

    for filter in &checked {
        if !filter.id.is_empty() && !taken.insert(filter.id.clone()) {
            return Err(Error::invalid(format!(
                "two retention filters have the id `{}`",
                filter.id
            )));
        }
    }

    for filter in &mut checked {
        if filter.id.is_empty() {
            filter.id = free_id(&filter.name, &taken);
            taken.insert(filter.id.clone());
        }
    }

    Ok(checked)
}

fn check_filter(mut filter: RetentionFilter) -> Result<RetentionFilter> {
    filter.name = filter.name.trim().to_string();

    if filter.name.is_empty() || filter.name.chars().count() > MAX_NAME_LEN {
        return Err(Error::invalid(format!(
            "a retention filter name has between 1 and {MAX_NAME_LEN} characters"
        )));
    }

    if filter.rate > 100 {
        return Err(Error::invalid(format!(
            "the rate of the retention filter `{}` must be between 0 and 100",
            filter.name
        )));
    }

    filter.id = filter.id.trim().to_ascii_lowercase();

    if !filter.id.is_empty() {
        let valid = filter.id.len() <= MAX_ID_LEN
            && filter
                .id
                .starts_with(|first: char| first.is_ascii_alphanumeric())
            && filter
                .id
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-');

        if !valid || is_reserved(&filter.id) {
            return Err(Error::invalid(format!(
                "`{}` is not an id a retention filter can have: use letters, digits and dashes, \
                 up to {MAX_ID_LEN} of them, and not `{DEFAULT_FILTER}` or `{ERRORS_FILTER}`",
                filter.id
            )));
        }
    }

    if filter.conditions.len() > MAX_RETENTION_CONDITIONS {
        return Err(Error::invalid(format!(
            "a retention filter has at most {MAX_RETENTION_CONDITIONS} conditions"
        )));
    }

    filter.conditions = filter
        .conditions
        .into_iter()
        .map(|condition| check_condition(&filter.name, condition))
        .collect::<Result<Vec<_>>>()?;

    Ok(filter)
}

fn check_condition(filter: &str, mut condition: RetentionCondition) -> Result<RetentionCondition> {
    condition.field = condition.field.trim().to_string();

    if !RETENTION_FIELDS.contains(&condition.field.as_str()) {
        return Err(Error::invalid(format!(
            "`{}` is not something a retention filter can look at: use {}",
            condition.field,
            RETENTION_FIELDS.join(", ")
        )));
    }

    let mut seen = HashSet::new();
    let mut values = Vec::new();

    for value in condition.values {
        let value = value.trim().to_string();

        if value.is_empty() {
            continue;
        }

        if value.chars().count() > MAX_VALUE_LEN {
            return Err(Error::invalid(format!(
                "a value of the retention filter `{filter}` is longer than {MAX_VALUE_LEN} \
                 characters"
            )));
        }

        if seen.insert(value.to_lowercase()) {
            values.push(value);
        }
    }

    if values.is_empty() || values.len() > MAX_RETENTION_VALUES {
        return Err(Error::invalid(format!(
            "a condition of the retention filter `{filter}` lists between 1 and \
             {MAX_RETENTION_VALUES} values"
        )));
    }

    condition.values = values;

    Ok(condition)
}

fn is_reserved(id: &str) -> bool {
    id == DEFAULT_FILTER || id == ERRORS_FILTER
}

/// An id made from a name: its letters and digits in lower case, joined by single dashes, and a
/// number after it when that is taken. A name with no such letters is called `filter`.
pub(super) fn free_id(name: &str, taken: &HashSet<String>) -> String {
    let mut slug = String::new();

    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }

    let slug = slug.trim_matches('-');
    // Room is left for the number.
    let slug = &slug[..slug.len().min(MAX_ID_LEN - 4)];
    let slug = slug.trim_end_matches('-');
    let slug = if slug.is_empty() { "filter" } else { slug };

    let candidate = |number: usize| {
        if number == 1 {
            slug.to_string()
        } else {
            format!("{slug}-{number}")
        }
    };

    (1..)
        .map(candidate)
        .find(|id| !is_reserved(id) && !taken.contains(id))
        .unwrap_or_else(|| slug.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(name: &str, rate: u32) -> RetentionFilter {
        RetentionFilter {
            id: String::new(),
            name: name.into(),
            rate,
            conditions: Vec::new(),
        }
    }

    fn is(field: &str, values: &[&str]) -> RetentionCondition {
        RetentionCondition {
            field: field.into(),
            op: RetentionOp::Is,
            values: values.iter().map(|value| value.to_string()).collect(),
        }
    }

    fn ids(filters: Vec<RetentionFilter>) -> Vec<String> {
        normalize_filters(filters)
            .unwrap()
            .into_iter()
            .map(|filter| filter.id)
            .collect()
    }

    #[test]
    fn a_filter_is_named_by_an_id_made_from_its_name() {
        assert_eq!(
            ids(vec![
                filter("Production", 100),
                filter("  Beta Testers (iOS) ", 50)
            ]),
            ["production", "beta-testers-ios"]
        );
    }

    #[test]
    fn names_with_no_letters_are_called_filters_and_numbered() {
        assert_eq!(
            ids(vec![
                filter("生产环境", 100),
                filter("测试环境", 10),
                filter("预发", 20)
            ]),
            ["filter", "filter-2", "filter-3"]
        );
    }

    #[test]
    fn an_id_that_was_given_is_kept_and_the_others_make_way_for_it() {
        let mut named = filter("Production", 100);
        named.id = "prod".into();

        let other = filter("Prod", 10);

        // `Prod` would be `prod`, which the other filter has.
        assert_eq!(ids(vec![other.clone(), named.clone()]), ["prod-2", "prod"]);

        // And a renamed filter keeps its id.
        named.name = "Everything in production".into();
        assert_eq!(ids(vec![named]), ["prod"]);
    }

    #[test]
    fn two_filters_cannot_share_a_name_or_an_id_and_the_reserved_ids_are_not_given_out() {
        assert!(normalize_filters(vec![filter("Prod", 1), filter("prod", 2)]).is_err());

        let mut one = filter("A", 1);
        let mut two = filter("B", 1);

        one.id = "x".into();
        two.id = "X".into();
        assert!(normalize_filters(vec![one, two]).is_err());

        for reserved in [DEFAULT_FILTER, ERRORS_FILTER] {
            let mut named = filter("Anything", 1);

            named.id = reserved.into();
            assert!(normalize_filters(vec![named]).is_err(), "{reserved}");
        }

        assert_eq!(
            ids(vec![filter("Default", 5), filter("Errors", 5)]),
            ["default-2", "errors-2"]
        );
    }

    #[test]
    fn ids_a_person_gives_are_letters_digits_and_dashes_only() {
        let long = "a".repeat(33);

        for bad in ["-x", "a b", "a_b", "é", long.as_str()] {
            let mut named = filter("Anything", 1);

            named.id = bad.into();
            assert!(normalize_filters(vec![named]).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_rate_is_a_percentage_and_a_name_has_some_text() {
        assert!(normalize_filters(vec![filter("A", 0)]).is_ok());
        assert!(normalize_filters(vec![filter("A", 100)]).is_ok());
        assert!(normalize_filters(vec![filter("A", 101)]).is_err());
        assert!(normalize_filters(vec![filter("   ", 10)]).is_err());
        assert!(normalize_filters(vec![filter(&"n".repeat(65), 10)]).is_err());
    }

    #[test]
    fn a_filter_looks_at_what_stays_the_same_in_a_session() {
        let mut with = filter("Prod", 100);

        with.conditions = vec![is(" env ", &[" prod ", "Prod", "", "production"])];

        let checked = normalize_filters(vec![with]).unwrap();

        assert_eq!(checked[0].conditions[0].field, "env");
        // Trimmed, and a value is listed once whatever its case.
        assert_eq!(checked[0].conditions[0].values, ["prod", "production"]);

        for field in ["usr.id", "view.url_path", "context", ""] {
            let mut bad = filter("Bad", 10);

            bad.conditions = vec![is(field, &["x"])];
            assert!(normalize_filters(vec![bad]).is_err(), "{field}");
        }
    }

    #[test]
    fn a_condition_lists_some_values_but_not_too_many() {
        let mut none = filter("None", 10);
        none.conditions = vec![is("env", &["", "  "])];
        assert!(normalize_filters(vec![none]).is_err());

        let many = (0..=MAX_RETENTION_VALUES)
            .map(|n| n.to_string())
            .collect::<Vec<_>>();
        let many = many.iter().map(String::as_str).collect::<Vec<_>>();

        let mut too_many = filter("Many", 10);
        too_many.conditions = vec![is("version", &many)];
        assert!(normalize_filters(vec![too_many]).is_err());

        let long_value = "v".repeat(129);
        let mut long = filter("Long", 10);
        long.conditions = vec![is("version", &[long_value.as_str()])];
        assert!(normalize_filters(vec![long]).is_err());
    }

    #[test]
    fn there_are_limits_to_the_filters_and_to_their_conditions() {
        let many = (0..=MAX_RETENTION_FILTERS)
            .map(|n| filter(&format!("Filter number {n}"), 10))
            .collect::<Vec<_>>();

        assert!(normalize_filters(many.clone()).is_err());
        assert!(normalize_filters(many[..MAX_RETENTION_FILTERS].to_vec()).is_ok());

        let mut crowded = filter("Crowded", 10);

        crowded.conditions = RETENTION_FIELDS
            .iter()
            .take(MAX_RETENTION_CONDITIONS + 1)
            .map(|field| is(field, &["x"]))
            .collect();
        assert!(normalize_filters(vec![crowded]).is_err());
    }

    #[test]
    fn a_filter_is_written_and_read_by_its_names() {
        let value = serde_json::json!({
            "name": "Production",
            "rate": 100,
            "conditions": [{ "field": "env", "op": "is_not", "values": ["dev"] }]
        });

        let read: RetentionFilter = serde_json::from_value(value).unwrap();

        assert_eq!(read.id, "");
        assert_eq!(read.conditions[0].op, RetentionOp::IsNot);
        assert!(
            serde_json::from_value::<RetentionFilter>(serde_json::json!({
                "name": "x", "rate": 1,
                "conditions": [{ "field": "env", "op": "maybe", "values": [] }]
            }))
            .is_err()
        );
    }
}
