// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Finding triggers check whether a newly discovered problem matches the rule's filters.
//! Each finding is investigated only once.

use domain::alerting::{discovery::Finding, incident::Severity};
use serde::{Deserialize, Serialize};

/// Specifies which new findings trigger a run: empty fields impose no limit; all set fields must
/// match.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingFilter {
    /// Finding kinds, such as `apm_anomaly`, `log_error_surge`.
    #[serde(default)]
    pub kinds: Vec<String>,
    /// Minimum severity: `info`, `warning`, `error`, `critical`.
    #[serde(default)]
    pub min_severity: Option<String>,
    /// Subjects: APM services, RUM applications, services in logs, hosts, or containers.
    #[serde(default)]
    pub subjects: Vec<String>,
}

impl FindingFilter {
    pub fn matches(&self, finding: &Finding) -> bool {
        let kind =
            self.kinds.is_empty() || self.kinds.iter().any(|kind| kind == finding.kind.as_str());
        let severity = self
            .min_severity
            .as_deref()
            .and_then(Severity::from_label)
            .is_none_or(|bar| finding.severity >= bar);
        let subject = self.subjects.is_empty()
            || self.subjects.iter().any(|subject| {
                *subject == finding.subject
                    || finding
                        .subject
                        .rsplit_once('/')
                        .is_some_and(|(_, name)| name == subject)
            });

        kind && severity && subject
    }
}
