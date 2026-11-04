// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What an assertion or an extraction may read, checked when a revision is saved rather than
//! when a run trips over it.

use crate::{
    domain::synthetics::{
        AssertionOperator, Extraction, MonitorAssertion, MonitorKind, assertion_source_allowed,
    },
    shared::{Error, Result},
};

const MAX_ASSERTIONS: usize = 20;

pub(super) fn validate_assertions(
    kind: MonitorKind,
    assertions: &[MonitorAssertion],
) -> Result<()> {
    if assertions.len() > MAX_ASSERTIONS {
        return Err(Error::invalid(format!(
            "a check step can have at most {MAX_ASSERTIONS} assertions"
        )));
    }

    for assertion in assertions {
        if !assertion_source_allowed(kind, assertion.source.trim()) {
            return Err(Error::invalid(format!(
                "{} assertions cannot read `{}`",
                kind.as_str(),
                assertion.source
            )));
        }

        validate_json_path(assertion.source.trim())?;

        match &assertion.operator {
            AssertionOperator::Matches { pattern } => {
                regex::Regex::new(pattern).map_err(|error| {
                    Error::invalid(format!("invalid assertion pattern `{pattern}`: {error}"))
                })?;
            }
            AssertionOperator::JsonSchema { schema } => {
                if assertion.source.trim() != "body" {
                    return Err(Error::invalid("a JSON Schema assertion reads the body"));
                }

                validate_json_schema(schema)?;
            }
            _ => {}
        }
    }

    Ok(())
}

/// A `json:` source names a JSONPath that parses.
fn validate_json_path(source: &str) -> Result<()> {
    if let Some(path) = source.strip_prefix("json:") {
        serde_json_path::JsonPath::parse(path.trim()).map_err(|error| {
            Error::invalid(format!("invalid JSONPath `{}`: {error}", path.trim()))
        })?;
    }

    Ok(())
}

/// A JSON Schema compiles without reaching outside itself: compiling one that referred to a URL
/// or a file would have the server fetch or read it.
fn validate_json_schema(schema: &serde_json::Value) -> Result<()> {
    jsonschema::options()
        .with_retriever(NoExternalReferences)
        .with_pattern_options(jsonschema::PatternOptions::regex())
        .build(schema)
        .map(|_| ())
        .map_err(|error| Error::invalid(format!("invalid JSON Schema: {error}")))
}

struct NoExternalReferences;

impl jsonschema::Retrieve for NoExternalReferences {
    fn retrieve(
        &self,
        uri: &jsonschema::Uri<String>,
    ) -> std::result::Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        Err(format!("a JSON Schema may not reference `{}`", uri.as_str()).into())
    }
}

/// An HTTP extraction reads what an HTTP assertion may read, through a pattern that compiles.
pub(super) fn validate_extractions(extractions: &[Extraction]) -> Result<()> {
    for extraction in extractions {
        let variable = extraction.variable.trim();

        if variable.is_empty() || variable.len() > 128 {
            return Err(Error::invalid(
                "an extraction needs a variable name of up to 128 characters",
            ));
        }

        if !assertion_source_allowed(MonitorKind::Http, extraction.source.trim()) {
            return Err(Error::invalid(format!(
                "extractions cannot read `{}`",
                extraction.source
            )));
        }

        validate_json_path(extraction.source.trim())?;

        regex::Regex::new(&extraction.expression).map_err(|error| {
            Error::invalid(format!(
                "invalid extraction pattern `{}`: {error}",
                extraction.expression
            ))
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_assertions;
    use crate::{
        domain::synthetics::{AssertionOperator, AssertionSeverity, MonitorAssertion, MonitorKind},
        shared::ids::Id,
    };

    fn reading(source: &str) -> MonitorAssertion {
        MonitorAssertion {
            id: Id::from_string("a"),
            name: "check".into(),
            source: source.into(),
            severity: AssertionSeverity::Critical,
            operator: AssertionOperator::Exists,
        }
    }

    #[test]
    fn each_kind_reads_its_own_sources() {
        assert!(validate_assertions(MonitorKind::Http, &[reading("header:etag")]).is_ok());
        assert!(validate_assertions(MonitorKind::Dns, &[reading("rcode")]).is_ok());
        assert!(validate_assertions(MonitorKind::Tcp, &[reading("response")]).is_ok());
        assert!(validate_assertions(MonitorKind::Grpc, &[reading("status")]).is_ok());

        let refused =
            validate_assertions(MonitorKind::Tcp, &[reading("body")]).expect_err("TCP has no body");
        assert!(
            refused
                .to_string()
                .contains("tcp assertions cannot read `body`")
        );
        assert!(validate_assertions(MonitorKind::Http, &[reading("header:")]).is_err());
    }

    #[test]
    fn a_json_path_has_to_parse_and_a_schema_reads_the_body() {
        assert!(validate_assertions(MonitorKind::Http, &[reading("json:$.items[0].id")]).is_ok());
        assert!(validate_assertions(MonitorKind::Http, &[reading("json:$.items[")]).is_err());

        let mut schema = reading("body");
        schema.operator = AssertionOperator::JsonSchema {
            schema: serde_json::json!({"type": "object"}),
        };
        assert!(validate_assertions(MonitorKind::Http, &[schema.clone()]).is_ok());

        schema.source = "header:content-type".into();
        assert!(validate_assertions(MonitorKind::Http, &[schema.clone()]).is_err());

        schema.source = "body".into();
        schema.operator = AssertionOperator::JsonSchema {
            schema: serde_json::json!({"$ref": "file:///etc/passwd"}),
        };
        assert!(validate_assertions(MonitorKind::Http, &[schema]).is_err());
    }

    #[test]
    fn a_pattern_has_to_compile() {
        let mut broken = reading("body");
        broken.operator = AssertionOperator::Matches {
            pattern: "(".into(),
        };

        assert!(validate_assertions(MonitorKind::Http, &[broken]).is_err());
    }
}
