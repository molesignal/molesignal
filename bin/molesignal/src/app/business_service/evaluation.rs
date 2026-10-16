// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Working out the health of an organization's business services and acting
//! on what changed. The status is written by compare-and-set, so only the
//! evaluation that records a change acts on it.

use std::collections::BTreeMap;

use super::BusinessServiceService;
use crate::{
    domain::{
        alerting::{incident::Incident, mute::match_labels_for_incident},
        business_service::{
            BusinessService, BusinessServiceHealth, BusinessServiceStatus, HealthSignals,
            HealthWrite, IncidentSignal, MonitorSignal, evaluate,
        },
        status_page::{AutomationSourceKind, AutomationSourceObservation},
        synthetics::MonitorLifecycle,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// The labels an alert carries the name of its service under.
const SERVICE_LABELS: [&str; 3] = ["service", "service.name", "service_name"];

impl BusinessServiceService {
    /// The organizations that have business services.
    pub async fn organizations(&self) -> Result<Vec<Id>> {
        self.repository.list_business_service_organizations().await
    }

    /// Works out the health of the organization's business services and acts
    /// on every change. Returns how many changed.
    pub async fn evaluate_organization(&self, org_id: &Id) -> Result<u32> {
        let services = self.repository.list_business_services(org_id).await?;

        if services.is_empty() {
            return Ok(0);
        }

        let signals = self.health_signals(org_id).await?;
        let evaluations = evaluate(&services, &signals);
        let now = TimestampMicros::now();
        let mut changed = 0;

        for service in &services {
            let Some(evaluation) = evaluations.get(&service.id) else {
                continue;
            };

            let after = service
                .health
                .evaluated(evaluation.status, evaluation.causes.clone(), now);

            let write = self
                .repository
                .record_business_service_health(org_id, &service.id, &service.health, &after)
                .await?;

            if write == HealthWrite::Changed {
                changed += 1;
                self.act_on_change(service, &after, now).await;
            }
        }

        Ok(changed)
    }

    /// What is going on in the organization: its open alert incidents that are
    /// not muted, and the state of its monitors.
    async fn health_signals(&self, org_id: &Id) -> Result<HealthSignals> {
        let now = TimestampMicros::now();
        let mute_rules = self.mute_rules.list_enabled(org_id).await?;

        let incidents = self
            .incidents
            .list_active(org_id)
            .await?
            .into_iter()
            .filter(|incident| {
                let labels = match_labels_for_incident(&incident.labels, &incident.id);
                !mute_rules.iter().any(|rule| rule.is_muting(&labels, now))
            })
            .map(|incident| IncidentSignal {
                services: incident_services(&incident),
                id: incident.id,
                rule_id: incident.rule_id,
                severity: incident.severity,
                summary: incident.summary,
            })
            .collect();

        let monitors = self
            .synthetics
            .list_monitors(org_id)
            .await?
            .into_iter()
            .map(|monitor| MonitorSignal {
                id: monitor.id,
                state: monitor.state,
                watched: monitor.lifecycle == MonitorLifecycle::Active,
            })
            .collect();

        Ok(HealthSignals {
            incidents,
            monitors,
        })
    }

    /// Passes a change on to the components that stand for the service: the
    /// ones that follow it take the status, and the automation rules of the
    /// pages hear of it as an observation. A failure is logged; the next
    /// change, or the periodic sweep, passes the health on again.
    async fn act_on_change(
        &self,
        service: &BusinessService,
        after: &BusinessServiceHealth,
        at: TimestampMicros,
    ) {
        let first = service.health.evaluated_at.is_none();

        if let Err(error) = self
            .status_pages
            .apply_business_service_health(&service.org_id, &service.id, after.status)
            .await
        {
            tracing::warn!(
                business_service_id = %service.id,
                error = %error,
                "components could not follow their business service"
            );
        }

        // A service found operational the first time has nothing to recover from.
        if first && after.status == BusinessServiceStatus::Operational {
            return;
        }

        let observation = observation(service, after, at);

        let result = if observation.active {
            self.status_pages
                .observe_automation_source(observation)
                .await
                .map(|_| ())
        } else {
            self.status_pages
                .recover_automation_source(observation)
                .await
                .map(|_| ())
        };

        if let Err(error) = result {
            tracing::warn!(
                business_service_id = %service.id,
                error = %error,
                "status-page automation could not take a business service change"
            );
        }
    }
}

/// The change as status-page automation sees it. Templates can name the
/// service and its statuses with `{{labels.business_service_name}}`,
/// `{{labels.business_service_status}}` and
/// `{{labels.previous_business_service_status}}`.
fn observation(
    service: &BusinessService,
    after: &BusinessServiceHealth,
    at: TimestampMicros,
) -> AutomationSourceObservation {
    let mut labels = BTreeMap::new();
    labels.insert("business_service_name".to_string(), service.name.clone());

    labels.insert(
        "business_service_status".to_string(),
        after.status.as_str().to_string(),
    );

    let previous = if service.health.evaluated_at.is_some() {
        service.health.status
    } else {
        BusinessServiceStatus::Operational
    };

    labels.insert(
        "previous_business_service_status".to_string(),
        previous.as_str().to_string(),
    );

    AutomationSourceObservation {
        organization_id: service.org_id.clone(),
        source_kind: AutomationSourceKind::BusinessService,
        source_id: service.id.clone(),
        source_instance_id: service.id.clone(),
        severity: after.status.severity(),
        labels,
        active: after.status != BusinessServiceStatus::Operational,
        muted: false,
        observed_at: at,
    }
}

/// The services an alert incident is about: the service its labels name and
/// the services its query saw.
fn incident_services(incident: &Incident) -> Vec<String> {
    SERVICE_LABELS
        .iter()
        .filter_map(|label| incident.labels.get(*label))
        .chain(incident.affected_services.iter())
        .map(|service| service.trim().to_string())
        .filter(|service| !service.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::alerting::incident::Severity;

    fn checkout(health: BusinessServiceHealth) -> BusinessService {
        BusinessService {
            id: Id("checkout".into()),
            org_id: Id("org".into()),
            name: "Checkout".into(),
            description: String::new(),
            dependencies: Vec::new(),
            health,
            created_by: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn an_outage_is_an_active_observation_and_recovery_an_inactive_one() {
        let before = BusinessServiceHealth::unevaluated().evaluated(
            BusinessServiceStatus::Operational,
            Vec::new(),
            TimestampMicros(1),
        );

        let outage = before.evaluated(
            BusinessServiceStatus::PartialOutage,
            Vec::new(),
            TimestampMicros(2),
        );
        let observed = observation(&checkout(before.clone()), &outage, TimestampMicros(2));

        assert!(observed.active);
        assert_eq!(observed.severity, Severity::Error);
        assert_eq!(observed.source_kind, AutomationSourceKind::BusinessService);
        assert_eq!(observed.labels["business_service_name"], "Checkout");
        assert_eq!(
            observed.labels["previous_business_service_status"],
            "operational"
        );

        let recovered = outage.evaluated(
            BusinessServiceStatus::Operational,
            Vec::new(),
            TimestampMicros(3),
        );
        let observed = observation(&checkout(outage), &recovered, TimestampMicros(3));
        assert!(!observed.active);
        assert_eq!(
            observed.labels["previous_business_service_status"],
            "partial_outage"
        );
    }
}
