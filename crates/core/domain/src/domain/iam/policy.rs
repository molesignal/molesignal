// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Permission policies: named sets of permissions, written as a JSON document after the AWS IAM
//! policy language.
//!
//! ```json
//! {
//!   "Version": "1",
//!   "Statement": [
//!     { "Sid": "ReadDashboards", "Effect": "Allow", "Action": ["dashboards.read", "alerts.*"] }
//!   ]
//! }
//! ```
//!
//! A statement allows the organization permissions its actions name; `*` in an action stands for
//! any run of characters, so `dashboards.*` is every dashboard permission and `*` all of them. A
//! policy only allows: there is no `Deny`, and what a role reaches on single resources is set by
//! access grants, not by a `Resource` in a policy.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::shared::{Error, Result};

/// The version of the policy language a document is written in.
pub const POLICY_VERSION: &str = "1";

/// How a built-in policy's id begins: `builtin:<key>`.
pub const BUILTIN_POLICY_PREFIX: &str = "builtin:";

/// The built-in policy a data intake token follows, the default intake token included.
pub const DATA_INTAKE_POLICY: &str = "builtin:data_intake";

/// Keys of the AWS policy language that this one leaves out, with where the idea lives instead.
const UNSUPPORTED_KEYS: [(&str, &str); 5] = [
    (
        "Resource",
        "a policy applies to the whole organization; scope a role to resources with an access grant",
    ),
    (
        "NotResource",
        "a policy applies to the whole organization; scope a role to resources with an access grant",
    ),
    ("NotAction", "list the actions a statement allows"),
    ("Condition", "conditions belong to access grants"),
    (
        "Principal",
        "a policy is attached to roles and followed by API tokens",
    ),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyDocument {
    #[serde(rename = "Version")]
    pub version: String,
    #[serde(rename = "Statement")]
    pub statements: Vec<PolicyStatement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyStatement {
    #[serde(rename = "Sid", default, skip_serializing_if = "Option::is_none")]
    pub sid: Option<String>,
    #[serde(rename = "Effect")]
    pub effect: String,
    #[serde(rename = "Action")]
    pub action: PolicyActions,
}

/// One action, or a list of them, as the AWS language writes either.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PolicyActions {
    One(String),
    Many(Vec<String>),
}

impl PolicyActions {
    fn as_slice(&self) -> &[String] {
        match self {
            Self::One(action) => std::slice::from_ref(action),
            Self::Many(actions) => actions,
        }
    }
}

impl PolicyDocument {
    /// Reads a document, naming what is wrong with it the way an editor can show.
    pub fn parse(value: &Value) -> Result<Self> {
        if let Some(statements) = value.get("Statement").and_then(Value::as_array) {
            for (index, statement) in statements.iter().enumerate() {
                for (key, instead) in UNSUPPORTED_KEYS {
                    if statement.get(key).is_some() {
                        return Err(Error::invalid(format!(
                            "Statement[{index}].{key} is not supported: {instead}"
                        )));
                    }
                }
            }
        }

        serde_json::from_value(value.clone())
            .map_err(|error| Error::invalid(format!("invalid policy document: {error}")))
    }

    /// One statement allowing exactly these permissions: what a policy picked from a list is.
    pub fn allowing(permissions: &[String]) -> Self {
        Self {
            version: POLICY_VERSION.to_string(),
            statements: vec![PolicyStatement {
                sid: None,
                effect: "Allow".to_string(),
                action: PolicyActions::Many(permissions.to_vec()),
            }],
        }
    }

    /// Whether an action names permissions by a pattern, so that a permission added to the
    /// catalog later may belong to the policy too.
    pub fn has_wildcards(&self) -> bool {
        self.statements.iter().any(|statement| {
            statement
                .action
                .as_slice()
                .iter()
                .any(|action| action.contains('*'))
        })
    }

    /// The permissions the document allows, sorted, among the organization permissions of the
    /// catalog. An action that names nothing there is an error, a pattern included. An empty
    /// statement or action list allows nothing, but the document as a whole has to allow
    /// something.
    pub fn resolve(&self, catalog: &BTreeSet<String>) -> Result<Vec<String>> {
        if self.version != POLICY_VERSION {
            return Err(Error::invalid(format!(
                "Version must be \"{POLICY_VERSION}\", not \"{}\"",
                self.version
            )));
        }

        let mut allowed = BTreeSet::new();

        for (index, statement) in self.statements.iter().enumerate() {
            if statement.effect != "Allow" {
                return Err(Error::invalid(format!(
                    "Statement[{index}].Effect must be \"Allow\": a policy only allows"
                )));
            }

            for (position, action) in statement.action.as_slice().iter().enumerate() {
                let action = action.trim().to_ascii_lowercase();
                let before = allowed.len();

                if action.contains('*') {
                    allowed.extend(
                        catalog
                            .iter()
                            .filter(|permission| wildcard_matches(&action, permission))
                            .cloned(),
                    );
                } else if catalog.contains(&action) {
                    allowed.insert(action.clone());
                }

                let matched = allowed.len() > before
                    || catalog
                        .iter()
                        .any(|permission| wildcard_matches(&action, permission));

                if !matched {
                    return Err(Error::invalid(format!(
                        "Statement[{index}].Action[{position}]: no permission is called `{action}`"
                    )));
                }
            }
        }

        if allowed.is_empty() {
            return Err(Error::invalid(
                "a policy must allow at least one permission",
            ));
        }

        Ok(allowed.into_iter().collect())
    }
}

/// Whether `text` matches `pattern`, where `*` stands for any run of characters.
fn wildcard_matches(pattern: &str, text: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();

    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };

    let parts = parts.collect::<Vec<_>>();

    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty();
    };

    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }

    rest.len() >= last.len() && rest.ends_with(last)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn catalog() -> BTreeSet<String> {
        [
            "alerts.rules.manage",
            "alerts.rules.read",
            "dashboards.edit",
            "dashboards.read",
            "streams.query",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    fn resolve(document: Value) -> Result<Vec<String>> {
        PolicyDocument::parse(&document)?.resolve(&catalog())
    }

    #[test]
    fn a_document_allows_what_its_actions_name() {
        let allowed = resolve(json!({
            "Version": "1",
            "Statement": [
                {"Sid": "Read", "Effect": "Allow", "Action": ["dashboards.read", "alerts.*"]},
                {"Effect": "Allow", "Action": "streams.query"}
            ]
        }))
        .expect("valid document");

        assert_eq!(
            allowed,
            [
                "alerts.rules.manage",
                "alerts.rules.read",
                "dashboards.read",
                "streams.query"
            ]
        );
    }

    #[test]
    fn a_star_alone_allows_every_permission() {
        let allowed = resolve(json!({
            "Version": "1",
            "Statement": [{"Effect": "Allow", "Action": "*"}]
        }))
        .expect("valid document");

        assert_eq!(allowed.len(), catalog().len());
    }

    #[test]
    fn a_pattern_in_the_middle_or_the_end_matches_as_written() {
        assert!(wildcard_matches("*.read", "alerts.rules.read"));
        assert!(wildcard_matches("dash*.e*t", "dashboards.edit"));
        assert!(!wildcard_matches("dashboards.*", "alerts.rules.read"));
        assert!(!wildcard_matches(
            "alerts.rules.read*x",
            "alerts.rules.read"
        ));
    }

    #[test]
    fn what_names_nothing_is_an_error_with_its_place() {
        let error = resolve(json!({
            "Version": "1",
            "Statement": [{"Effect": "Allow", "Action": ["dashboards.read", "dashboards.reed"]}]
        }))
        .expect_err("unknown permission");

        assert!(error.to_string().contains("Statement[0].Action[1]"));

        assert!(
            resolve(json!({
                "Version": "1",
                "Statement": [{"Effect": "Allow", "Action": "billing.*"}]
            }))
            .is_err()
        );
    }

    #[test]
    fn deny_resources_and_unknown_keys_are_refused() {
        assert!(
            resolve(json!({
                "Version": "1",
                "Statement": [{"Effect": "Deny", "Action": "alerts.rules.read"}]
            }))
            .is_err()
        );

        let resource = resolve(json!({
            "Version": "1",
            "Statement": [{"Effect": "Allow", "Action": "alerts.rules.read", "Resource": "*"}]
        }))
        .expect_err("Resource is not supported");

        assert!(resource.to_string().contains("access grant"));

        assert!(
            resolve(json!({
                "Version": "1",
                "Statement": [{"Effect": "Allow", "Action": "alerts.rules.read", "Note": "x"}]
            }))
            .is_err()
        );

        assert!(resolve(json!({"Version": "2", "Statement": []})).is_err());
    }

    #[test]
    fn an_empty_list_allows_nothing_but_a_policy_allows_something() {
        let allowed = resolve(json!({
            "Version": "1",
            "Statement": [
                {"Effect": "Allow", "Action": []},
                {"Effect": "Allow", "Action": "alerts.rules.read"}
            ]
        }))
        .expect("an empty statement beside one that allows");

        assert_eq!(allowed, ["alerts.rules.read"]);

        let error = resolve(json!({
            "Version": "1",
            "Statement": [{"Effect": "Allow", "Action": []}]
        }))
        .expect_err("allows nothing");

        assert!(error.to_string().contains("at least one permission"));
        assert!(resolve(json!({"Version": "1", "Statement": []})).is_err());
    }

    #[test]
    fn a_picked_list_is_one_statement_without_patterns() {
        let document = PolicyDocument::allowing(&[
            "alerts.rules.read".to_string(),
            "dashboards.read".to_string(),
        ]);

        assert!(!document.has_wildcards());
        assert_eq!(
            document.resolve(&catalog()).expect("valid"),
            ["alerts.rules.read", "dashboards.read"]
        );
        assert_eq!(
            serde_json::to_value(&document).expect("serializes"),
            json!({
                "Version": "1",
                "Statement": [{"Effect": "Allow", "Action": ["alerts.rules.read", "dashboards.read"]}]
            })
        );
    }
}
