// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The RUM event contract: one JSON Schema for the events SDKs send to `POST /api/v1/rum/events`,
//! shared with the SDK repositories through `contracts/rum/events/`.

use std::sync::LazyLock;

use super::validator::ContractValidator;

pub const RUM_EVENT_V1_SCHEMA: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../contracts/rum/events/v1.schema.json"
));

static RUM_EVENT_VALIDATOR: LazyLock<ContractValidator> = LazyLock::new(|| {
    ContractValidator::from_json(RUM_EVENT_V1_SCHEMA)
        .expect("embedded RUM event v1 schema must compile")
});

/// Validates one event; the body of a request is validated event by event.
#[must_use]
pub fn rum_event_validator() -> &'static ContractValidator {
    &RUM_EVENT_VALIDATOR
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use serde_json::Value;

    use super::*;

    fn contract_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../contracts/rum/events")
    }

    fn read(path: &str) -> Value {
        let text = fs::read_to_string(contract_dir().join("fixtures").join(path))
            .unwrap_or_else(|error| panic!("{path}: {error}"));

        serde_json::from_str(&text).unwrap_or_else(|error| panic!("{path}: {error}"))
    }

    #[test]
    fn the_embedded_schema_compiles() {
        assert!(!rum_event_validator().schema_hash().is_empty());
    }

    #[test]
    fn every_fixture_has_the_validity_and_issue_codes_the_manifest_states() {
        let manifest = read("manifest.json");
        let cases = manifest["cases"].as_array().expect("cases");

        assert!(cases.len() >= 20, "{} cases", cases.len());

        for case in cases {
            let path = case["path"].as_str().expect("path");
            let expected_valid = case["valid"].as_bool().expect("valid");
            let body = read(path);

            let events = match (case["batch"].as_bool().unwrap_or(false), &body) {
                (true, Value::Array(items)) => items.iter().collect::<Vec<_>>(),
                (false, event) => vec![event],
                (true, _) => panic!("{path}: a batch fixture must be an array"),
            };

            let mut codes = Vec::new();

            for event in &events {
                codes.extend(
                    rum_event_validator()
                        .validate(event)
                        .into_iter()
                        .map(|issue| issue.code),
                );
            }

            assert_eq!(codes.is_empty(), expected_valid, "{path}: {codes:?}");

            if !expected_valid {
                for expected in case["expectedIssueCodes"].as_array().expect("codes") {
                    let expected = expected.as_str().expect("code");

                    assert!(
                        codes.iter().any(|code| code == expected),
                        "{path}: wanted {expected} among {codes:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_fixture_in_the_directory_is_in_the_manifest() {
        let manifest = read("manifest.json");

        let listed = manifest["cases"]
            .as_array()
            .expect("cases")
            .iter()
            .map(|case| case["path"].as_str().expect("path").to_string())
            .collect::<std::collections::BTreeSet<_>>();

        let mut found = std::collections::BTreeSet::new();

        for folder in ["valid", "invalid"] {
            let directory = contract_dir().join("fixtures").join(folder);

            for entry in fs::read_dir(&directory).expect("fixture directory") {
                let name = entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned();

                found.insert(format!("{folder}/{name}"));
            }
        }

        assert_eq!(listed, found);
    }
}
