// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 事件触发：一个新事件是否满足规则的筛选。
//!
//! 同一事件只分析一次；同一组事件（同一告警规则与同一组标签，即事件的指纹）在
//! [`MERGE_WINDOW_MICROS`] 之内只分析一次，接连开关的告警不会让同一个问题被分析好几遍。

use super::IncidentFilter;
use crate::domain::alerting::incident::Incident;

/// 同一组事件在这段时间内只分析一次。
pub const MERGE_WINDOW_MICROS: i64 = 30 * 60 * 1_000_000;

/// 筛选用到的事件事实。
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
    /// 填了的每一项都满足：严重级别、告警规则各中一个，服务里有一个受影响，标签值相等。
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
