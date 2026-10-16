// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How the health of business services is worked out from what is going on:
//! open alert incidents and the state of synthetic monitors. A business
//! service is as bad as the worst dependency it has, each dependency passing
//! on its status as its impact allows; a business service that depends on
//! another takes on that one's health in the same way.

use std::collections::{HashMap, HashSet};

use super::{BusinessService, BusinessServiceStatus, CauseIncident, DependencyKind, HealthCause};
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
    pub status: BusinessServiceStatus,
    pub causes: Vec<HealthCause>,
}

/// The health of every service given, by id.
pub fn evaluate(services: &[BusinessService], signals: &HealthSignals) -> HashMap<Id, Evaluation> {
    let evaluator = Evaluator {
        services: services
            .iter()
            .map(|service| (service.id.as_str(), service))
            .collect(),
        monitors: signals
            .monitors
            .iter()
            .map(|monitor| (monitor.id.as_str(), monitor))
            .collect(),
        incidents: &signals.incidents,
    };

    let mut done = HashMap::new();

    for service in services {
        evaluator.evaluate(service.id.as_str(), &mut done, &mut HashSet::new());
    }

    done.into_iter()
        .map(|(id, evaluation)| (Id(id.to_string()), evaluation))
        .collect()
}

struct Evaluator<'a> {
    services: HashMap<&'a str, &'a BusinessService>,
    monitors: HashMap<&'a str, &'a MonitorSignal>,
    incidents: &'a [IncidentSignal],
}

impl<'a> Evaluator<'a> {
    fn evaluate(
        &self,
        id: &'a str,
        done: &mut HashMap<&'a str, Evaluation>,
        visiting: &mut HashSet<&'a str>,
    ) -> BusinessServiceStatus {
        if let Some(evaluation) = done.get(id) {
            return evaluation.status;
        }

        // A service reached again on its own path is a cycle the catalog
        // refuses to save; should one exist, it passes nothing on.
        let Some(service) = self.services.get(id) else {
            return BusinessServiceStatus::Operational;
        };

        if !visiting.insert(id) {
            return BusinessServiceStatus::Operational;
        }

        let mut causes = Vec::new();

        for dependency in &service.dependencies {
            let mut cause = match dependency.kind {
                DependencyKind::TechnicalService => {
                    let target = dependency.target.trim();

                    self.incident_cause(|incident| {
                        incident
                            .services
                            .iter()
                            .any(|service| service.trim().eq_ignore_ascii_case(target))
                    })
                }
                DependencyKind::AlertRule => {
                    self.incident_cause(|incident| incident.rule_id.as_str() == dependency.target)
                }
                DependencyKind::SyntheticMonitor => self.monitor_cause(&dependency.target),
                DependencyKind::BusinessService => {
                    let target = self
                        .services
                        .get_key_value(dependency.target.as_str())
                        .map(|(key, _)| *key);

                    let status = target
                        .map(|target| self.evaluate(target, done, visiting))
                        .unwrap_or(BusinessServiceStatus::Operational);

                    blank_cause(status)
                }
            };

            cause.status = cause.status.passed_on(dependency.impact);

            if cause.status != BusinessServiceStatus::Operational {
                cause.kind = dependency.kind;
                cause.target = dependency.target.clone();
                causes.push(cause);
            }
        }

        causes.sort_by_key(|cause| std::cmp::Reverse(cause.status.rank()));

        let status = causes
            .iter()
            .fold(BusinessServiceStatus::Operational, |worst, cause| {
                worst.worst(cause.status)
            });

        visiting.remove(id);
        done.insert(id, Evaluation { status, causes });
        status
    }

    fn incident_cause(&self, about: impl Fn(&IncidentSignal) -> bool) -> HealthCause {
        let mut incidents: Vec<&IncidentSignal> = self
            .incidents
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
            .map(|incident| BusinessServiceStatus::from_severity(incident.severity))
            .unwrap_or(BusinessServiceStatus::Operational);

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

    fn monitor_cause(&self, monitor_id: &str) -> HealthCause {
        match self.monitors.get(monitor_id) {
            Some(monitor) if monitor.watched => HealthCause {
                monitor_state: Some(monitor.state),
                ..blank_cause(BusinessServiceStatus::from_monitor_state(monitor.state))
            },
            _ => blank_cause(BusinessServiceStatus::Operational),
        }
    }
}

fn blank_cause(status: BusinessServiceStatus) -> HealthCause {
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
    use crate::{
        domain::business_service::{
            BusinessServiceDependency, BusinessServiceHealth, DependencyImpact,
        },
        shared::time::TimestampMicros,
    };

    fn service(id: &str, dependencies: Vec<BusinessServiceDependency>) -> BusinessService {
        BusinessService {
            id: Id(id.into()),
            org_id: Id("org".into()),
            name: id.into(),
            description: String::new(),
            dependencies,
            health: BusinessServiceHealth::unevaluated(),
            created_by: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn depends(
        kind: DependencyKind,
        target: &str,
        impact: DependencyImpact,
    ) -> BusinessServiceDependency {
        BusinessServiceDependency {
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
        let services = [service(
            "checkout",
            vec![depends(
                DependencyKind::TechnicalService,
                "Payments-API",
                DependencyImpact::Outage,
            )],
        )];

        let signals = HealthSignals {
            incidents: vec![
                incident("i1", "r1", Severity::Warning, &["payments-api"]),
                incident("i2", "r2", Severity::Critical, &["payments-api", "ledger"]),
                incident("i3", "r3", Severity::Critical, &["search"]),
            ],
            monitors: Vec::new(),
        };

        let evaluation = &evaluate(&services, &signals)[&Id("checkout".into())];
        assert_eq!(evaluation.status, BusinessServiceStatus::MajorOutage);
        assert_eq!(evaluation.causes.len(), 1);
        assert_eq!(evaluation.causes[0].incident_count, 2);
        assert_eq!(evaluation.causes[0].incidents[0].id, Id("i2".into()));
        assert_eq!(evaluation.causes[0].target, "Payments-API");
    }

    #[test]
    fn health_propagates_through_business_services_as_impacts_allow() {
        let services = [
            service(
                "storefront",
                vec![
                    depends(
                        DependencyKind::BusinessService,
                        "checkout",
                        DependencyImpact::Outage,
                    ),
                    depends(
                        DependencyKind::BusinessService,
                        "recommendations",
                        DependencyImpact::Degraded,
                    ),
                ],
            ),
            service(
                "checkout",
                vec![depends(
                    DependencyKind::SyntheticMonitor,
                    "m-checkout",
                    DependencyImpact::Outage,
                )],
            ),
            service(
                "recommendations",
                vec![depends(
                    DependencyKind::AlertRule,
                    "r-reco",
                    DependencyImpact::Outage,
                )],
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

        let health = evaluate(&services, &signals);
        assert_eq!(
            health[&Id("recommendations".into())].status,
            BusinessServiceStatus::MajorOutage
        );
        assert_eq!(
            health[&Id("checkout".into())].status,
            BusinessServiceStatus::Operational
        );

        let storefront = &health[&Id("storefront".into())];
        assert_eq!(
            storefront.status,
            BusinessServiceStatus::DegradedPerformance
        );
        assert_eq!(storefront.causes[0].kind, DependencyKind::BusinessService);
        assert_eq!(storefront.causes[0].target, "recommendations");
    }

    #[test]
    fn a_monitor_that_is_not_watched_or_only_flaky_passes_nothing_on() {
        let services = [service(
            "api",
            vec![
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
            ],
        )];

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

        let evaluation = &evaluate(&services, &signals)[&Id("api".into())];
        assert_eq!(evaluation.status, BusinessServiceStatus::Operational);
        assert!(evaluation.causes.is_empty());
    }

    #[test]
    fn a_cycle_ends_the_walk_instead_of_looping() {
        let services = [
            service(
                "a",
                vec![
                    depends(
                        DependencyKind::BusinessService,
                        "b",
                        DependencyImpact::Outage,
                    ),
                    depends(DependencyKind::AlertRule, "r", DependencyImpact::Outage),
                ],
            ),
            service(
                "b",
                vec![depends(
                    DependencyKind::BusinessService,
                    "a",
                    DependencyImpact::Outage,
                )],
            ),
        ];

        let signals = HealthSignals {
            incidents: vec![incident("i", "r", Severity::Error, &[])],
            monitors: Vec::new(),
        };

        let health = evaluate(&services, &signals);
        assert_eq!(
            health[&Id("a".into())].status,
            BusinessServiceStatus::PartialOutage
        );
        assert_eq!(health.len(), 2);
    }
}
