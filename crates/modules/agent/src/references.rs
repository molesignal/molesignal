// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! References in messages: incidents, alert rules, services, RUM applications, dashboards, and
//! streams inserted with `@` in the input box.
//!
//! When a message is sent, the backend checks whether the requester can read each reference, then
//! resolves them into context blocks for the model (incident summary, triggering query, time
//! window, related services, etc.); references are recorded on the user message so the UI can
//! render chips.

use serde::{Deserialize, Serialize};

/// The maximum number of references per message.
pub const MAX_REFERENCES: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    Incident,
    AlertRule,
    Service,
    RumApplication,
    Dashboard,
    Stream,
}

impl ReferenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Incident => "incident",
            Self::AlertRule => "alert_rule",
            Self::Service => "service",
            Self::RumApplication => "rum_application",
            Self::Dashboard => "dashboard",
            Self::Stream => "stream",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageReference {
    pub kind: ReferenceKind,
    pub id: String,
    /// The name displayed on the chip; replaced with the current name when the backend resolves it.
    #[serde(default)]
    pub label: String,
}

/// The representation of a reference passed to the model.
pub fn context_block(reference: &MessageReference, facts: &str) -> String {
    format!(
        "<reference kind=\"{}\" id=\"{}\" name=\"{}\">\n{}\n</reference>",
        reference.kind.as_str(),
        reference.id.replace('"', "'"),
        reference.label.replace('"', "'"),
        facts.trim()
    )
}

/// The introduction to the references goes before these blocks.
pub const REFERENCES_INTRODUCTION: &str = "The person attached the objects below to this question. Use them as the subject of the question; fetch more about them with your tools when needed.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rum_application_is_a_kind_of_its_own() {
        let reference: MessageReference = serde_json::from_value(
            serde_json::json!({"kind": "rum_application", "id": "shop-web", "label": "Shop"}),
        )
        .unwrap();

        assert_eq!(reference.kind, ReferenceKind::RumApplication);
        assert_eq!(reference.kind.as_str(), "rum_application");
        assert!(context_block(&reference, "x").starts_with("<reference kind=\"rum_application\""));
    }

    #[test]
    fn a_reference_names_what_it_is() {
        let reference: MessageReference = serde_json::from_value(
            serde_json::json!({"kind": "alert_rule", "id": "r1", "label": "API \"5xx\""}),
        )
        .unwrap();

        let block = context_block(&reference, "query: count(5xx) > 10\n");

        assert!(block.starts_with("<reference kind=\"alert_rule\" id=\"r1\" name=\"API '5xx'\">"));
        assert!(block.ends_with("count(5xx) > 10\n</reference>"));
    }
}
