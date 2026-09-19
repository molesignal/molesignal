// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    app::{alerting::AlertIncidentLifecycleSink, status_page::StatusPageService},
    domain::{
        alerting::{
            incident::{Incident, IncidentStatus},
            mute::{MuteRuleRepository, match_labels_for_incident},
        },
        status_page::{AutomationSourceKind, AutomationSourceObservation},
    },
    shared::Result,
};

pub struct StatusPageAlertIncidentSink {
    status_pages: Arc<StatusPageService>,
    mute_rules: Arc<dyn MuteRuleRepository>,
}

impl StatusPageAlertIncidentSink {
    pub fn new(
        status_pages: Arc<StatusPageService>,
        mute_rules: Arc<dyn MuteRuleRepository>,
    ) -> Self {
        Self {
            status_pages,
            mute_rules,
        }
    }
}

#[async_trait]
impl AlertIncidentLifecycleSink for StatusPageAlertIncidentSink {
    async fn on_incident_changed(&self, incident: &Incident) -> Result<()> {
        let active = matches!(
            incident.status,
            IncidentStatus::Open | IncidentStatus::Acknowledged
        );
        let mut labels = incident.labels.clone();
        labels.insert("incident_summary".to_string(), incident.summary.clone());
        labels.insert("alert_rule_id".to_string(), incident.rule_id.to_string());
        labels.insert(
            "incident_status".to_string(),
            incident.status.as_str().to_string(),
        );
        let mute_labels = match_labels_for_incident(&incident.labels, &incident.id);
        let observed_at = incident
            .resolved_at
            .or(incident.acknowledged_at)
            .unwrap_or(incident.created_at);
        let muted = self
            .mute_rules
            .list_enabled(&incident.org_id)
            .await?
            .into_iter()
            .any(|rule| rule.is_muting(&mute_labels, observed_at));
        let observation = AutomationSourceObservation {
            organization_id: incident.org_id.clone(),
            source_kind: AutomationSourceKind::AlertIncident,
            source_id: incident.rule_id.clone(),
            source_instance_id: incident.id.clone(),
            severity: incident.severity,
            labels,
            active,
            muted,
            observed_at,
        };
        if active && !muted {
            self.status_pages
                .observe_automation_source(observation)
                .await?;
        } else {
            self.status_pages
                .recover_automation_source(observation)
                .await?;
        }
        Ok(())
    }
}
