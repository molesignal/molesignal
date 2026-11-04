// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Components that depend on technical services, synthetic monitors and alert
//! rules. The health worked out from those reaches a component in follow mode
//! as its status, and the automation rules of its page as an observation. The
//! health is written by compare-and-set, so only the evaluation that records a
//! change acts on it.

mod signals;

use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

pub use signals::{DependencyAlertSink, DependencySignalQueue, DependencySyntheticSink};

use super::StatusPageService;
use crate::{
    domain::{
        alerting::{
            incident::Incident,
            mute::{MuteRuleRepository, match_labels_for_incident},
            repositories::AlertRuleRepository,
        },
        status_page::{
            ActiveAutomationRule, AutomationSourceKind, AutomationSourceObservation,
            ComponentDependency, ComponentKind, ComponentLifecycle, ComponentStatus,
            DependencyHealth, DependencyKind, DependencyMode, DependencyStatus, HealthSignals,
            HealthWrite, IncidentSignal, MonitorSignal, StatusPageComponent,
            StatusPageComponentStatusEvent, evaluate_dependencies, normalize_dependencies,
        },
        synthetics::{MonitorLifecycle, SyntheticRepository},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// The labels an alert carries the name of its service under.
const SERVICE_LABELS: [&str; 3] = ["service", "service.name", "service_name"];

/// What component dependencies are read from, and the queue that asks for
/// them to be worked out again.
pub struct DependencySources {
    pub alert_rules: Arc<dyn AlertRuleRepository>,
    pub mute_rules: Arc<dyn MuteRuleRepository>,
    pub synthetics: Arc<dyn SyntheticRepository>,
    pub signals: Arc<DependencySignalQueue>,
}

impl StatusPageService {
    /// The queue that asks for component dependencies to be worked out again.
    pub fn dependency_signals(&self) -> Option<&Arc<DependencySignalQueue>> {
        self.dependency_sources
            .as_ref()
            .map(|sources| &sources.signals)
    }

    /// The organizations with components that depend on something.
    pub async fn dependency_organizations(&self) -> Result<Vec<Id>> {
        self.repository.list_dependency_organizations().await
    }

    /// Works out the health of the organization's components from their
    /// dependencies and acts on every change. Returns how many changed.
    pub async fn evaluate_component_dependencies(&self, org_id: &Id) -> Result<u32> {
        let components = self.repository.list_dependent_components(org_id).await?;

        if components.is_empty() {
            return Ok(0);
        }

        let signals = self.dependency_health_signals(org_id).await?;
        let now = TimestampMicros::now();
        let mut changed = 0;

        for component in &components {
            let evaluation = evaluate_dependencies(&component.dependencies, &signals);

            let after = DependencyHealth::after(
                component.dependency_health.as_ref(),
                evaluation.status,
                evaluation.causes,
                now,
            );

            let write = self
                .repository
                .record_dependency_health(component, &after)
                .await?;

            if write == HealthWrite::Changed {
                changed += 1;
                self.act_on_dependency_change(component, &after, now).await;
            }
        }

        Ok(changed)
    }

    /// The dependencies as a component keeps them. A group has none of its
    /// own, and a monitor or alert rule named must be one of the organization's.
    pub(super) async fn component_dependencies(
        &self,
        org_id: &Id,
        kind: ComponentKind,
        dependencies: Vec<ComponentDependency>,
    ) -> Result<Vec<ComponentDependency>> {
        let dependencies = normalize_dependencies(dependencies)?;

        if dependencies.is_empty() {
            return Ok(dependencies);
        }

        if kind == ComponentKind::Group {
            return Err(Error::invalid(
                "a group depends on nothing of its own; give its components dependencies",
            ));
        }

        let sources = self.sources()?;
        let named = |kind| {
            dependencies
                .iter()
                .any(|dependency| dependency.kind == kind)
        };

        let monitors: HashSet<String> = if named(DependencyKind::SyntheticMonitor) {
            sources
                .synthetics
                .list_monitors(org_id)
                .await?
                .into_iter()
                .map(|monitor| monitor.id.0)
                .collect()
        } else {
            HashSet::new()
        };

        let rules: HashSet<String> = if named(DependencyKind::AlertRule) {
            sources
                .alert_rules
                .list(org_id)
                .await?
                .into_iter()
                .map(|rule| rule.id.0)
                .collect()
        } else {
            HashSet::new()
        };

        for dependency in &dependencies {
            let exists = match dependency.kind {
                DependencyKind::TechnicalService => true,
                DependencyKind::SyntheticMonitor => monitors.contains(&dependency.target),
                DependencyKind::AlertRule => rules.contains(&dependency.target),
            };

            if !exists {
                return Err(Error::not_found(format!(
                    "{} {} not found",
                    dependency.kind.as_str().replace('_', " "),
                    dependency.target
                )));
            }
        }

        Ok(dependencies)
    }

    /// Asks for the organization's component dependencies to be worked out
    /// again, as they were just changed.
    pub(super) fn request_dependency_evaluation(&self, org_id: &Id) {
        if let Some(signals) = self.dependency_signals() {
            signals.request(org_id);
        }
    }

    /// The components a matching automation rule publishes on: the ones it
    /// names, or for a dependency rule that names none, the component whose
    /// dependencies changed, when it is on the rule's page and in automation
    /// mode.
    pub(in crate::app::status_page) async fn rule_components(
        &self,
        rule: &ActiveAutomationRule,
        observation: &AutomationSourceObservation,
    ) -> Result<Vec<Id>> {
        let named = &rule.revision.action.component_ids;

        if !named.is_empty()
            || observation.source_kind != AutomationSourceKind::ComponentDependencies
        {
            return Ok(named.clone());
        }

        let component = self
            .repository
            .get_component(
                &observation.organization_id,
                &rule.rule.status_page_id,
                &observation.source_id,
            )
            .await;

        Ok(match component {
            Ok(component)
                if component.lifecycle == ComponentLifecycle::Active
                    && component.dependency_mode == DependencyMode::Automation =>
            {
                vec![component.id]
            }
            _ => Vec::new(),
        })
    }

    fn sources(&self) -> Result<&DependencySources> {
        self.dependency_sources
            .as_ref()
            .ok_or_else(|| Error::unavailable("component dependencies are not configured"))
    }

    /// What is going on in the organization: its open alert incidents that are
    /// not muted, and the state of its monitors.
    async fn dependency_health_signals(&self, org_id: &Id) -> Result<HealthSignals> {
        let sources = self.sources()?;
        let now = TimestampMicros::now();
        let mute_rules = sources.mute_rules.list_enabled(org_id).await?;

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

        let monitors = sources
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

    /// Passes a change on: a component in follow mode takes the status, and
    /// the automation rules of its page hear of it as an observation. A
    /// failure is logged; the next change, or the periodic sweep, passes the
    /// health on again.
    async fn act_on_dependency_change(
        &self,
        component: &StatusPageComponent,
        after: &DependencyHealth,
        at: TimestampMicros,
    ) {
        if component.dependency_mode == DependencyMode::Follow
            && let Err(error) = self
                .follow_dependencies(component.clone(), after.status.component_status())
                .await
        {
            tracing::warn!(
                component_id = %component.id,
                error = %error,
                "status-page component could not follow its dependencies"
            );
        }

        // A component found operational the first time has nothing to recover from.
        if component.dependency_health.is_none() && after.status == DependencyStatus::Operational {
            return;
        }

        let observation = observation(component, after, at);

        let result = if observation.active {
            self.observe_automation_source(observation)
                .await
                .map(|_| ())
        } else {
            self.recover_automation_source(observation)
                .await
                .map(|_| ())
        };

        if let Err(error) = result {
            tracing::warn!(
                component_id = %component.id,
                error = %error,
                "status-page automation could not take a dependency change"
            );
        }
    }

    async fn follow_dependencies(
        &self,
        component: StatusPageComponent,
        status: ComponentStatus,
    ) -> Result<StatusPageComponent> {
        if component.status == status || component.lifecycle == ComponentLifecycle::Archived {
            return Ok(component);
        }

        let now = TimestampMicros::now();
        let expected_status = component.status;

        let event = StatusPageComponentStatusEvent {
            id: Id::new(),
            org_id: component.org_id.clone(),
            status_page_id: component.status_page_id.clone(),
            component_id: component.id.clone(),
            status,
            started_at: now,
            ended_at: None,
            created_at: now,
            notify_subscribers: component.dependency_notify,
        };

        self.repository
            .update_component(
                StatusPageComponent {
                    status,
                    updated_at: now,
                    ..component
                },
                expected_status,
                Some(event),
            )
            .await
    }
}

/// The change as status-page automation sees it. Templates can name the
/// component and its statuses with `{{labels.component_name}}`,
/// `{{labels.dependency_status}}` and `{{labels.previous_dependency_status}}`.
fn observation(
    component: &StatusPageComponent,
    after: &DependencyHealth,
    at: TimestampMicros,
) -> AutomationSourceObservation {
    let mut labels = BTreeMap::new();
    labels.insert("component_name".to_string(), component.name.clone());

    labels.insert(
        "dependency_status".to_string(),
        after.status.as_str().to_string(),
    );

    let previous = component
        .dependency_health
        .as_ref()
        .map_or(DependencyStatus::Operational, |health| health.status);

    labels.insert(
        "previous_dependency_status".to_string(),
        previous.as_str().to_string(),
    );

    AutomationSourceObservation {
        organization_id: component.org_id.clone(),
        source_kind: AutomationSourceKind::ComponentDependencies,
        source_id: component.id.clone(),
        source_instance_id: component.id.clone(),
        severity: after.status.severity(),
        labels,
        active: after.status != DependencyStatus::Operational,
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
    use crate::domain::{
        alerting::incident::Severity,
        status_page::{ComponentVisibility, DependencyImpact},
    };

    fn checkout(health: Option<DependencyHealth>) -> StatusPageComponent {
        StatusPageComponent {
            id: Id("checkout".into()),
            org_id: Id("org".into()),
            status_page_id: Id("page".into()),
            name: "Checkout".into(),
            description: String::new(),
            status: ComponentStatus::Operational,
            visibility: ComponentVisibility::Enabled,
            lifecycle: ComponentLifecycle::Active,
            position: 0,
            archived_at: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
            dependencies: vec![ComponentDependency {
                kind: DependencyKind::TechnicalService,
                target: "checkout-api".into(),
                impact: DependencyImpact::Outage,
            }],
            dependency_mode: DependencyMode::Automation,
            dependency_notify: true,
            dependency_health: health,
            kind: ComponentKind::Component,
            group_id: None,
        }
    }

    #[test]
    fn an_outage_is_an_active_observation_and_recovery_an_inactive_one() {
        let before = DependencyHealth::after(
            None,
            DependencyStatus::Operational,
            Vec::new(),
            TimestampMicros(1),
        );

        let outage = DependencyHealth::after(
            Some(&before),
            DependencyStatus::PartialOutage,
            Vec::new(),
            TimestampMicros(2),
        );
        let observed = observation(&checkout(Some(before)), &outage, TimestampMicros(2));

        assert!(observed.active);
        assert_eq!(observed.severity, Severity::Error);
        assert_eq!(
            observed.source_kind,
            AutomationSourceKind::ComponentDependencies
        );
        assert_eq!(observed.source_id, Id("checkout".into()));
        assert_eq!(observed.labels["component_name"], "Checkout");
        assert_eq!(observed.labels["previous_dependency_status"], "operational");

        let recovered = DependencyHealth::after(
            Some(&outage),
            DependencyStatus::Operational,
            Vec::new(),
            TimestampMicros(3),
        );
        let observed = observation(&checkout(Some(outage)), &recovered, TimestampMicros(3));
        assert!(!observed.active);
        assert_eq!(
            observed.labels["previous_dependency_status"],
            "partial_outage"
        );
    }
}
