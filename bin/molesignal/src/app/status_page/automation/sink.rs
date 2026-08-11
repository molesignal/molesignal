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
    async fn on_transition(&self, transition: &SyntheticStateTransition) -> Result<()> {
        let (active, severity) = match transition.current_state {
            MonitorState::Healthy => (false, Severity::Info),
            MonitorState::Degraded => (true, Severity::Warning),
            MonitorState::Failing => (true, Severity::Error),
            MonitorState::Unknown => return Ok(()),
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
