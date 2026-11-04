// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How the health of a component is worked out from what is going on: open
//! alert incidents and the state of synthetic monitors. A component is as bad
//! as the worst of its dependencies, each passing on its status as its impact
//! allows.

use std::collections::HashMap;

use super::{CauseIncident, ComponentDependency, DependencyKind, DependencyStatus, HealthCause};
use crate::{
    domain::{alerting::incident::Severity, synthetics::MonitorState},
    shared::ids::Id,
};

/// How many incidents a cause names.
const CAUSE_INCIDENTS: usize = 5;

/// An open alert incident that has not been muted.
#[derive(Debug, Clone)]
pub struct IncidentSignal {
    pub id: Id,
    pub rule_id: Id,
    pub severity: Severity,
    pub summary: String,
    /// The services the incident is about: its service labels and the
    /// services its query saw.
    pub services: Vec<String>,
}

/// A synthetic monitor and the state it is in. A monitor that is not active
/// is not watched and passes nothing on.
#[derive(Debug, Clone)]
pub struct MonitorSignal {
    pub id: Id,
    pub state: MonitorState,
    pub watched: bool,
}

#[derive(Debug, Clone, Default)]
pub struct HealthSignals {
    pub incidents: Vec<IncidentSignal>,
    pub monitors: Vec<MonitorSignal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    pub status: DependencyStatus,
    pub causes: Vec<HealthCause>,
}

/// The health that follows from the dependencies given.
pub fn evaluate(dependencies: &[ComponentDependency], signals: &HealthSignals) -> Evaluation {
    let monitors: HashMap<&str, &MonitorSignal> = signals
        .monitors
        .iter()
        .map(|monitor| (monitor.id.as_str(), monitor))
        .collect();

    let mut causes: Vec<HealthCause> = dependencies
        .iter()
        .filter_map(|dependency| {
            let mut cause = match dependency.kind {
                DependencyKind::TechnicalService => {
                    let target = dependency.target.trim();

                    incident_cause(&signals.incidents, |incident| {
                        incident
                            .services
                            .iter()
                            .any(|service| service.trim().eq_ignore_ascii_case(target))
                    })
                }
                DependencyKind::AlertRule => incident_cause(&signals.incidents, |incident| {
                    incident.rule_id.as_str() == dependency.target
                }),
                DependencyKind::SyntheticMonitor => {
                    monitor_cause(monitors.get(dependency.target.as_str()).copied())
                }
            };

            cause.status = cause.status.passed_on(dependency.impact);
            cause.kind = dependency.kind;
            cause.target = dependency.target.clone();

            (cause.status != DependencyStatus::Operational).then_some(cause)
        })
        .collect();

    causes.sort_by_key(|cause| std::cmp::Reverse(cause.status.rank()));

    let status = causes
        .iter()
        .fold(DependencyStatus::Operational, |worst, cause| {
            worst.worst(cause.status)
        });

    Evaluation { status, causes }
}

fn incident_cause(
    incidents: &[IncidentSignal],
    about: impl Fn(&IncidentSignal) -> bool,
) -> HealthCause {
    let mut incidents: Vec<&IncidentSignal> = incidents
        .iter()
        .filter(|incident| about(incident))
        .collect();

    incidents.sort_by(|left, right| {
        right
            .severity
            .cmp(&left.severity)
            .then_with(|| left.id.as_str().cmp(right.id.as_str()))
    });

    let status = incidents
        .first()
        .map(|incident| DependencyStatus::from_severity(incident.severity))
        .unwrap_or(DependencyStatus::Operational);

    HealthCause {
        incident_count: u32::try_from(incidents.len()).unwrap_or(u32::MAX),
        incidents: incidents
            .into_iter()
            .take(CAUSE_INCIDENTS)
            .map(|incident| CauseIncident {
                id: incident.id.clone(),
                summary: incident.summary.clone(),
                severity: incident.severity,
            })
            .collect(),
        ..blank_cause(status)
    }
}

fn monitor_cause(monitor: Option<&MonitorSignal>) -> HealthCause {
    match monitor {
        Some(monitor) if monitor.watched => HealthCause {
            monitor_state: Some(monitor.state),
            ..blank_cause(DependencyStatus::from_monitor_state(monitor.state))
        },
        _ => blank_cause(DependencyStatus::Operational),
    }
}

fn blank_cause(status: DependencyStatus) -> HealthCause {
    HealthCause {
        kind: DependencyKind::TechnicalService,
        target: String::new(),
        status,
        incidents: Vec::new(),
        incident_count: 0,
        monitor_state: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::status_page::DependencyImpact;

    fn depends(
        kind: DependencyKind,
        target: &str,
        impact: DependencyImpact,
    ) -> ComponentDependency {
        ComponentDependency {
            kind,
            target: target.into(),
            impact,
        }
    }

    fn incident(id: &str, rule: &str, severity: Severity, services: &[&str]) -> IncidentSignal {
        IncidentSignal {
            id: Id(id.into()),
            rule_id: Id(rule.into()),
            severity,
            summary: format!("{id} summary"),
            services: services
                .iter()
                .map(|service| (*service).to_string())
                .collect(),
        }
    }

    #[test]
    fn a_technical_service_takes_the_worst_severity_of_its_open_incidents() {
        let dependencies = [depends(
            DependencyKind::TechnicalService,
            "Payments-API",
            DependencyImpact::Outage,
        )];

        let signals = HealthSignals {
            incidents: vec![
                incident("i1", "r1", Severity::Warning, &["payments-api"]),
                incident("i2", "r2", Severity::Critical, &["payments-api", "ledger"]),
                incident("i3", "r3", Severity::Critical, &["search"]),
            ],
            monitors: Vec::new(),
        };

        let evaluation = evaluate(&dependencies, &signals);
        assert_eq!(evaluation.status, DependencyStatus::MajorOutage);
        assert_eq!(evaluation.causes.len(), 1);
        assert_eq!(evaluation.causes[0].incident_count, 2);
        assert_eq!(evaluation.causes[0].incidents[0].id, Id("i2".into()));
        assert_eq!(evaluation.causes[0].target, "Payments-API");
    }

    #[test]
    fn the_worst_dependency_decides_as_its_impact_allows() {
        let dependencies = [
            depends(
                DependencyKind::SyntheticMonitor,
                "m-checkout",
                DependencyImpact::Outage,
            ),
            depends(
                DependencyKind::AlertRule,
                "r-reco",
                DependencyImpact::Degraded,
            ),
        ];

        let signals = HealthSignals {
            incidents: vec![incident("i1", "r-reco", Severity::Critical, &[])],
            monitors: vec![MonitorSignal {
                id: Id("m-checkout".into()),
                state: MonitorState::Healthy,
                watched: true,
            }],
        };

        let evaluation = evaluate(&dependencies, &signals);
        assert_eq!(evaluation.status, DependencyStatus::DegradedPerformance);
        assert_eq!(evaluation.causes[0].kind, DependencyKind::AlertRule);
        assert_eq!(evaluation.causes[0].target, "r-reco");
    }

    #[test]
    fn a_monitor_that_is_not_watched_or_only_flaky_passes_nothing_on() {
        let dependencies = [
            depends(
                DependencyKind::SyntheticMonitor,
                "paused",
                DependencyImpact::Outage,
            ),
            depends(
                DependencyKind::SyntheticMonitor,
                "flaky",
                DependencyImpact::Outage,
            ),
            depends(
                DependencyKind::SyntheticMonitor,
                "missing",
                DependencyImpact::Outage,
            ),
        ];

        let signals = HealthSignals {
            incidents: Vec::new(),
            monitors: vec![
                MonitorSignal {
                    id: Id("paused".into()),
                    state: MonitorState::Failing,
                    watched: false,
                },
                MonitorSignal {
                    id: Id("flaky".into()),
                    state: MonitorState::Flaky,
                    watched: true,
                },
            ],
        };

        let evaluation = evaluate(&dependencies, &signals);
        assert_eq!(evaluation.status, DependencyStatus::Operational);
        assert!(evaluation.causes.is_empty());
    }
}
