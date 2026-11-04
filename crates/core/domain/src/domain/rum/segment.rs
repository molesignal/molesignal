// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A segment of an application: a name for the sessions of some people, such as the customers of one
//! plan or the visitors of one country, that the questions of the analytics pages can be narrowed to.
//!
//! A segment tells sessions by what stays the same while they last and what every row of them says,
//! so it narrows a funnel of views and actions as it does a retention of sessions.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::retention::{RetentionCondition, free_id};
use crate::shared::{Error, Result};

/// Most segments an application has.
pub const MAX_SEGMENTS: usize = 20;
/// Most conditions a segment has; a session has to meet all of them.
pub const MAX_SEGMENT_CONDITIONS: usize = 5;
/// Most values a condition lists; a session has to have one of them.
pub const MAX_SEGMENT_VALUES: usize = 20;

const MAX_NAME_LEN: usize = 64;
const MAX_VALUE_LEN: usize = 256;
const MAX_ID_LEN: usize = 32;
const MAX_KEY_LEN: usize = 128;

/// What a segment can look at: the columns every row of a session has, sessions included.
pub const SEGMENT_FIELDS: [&str; 12] = [
    "service",
    "version",
    "source",
    "usr.id",
    "usr.email",
    "account.id",
    "browser.name",
    "os.name",
    "device.type",
    "geo.country_iso_code",
    "geo.continent_code",
    "geo.city",
];

/// The properties of a person the application set, as a segment names one: `usr.extra.plan`.
pub const SEGMENT_USER_PREFIX: &str = "usr.extra.";

/// A named group of the sessions of an application.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RumSegment {
    /// What the address of a page names it by. Made from the name when a change leaves it out, and
    /// kept when the segment is renamed.
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub conditions: Vec<RetentionCondition>,
}

/// The segments as they are stored: checked, with the text trimmed and each named by an id.
pub fn normalize_segments(segments: Vec<RumSegment>) -> Result<Vec<RumSegment>> {
    if segments.len() > MAX_SEGMENTS {
        return Err(Error::invalid(format!(
            "an application has at most {MAX_SEGMENTS} segments"
        )));
    }

    let mut checked = segments
        .into_iter()
        .map(check_segment)
        .collect::<Result<Vec<_>>>()?;

    let mut names = HashSet::new();
    let mut taken = HashSet::new();

    for segment in &checked {
        if !names.insert(segment.name.to_lowercase()) {
            return Err(Error::invalid(format!(
                "two segments are called `{}`",
                segment.name
            )));
        }

        if !segment.id.is_empty() && !taken.insert(segment.id.clone()) {
            return Err(Error::invalid(format!(
                "two segments have the id `{}`",
                segment.id
            )));
        }
    }

    for segment in &mut checked {
        if segment.id.is_empty() {
            segment.id = free_id(&segment.name, &taken);
            taken.insert(segment.id.clone());
        }
    }

    Ok(checked)
}

fn check_segment(mut segment: RumSegment) -> Result<RumSegment> {
    segment.name = segment.name.trim().to_string();

    if segment.name.is_empty() || segment.name.chars().count() > MAX_NAME_LEN {
        return Err(Error::invalid(format!(
            "a segment name has between 1 and {MAX_NAME_LEN} characters"
        )));
    }

    segment.id = segment.id.trim().to_ascii_lowercase();

    let id_is_fine = segment.id.is_empty()
        || (segment.id.len() <= MAX_ID_LEN
            && segment
                .id
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-'));

    if !id_is_fine {
        return Err(Error::invalid(format!(
            "`{}` is not an id a segment can have: use letters, digits and dashes, up to \
             {MAX_ID_LEN} of them",
            segment.id
        )));
    }

    if segment.conditions.is_empty() || segment.conditions.len() > MAX_SEGMENT_CONDITIONS {
        return Err(Error::invalid(format!(
            "the segment `{}` has between 1 and {MAX_SEGMENT_CONDITIONS} conditions",
            segment.name
        )));
    }

    segment.conditions = segment
        .conditions
        .into_iter()
        .map(|condition| check_condition(&segment.name, condition))
        .collect::<Result<Vec<_>>>()?;

    Ok(segment)
}

fn check_condition(segment: &str, mut condition: RetentionCondition) -> Result<RetentionCondition> {
    condition.field = condition.field.trim().to_string();

    if !can_look_at(&condition.field) {
        return Err(Error::invalid(format!(
            "`{}` is not something a segment can look at: use {} or {SEGMENT_USER_PREFIX}<key>",
            condition.field,
            SEGMENT_FIELDS.join(", ")
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
                "a value of the segment `{segment}` is longer than {MAX_VALUE_LEN} characters"
            )));
        }

        if seen.insert(value.clone()) {
            values.push(value);
        }
    }

    if values.is_empty() || values.len() > MAX_SEGMENT_VALUES {
        return Err(Error::invalid(format!(
            "a condition of the segment `{segment}` lists between 1 and {MAX_SEGMENT_VALUES} values"
        )));
    }

    condition.values = values;

    Ok(condition)
}

fn can_look_at(field: &str) -> bool {
    SEGMENT_FIELDS.contains(&field)
        || field
            .strip_prefix(SEGMENT_USER_PREFIX)
            .is_some_and(|key| !key.is_empty() && key.len() <= MAX_KEY_LEN && !key.contains('\0'))
}

#[cfg(test)]
mod tests {
    use super::{super::retention::RetentionOp, *};

    fn on(field: &str, values: &[&str]) -> RetentionCondition {
        RetentionCondition {
            field: field.into(),
            op: RetentionOp::Is,
            values: values.iter().map(|value| (*value).to_string()).collect(),
        }
    }

    fn segment(name: &str, conditions: Vec<RetentionCondition>) -> RumSegment {
        RumSegment {
            id: String::new(),
            name: name.into(),
            conditions,
        }
    }

    #[test]
    fn a_segment_is_named_by_an_id_of_its_own_and_keeps_it_when_renamed() {
        let made = normalize_segments(vec![
            segment(
                " Enterprise plan ",
                vec![on("usr.extra.plan", &["enterprise"])],
            ),
            segment(
                "Germany",
                vec![on("geo.country_iso_code", &["DE", " DE ", ""])],
            ),
        ])
        .unwrap();

        assert_eq!(made[0].id, "enterprise-plan");
        assert_eq!(made[0].name, "Enterprise plan");
        assert_eq!(made[1].id, "germany");
        assert_eq!(made[1].conditions[0].values, ["DE"]);

        let renamed = normalize_segments(vec![RumSegment {
            name: "Big customers".into(),
            ..made[0].clone()
        }])
        .unwrap();

        assert_eq!(renamed[0].id, "enterprise-plan");
    }

    #[test]
    fn a_segment_only_looks_at_what_every_row_of_a_session_says() {
        for field in [
            "view.url_path",
            "context.tier",
            "usr.extra.",
            "feature_flags.x",
        ] {
            assert!(
                normalize_segments(vec![segment("x", vec![on(field, &["1"])])]).is_err(),
                "{field}"
            );
        }

        assert!(
            normalize_segments(vec![segment("x", vec![on("browser.name", &["Chrome"])])]).is_ok()
        );
    }

    #[test]
    fn a_segment_says_something_and_is_told_apart() {
        assert!(normalize_segments(vec![segment("x", Vec::new())]).is_err());
        assert!(normalize_segments(vec![segment("x", vec![on("usr.id", &[" "])])]).is_err());

        let twice = vec![
            segment("Same", vec![on("usr.id", &["1"])]),
            segment("same", vec![on("usr.id", &["2"])]),
        ];

        assert!(normalize_segments(twice).is_err());

        let many = (0..=MAX_SEGMENTS)
            .map(|index| segment(&format!("s{index}"), vec![on("usr.id", &["1"])]))
            .collect();

        assert!(normalize_segments(many).is_err());
    }
}
