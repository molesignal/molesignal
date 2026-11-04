// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Which events a step that keeps count applies to: field tests that must all hold. They are the
//! tests of the editor's filter, read the same way, so a condition and a filter that say the same
//! thing pick the same events. An event the condition does not pick goes past the step untouched.
//!
//! A field is looked up as its key first (`service.name` as one key), then as a path into the
//! event (`name` inside `service`).
use std::cmp::Ordering;

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

use crate::shared::{Error, Result};

/// At most this many tests in one condition.
pub const MAX_TESTS: usize = 16;
const MAX_FIELD_LENGTH: usize = 255;
const MAX_VALUE_LENGTH: usize = 1_024;

/// What a test asks of a field.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FieldTest {
    Equals,
    NotEquals,
    Contains,
    StartsWith,
    Matches,
    Exists,
    Missing,
    Gt,
    Gte,
    Lt,
    Lte,
}

/// One test as a pipeline saves it.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FieldCondition {
    pub field: String,
    pub test: FieldTest,
    #[serde(default)]
    pub value: String,
}

/// The tests of a step, ready to run. None: every event.
#[derive(Debug, Clone, Default)]
pub struct Condition {
    tests: Vec<Compiled>,
}

#[derive(Debug, Clone)]
struct Compiled {
    field: String,
    test: Test,
}

#[derive(Debug, Clone)]
enum Test {
    /// The field holds this text; a number or a boolean typed as text also matches its value.
    Equals(Literal),
    NotEquals(Literal),
    Contains(String),
    StartsWith(String),
    Matches(Regex),
    Exists,
    Missing,
    /// The field is a number, or text that reads as one, standing so to the value.
    Compare(Ordering, bool, f64),
}

#[derive(Debug, Clone)]
struct Literal {
    text: String,
    number: Option<f64>,
    boolean: Option<bool>,
}

impl Condition {
    /// The saved tests, checked and compiled. A test without a field, a pattern that does not
    /// compile or a comparison with something that is not a number is refused.
    pub fn compile(conditions: &[FieldCondition]) -> Result<Self> {
        if conditions.len() > MAX_TESTS {
            return Err(Error::invalid(format!(
                "a condition has at most {MAX_TESTS} tests"
            )));
        }

        let tests = conditions
            .iter()
            .map(compile_one)
            .collect::<Result<Vec<_>>>()?;

        Ok(Self { tests })
    }

    /// Whether every test holds for `event`.
    pub fn matches(&self, event: &Value) -> bool {
        self.tests.iter().all(|compiled| {
            let value = field_value(event, &compiled.field);
            compiled.test.holds(value)
        })
    }
}

fn compile_one(condition: &FieldCondition) -> Result<Compiled> {
    let field = condition.field.trim();

    if field.is_empty() || field.len() > MAX_FIELD_LENGTH {
        return Err(Error::invalid("every test of a condition names a field"));
    }

    if condition.value.len() > MAX_VALUE_LENGTH {
        return Err(Error::invalid(format!(
            "the value of a test is at most {MAX_VALUE_LENGTH} bytes"
        )));
    }

    let value = &condition.value;

    let test = match condition.test {
        FieldTest::Equals => Test::Equals(Literal::of(value)),
        FieldTest::NotEquals => Test::NotEquals(Literal::of(value)),
        FieldTest::Contains => Test::Contains(value.clone()),
        FieldTest::StartsWith => Test::StartsWith(value.clone()),
        FieldTest::Matches => Test::Matches(Regex::new(value).map_err(|error| {
            Error::invalid(format!(
                "the pattern of `{field}` does not compile: {error}"
            ))
        })?),
        FieldTest::Exists => Test::Exists,
        FieldTest::Missing => Test::Missing,
        FieldTest::Gt | FieldTest::Gte | FieldTest::Lt | FieldTest::Lte => {
            let number = number_literal(value).ok_or_else(|| {
                Error::invalid(format!(
                    "`{field}` is compared with a number, not `{value}`"
                ))
            })?;

            match condition.test {
                FieldTest::Gt => Test::Compare(Ordering::Greater, false, number),
                FieldTest::Gte => Test::Compare(Ordering::Greater, true, number),
                FieldTest::Lt => Test::Compare(Ordering::Less, false, number),
                _ => Test::Compare(Ordering::Less, true, number),
            }
        }
    };

    Ok(Compiled {
        field: field.to_owned(),
        test,
    })
}

impl Test {
    fn holds(&self, value: Option<&Value>) -> bool {
        match self {
            Self::Exists => value.is_some(),
            Self::Missing => value.is_none(),
            Self::Equals(literal) => value.is_some_and(|value| literal.equals(value)),
            Self::NotEquals(literal) => !value.is_some_and(|value| literal.equals(value)),
            Self::Contains(text) => text_of(value).is_some_and(|value| value.contains(text)),
            Self::StartsWith(text) => text_of(value).is_some_and(|value| value.starts_with(text)),
            Self::Matches(pattern) => text_of(value).is_some_and(|value| pattern.is_match(value)),
            Self::Compare(side, or_equal, number) => value
                .and_then(number_of)
                .and_then(|value| value.partial_cmp(number))
                .is_some_and(|ordering| ordering == *side || (*or_equal && ordering.is_eq())),
        }
    }
}

impl Literal {
    fn of(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            number: number_literal(text),
            boolean: match text {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            },
        }
    }

    fn equals(&self, value: &Value) -> bool {
        match value {
            Value::String(text) => *text == self.text,
            Value::Number(number) => self.number.is_some() && number.as_f64() == self.number,
            Value::Bool(boolean) => self.boolean == Some(*boolean),
            _ => false,
        }
    }
}

/// The field of `event`: its key as written, else the path of its dotted parts.
pub fn field_value<'a>(event: &'a Value, field: &str) -> Option<&'a Value> {
    event.get(field).or_else(|| {
        if !field.contains('.') {
            return None;
        }

        field
            .split('.')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .try_fold(event, |value, key| value.get(key))
    })
}

/// A number, or text that reads as one: what the numeric tests and metrics take.
pub fn number_of(value: &Value) -> Option<f64> {
    let number = match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    };

    number.filter(|number| number.is_finite())
}

fn text_of(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

/// A number as typed in a test: digits, an optional minus and an optional fraction.
fn number_literal(text: &str) -> Option<f64> {
    let text = text.trim();
    let digits = text.strip_prefix('-').unwrap_or(text);
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, "0"));

    let plain = !whole.is_empty()
        && !fraction.is_empty()
        && whole.bytes().all(|byte| byte.is_ascii_digit())
        && fraction.bytes().all(|byte| byte.is_ascii_digit());

    plain.then(|| text.parse().ok()).flatten()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn condition(tests: Value) -> Condition {
        let tests: Vec<FieldCondition> = serde_json::from_value(tests).unwrap();
        Condition::compile(&tests).unwrap()
    }

    #[test]
    fn every_test_must_hold_and_none_picks_every_event() {
        let errors = condition(json!([
            { "field": "level", "test": "equals", "value": "error" },
            { "field": "status", "test": "gte", "value": "500" },
        ]));

        assert!(errors.matches(&json!({ "level": "error", "status": 503 })));
        assert!(errors.matches(&json!({ "level": "error", "status": "500" })));
        assert!(!errors.matches(&json!({ "level": "error", "status": 404 })));
        assert!(!errors.matches(&json!({ "level": "info", "status": 503 })));
        assert!(!errors.matches(&json!({ "level": "error" })));
        assert!(Condition::default().matches(&json!({})));
    }

    #[test]
    fn equality_takes_numbers_and_booleans_typed_as_text() {
        let ok = condition(json!([{ "field": "status", "test": "equals", "value": "200" }]));

        assert!(ok.matches(&json!({ "status": 200 })));
        assert!(ok.matches(&json!({ "status": 200.0 })));
        assert!(ok.matches(&json!({ "status": "200" })));
        assert!(!ok.matches(&json!({ "status": "200 " })));

        let cached = condition(json!([{ "field": "cached", "test": "equals", "value": "true" }]));
        assert!(cached.matches(&json!({ "cached": true })));
        assert!(cached.matches(&json!({ "cached": "true" })));
        assert!(!cached.matches(&json!({ "cached": 1 })));

        // A field that is not there is not equal to anything.
        let other = condition(json!([{ "field": "env", "test": "not_equals", "value": "prod" }]));
        assert!(other.matches(&json!({})));
        assert!(!other.matches(&json!({ "env": "prod" })));
    }

    #[test]
    fn a_field_is_its_key_before_its_path() {
        let service = condition(json!([
            { "field": "service.name", "test": "equals", "value": "checkout" },
        ]));

        assert!(service.matches(&json!({ "service.name": "checkout" })));
        assert!(service.matches(&json!({ "service": { "name": "checkout" } })));
        assert!(!service.matches(&json!({
            "service.name": "cart",
            "service": { "name": "checkout" },
        })));
    }

    #[test]
    fn text_tests_read_text_only() {
        let slow = condition(json!([
            { "field": "path", "test": "starts_with", "value": "/api" },
            { "field": "message", "test": "contains", "value": "timeout" },
            { "field": "host", "test": "matches", "value": "^web-\\d+$" },
        ]));

        assert!(slow.matches(&json!({
            "path": "/api/orders",
            "message": "upstream timeout",
            "host": "web-12",
        })));
        assert!(!slow.matches(&json!({
            "path": "/api/orders",
            "message": "upstream timeout",
            "host": 12,
        })));
        assert!(
            condition(json!([{ "field": "x", "test": "exists" }])).matches(&json!({ "x": null }))
        );
        assert!(condition(json!([{ "field": "x", "test": "missing" }])).matches(&json!({})));
    }

    #[test]
    fn a_test_that_cannot_run_is_refused() {
        for tests in [
            json!([{ "field": " ", "test": "exists" }]),
            json!([{ "field": "host", "test": "matches", "value": "(" }]),
            json!([{ "field": "status", "test": "gt", "value": "fast" }]),
            json!([{ "field": "status", "test": "gt", "value": "1e3" }]),
        ] {
            let tests: Vec<FieldCondition> = serde_json::from_value(tests).unwrap();
            assert!(Condition::compile(&tests).is_err());
        }

        let many = vec![
            FieldCondition {
                field: "x".into(),
                test: FieldTest::Exists,
                value: String::new(),
            };
            MAX_TESTS + 1
        ];
        assert!(Condition::compile(&many).is_err());
    }

    #[test]
    fn numbers_come_from_numbers_and_numeric_text() {
        assert_eq!(number_of(&json!(1.5)), Some(1.5));
        assert_eq!(number_of(&json!(" 42 ")), Some(42.0));
        assert_eq!(number_of(&json!("fast")), None);
        assert_eq!(number_of(&json!(true)), None);
        assert_eq!(number_of(&json!("NaN")), None);
    }
}
