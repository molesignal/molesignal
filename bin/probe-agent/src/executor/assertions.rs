// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Assertions, judged the same way whatever kind of check reads the values they look at.

use anyhow::{Result, anyhow, bail};
use regex::Regex;

use super::ExecutionContext;
use crate::protocol::v1::{self as wire, AssertionResult, AssertionSeverity, ProbeOutcome};

/// Judges `assertion` against the value `read` gives for its source. `read` answers `None` for a
/// source the check does not know, which is a setting the check cannot run with.
pub(super) fn evaluate(
    assertion: &wire::Assertion,
    read: impl Fn(&str) -> Option<String>,
    context: &ExecutionContext,
) -> Result<AssertionResult> {
    let actual = read(&assertion.source)
        .ok_or_else(|| anyhow!("unsupported Assertion source `{}`", assertion.source))?;

    let expected = assertion
        .expected
        .as_ref()
        .map(|value| context.resolve(value))
        .transpose()?
        .unwrap_or_default();

    let (passed, why) = match assertion.operator.as_str() {
        "equals" => (actual == expected, None),
        "not_equals" => (actual != expected, None),
        "contains" => (actual.contains(&expected), None),
        "not_contains" => (!actual.contains(&expected), None),
        "matches" => (Regex::new(&expected)?.is_match(&actual), None),
        "greater_than" => (number(&actual)? > number(&expected)?, None),
        "less_than" => (number(&actual)? < number(&expected)?, None),
        "exists" => (!actual.is_empty(), None),
        "json_schema" => holds_to_schema(&actual, &expected)?,
        operator => bail!("unsupported Assertion operator `{operator}`"),
    };

    Ok(AssertionResult {
        assertion_id: assertion.id.clone(),
        severity: assertion.severity,
        passed,
        actual: context.redact(&actual),
        message: match (passed, why) {
            (true, _) => String::new(),
            (false, Some(why)) => {
                context.redact(&format!("{} assertion failed: {why}", assertion.name))
            }
            (false, None) => format!("{} assertion failed", assertion.name),
        },
    })
}

/// Whether a JSON body holds to a JSON Schema, and the first way it does not.
fn holds_to_schema(actual: &str, schema: &str) -> Result<(bool, Option<String>)> {
    let schema = serde_json::from_str::<serde_json::Value>(schema)
        .map_err(|error| anyhow!("the JSON Schema is not JSON: {error}"))?;

    let validator = jsonschema::options()
        .with_retriever(NoExternalReferences)
        .with_pattern_options(jsonschema::PatternOptions::regex())
        .build(&schema)
        .map_err(|error| anyhow!("invalid JSON Schema: {error}"))?;

    let Ok(instance) = serde_json::from_str::<serde_json::Value>(actual) else {
        return Ok((false, Some("the body is not JSON".into())));
    };

    Ok(match validator.validate(&instance) {
        Ok(()) => (true, None),
        Err(error) => (false, Some(error.to_string())),
    })
}

/// A schema reaches no further than itself. A `$ref` to anything outside it would have the Agent
/// fetch a URL or read a file, past its egress policy.
struct NoExternalReferences;

impl jsonschema::Retrieve for NoExternalReferences {
    fn retrieve(
        &self,
        uri: &jsonschema::Uri<String>,
    ) -> std::result::Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        Err(format!("a JSON Schema may not reference `{}`", uri.as_str()).into())
    }
}

/// Judges every assertion of a check against the values `read` gives.
pub(super) fn evaluate_all(
    assertions: &[wire::Assertion],
    read: impl Fn(&str) -> Option<String>,
    context: &ExecutionContext,
) -> Result<Vec<AssertionResult>> {
    assertions
        .iter()
        .map(|assertion| evaluate(assertion, &read, context))
        .collect()
}

/// How the assertions judged an attempt: a failed critical assertion fails it, a failed warning
/// degrades it.
pub(super) fn outcome(assertions: &[AssertionResult]) -> ProbeOutcome {
    let failed = |severity: AssertionSeverity| {
        assertions
            .iter()
            .any(|result| !result.passed && result.severity == severity as i32)
    };

    if failed(AssertionSeverity::Critical) {
        ProbeOutcome::Failing
    } else if failed(AssertionSeverity::Warning) {
        ProbeOutcome::Degraded
    } else {
        ProbeOutcome::Healthy
    }
}

fn number(value: &str) -> Result<f64> {
    value
        .trim()
        .parse()
        .map_err(|_| anyhow!("`{value}` is not a number"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{evaluate, outcome};
    use crate::{
        executor::ExecutionContext,
        protocol::v1::{self as wire, AssertionSeverity, ProbeOutcome, value::Source},
    };

    fn context() -> ExecutionContext {
        ExecutionContext {
            secrets: HashMap::new(),
            variables: HashMap::new(),
            dialled: Default::default(),
        }
    }

    fn assertion(source: &str, operator: &str, expected: &str) -> wire::Assertion {
        wire::Assertion {
            id: "a".into(),
            name: "check".into(),
            severity: AssertionSeverity::Critical as i32,
            source: source.into(),
            operator: operator.into(),
            expected: Some(wire::Value {
                source: Some(Source::Literal(expected.into())),
            }),
        }
    }

    fn read(source: &str) -> Option<String> {
        match source {
            "rcode" => Some("NOERROR".into()),
            "duration_ms" => Some("42".into()),
            _ => None,
        }
    }

    #[test]
    fn judges_a_value_by_its_operator() {
        let passed = evaluate(&assertion("rcode", "equals", "NOERROR"), read, &context())
            .expect("evaluated");
        let slow = evaluate(
            &assertion("duration_ms", "less_than", "40"),
            read,
            &context(),
        )
        .expect("evaluated");

        assert!(passed.passed);
        assert!(!slow.passed);
        assert_eq!(outcome(&[passed, slow]), ProbeOutcome::Failing);
    }

    #[test]
    fn a_body_is_held_to_its_json_schema() {
        let schema =
            r#"{"type":"object","required":["status"],"properties":{"status":{"enum":["up"]}}}"#;
        let up = |source: &str| (source == "body").then(|| r#"{"status":"up"}"#.to_string());
        let down = |source: &str| (source == "body").then(|| r#"{"status":"down"}"#.to_string());

        let passed =
            evaluate(&assertion("body", "json_schema", schema), up, &context()).expect("evaluated");
        let failed = evaluate(&assertion("body", "json_schema", schema), down, &context())
            .expect("evaluated");

        assert!(passed.passed);
        assert!(!failed.passed);
        assert!(
            failed.message.starts_with("check assertion failed: "),
            "{}",
            failed.message
        );
    }

    #[test]
    fn a_json_schema_reaches_no_further_than_itself() {
        let schema = r#"{"$ref":"http://203.0.113.1/schema.json"}"#;
        let body = |_: &str| Some("{}".to_string());

        let error = evaluate(&assertion("body", "json_schema", schema), body, &context())
            .expect_err("an external reference is refused");

        assert!(error.to_string().contains("invalid JSON Schema"), "{error}");
    }

    #[test]
    fn refuses_a_source_the_check_does_not_read() {
        let error = evaluate(&assertion("body", "contains", "ok"), read, &context())
            .expect_err("unknown source");

        assert_eq!(error.to_string(), "unsupported Assertion source `body`");
    }
}
