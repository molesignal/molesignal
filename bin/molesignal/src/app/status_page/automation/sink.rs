// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;

use crate::{
    app::{status_page::StatusPageService, synthetics::SyntheticTransitionSink},
    domain::{
        alerting::incident::Severity,
        status_page::{AutomationSourceKind, AutomationSourceObservation},
        synthetics::{MonitorState, SyntheticStateTransition},
    },
    shared::Result,
};

pub struct StatusPageSyntheticTransitionSink {
    status_pages: Arc<StatusPageService>,
}

impl StatusPageSyntheticTransitionSink {
    pub fn new(status_pages: Arc<StatusPageService>) -> Self {
        Self { status_pages }
    }
}

#[async_trait]
impl SyntheticTransitionSink for StatusPageSyntheticTransitionSink {
    async fn on_transition(
        &self,
        transition: &SyntheticStateTransition,
        alert_on_degraded: bool,
        alert_on_flaky: bool,
    ) -> Result<()> {
        let Some((active, severity)) =
            transition_activation(transition.current_state, alert_on_degraded, alert_on_flaky)
        else {
            return Ok(());
        };
        let mut labels = BTreeMap::new();
        labels.insert(
            "synthetic_state".to_string(),
            transition.current_state.as_str().to_string(),
        );
        labels.insert(
            "previous_synthetic_state".to_string(),
            transition.previous_state.as_str().to_string(),
        );
        let observation = AutomationSourceObservation {
            organization_id: transition.organization_id.clone(),
            source_kind: AutomationSourceKind::SyntheticMonitor,
            source_id: transition.monitor_id.clone(),
            source_instance_id: transition.monitor_id.clone(),
            severity,
            labels,
            active,
            muted: false,
            observed_at: transition.created_at,
        };
        if active {
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

fn transition_activation(
    state: MonitorState,
    alert_on_degraded: bool,
    alert_on_flaky: bool,
) -> Option<(bool, Severity)> {
    match state {
        MonitorState::Healthy => Some((false, Severity::Info)),
        MonitorState::Flaky if alert_on_flaky => Some((true, Severity::Warning)),
        MonitorState::Flaky => Some((false, Severity::Info)),
        MonitorState::Degraded if alert_on_degraded => Some((true, Severity::Warning)),
        MonitorState::Degraded => Some((false, Severity::Info)),
        MonitorState::Failing => Some((true, Severity::Error)),
        MonitorState::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use super::transition_activation;
    use crate::domain::synthetics::MonitorState;

    #[test]
    fn degraded_activation_follows_revision_setting() {
        assert!(
            transition_activation(MonitorState::Degraded, true, false)
                .unwrap()
                .0
        );
        assert!(
            !transition_activation(MonitorState::Degraded, false, false)
                .unwrap()
                .0
        );
        assert!(
            transition_activation(MonitorState::Failing, false, false)
                .unwrap()
                .0
        );
        assert!(
            transition_activation(MonitorState::Flaky, false, true)
                .unwrap()
                .0
        );
        assert!(
            !transition_activation(MonitorState::Flaky, false, false)
                .unwrap()
                .0
        );
    }
}
