// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Incident triggers check whether a new incident matches the rule's filters.
//!
//! Each incident is analyzed only once; each group of incidents (the same alert rule and label set,
//! which form the incident fingerprint) is analyzed only once within [`MERGE_WINDOW_MICROS`], so
//! alerts that repeatedly open and close do not cause repeated analysis of the same problem.

use domain::alerting::incident::Incident;

use super::IncidentFilter;

/// Each group of incidents is analyzed only once within this time period.
pub const MERGE_WINDOW_MICROS: i64 = 30 * 60 * 1_000_000;

/// Incident facts used for filtering.
#[derive(Debug, Clone, Copy)]
pub struct IncidentFacts<'a> {
    pub severity: &'a str,
    pub rule_id: &'a str,
    pub services: &'a [String],
    pub labels: &'a std::collections::BTreeMap<String, String>,
}

impl<'a> IncidentFacts<'a> {
    pub fn of(incident: &'a Incident) -> Self {
        Self {
            severity: incident.severity.as_str(),
            rule_id: incident.rule_id.as_str(),
            services: &incident.affected_services,
            labels: &incident.labels,
        }
    }
}

impl IncidentFilter {
    /// All set fields must match: one severity, one alert rule, one affected service, and equal
    /// label values.
    pub fn matches(&self, facts: IncidentFacts<'_>) -> bool {
        let severity = self.severities.is_empty()
            || self
                .severities
                .iter()
                .any(|value| value.eq_ignore_ascii_case(facts.severity));

        let rule = self.rule_ids.is_empty() || self.rule_ids.iter().any(|id| id == facts.rule_id);

        let service = self.services.is_empty()
            || self
                .services
                .iter()
                .any(|wanted| facts.services.iter().any(|service| service == wanted));

        let labels = self
            .labels
            .iter()
            .all(|(key, value)| facts.labels.get(key) == Some(value));

        severity && rule && service && labels
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn facts<'a>(
        severity: &'a str,
        services: &'a [String],
        labels: &'a BTreeMap<String, String>,
    ) -> IncidentFacts<'a> {
        IncidentFacts {
            severity,
            rule_id: "rule-1",
            services,
            labels,
        }
    }

    #[test]
    fn an_empty_filter_takes_every_incident() {
        let labels = BTreeMap::new();
        assert!(IncidentFilter::default().matches(facts("info", &[], &labels)));
    }

    #[test]
    fn every_filled_part_must_match() {
        let services = vec!["checkout".to_string(), "payments".to_string()];
        let labels = BTreeMap::from([("env".to_string(), "prod".to_string())]);

        let filter = IncidentFilter {
            severities: vec!["Critical".into(), "error".into()],
            rule_ids: vec!["rule-1".into()],
            services: vec!["payments".into()],
            labels: BTreeMap::from([("env".to_string(), "prod".to_string())]),
        };

        assert!(filter.matches(facts("critical", &services, &labels)));
        assert!(!filter.matches(facts("warning", &services, &labels)));
        assert!(!filter.matches(facts("critical", &services[..1], &labels)));

        let staging = BTreeMap::from([("env".to_string(), "staging".to_string())]);
        assert!(!filter.matches(facts("critical", &services, &staging)));

        let other_rule = IncidentFilter {
            rule_ids: vec!["rule-2".into()],
            ..IncidentFilter::default()
        };
        assert!(!other_rule.matches(facts("critical", &services, &labels)));
    }
}
