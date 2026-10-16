// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The organizations whose business services should be worked out again: an
//! alert incident or a synthetic monitor of theirs changed, or their catalog
//! did. A burst of changes is worked out once.

use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use tokio::sync::Notify;

use crate::{
    app::{alerting::AlertIncidentLifecycleSink, synthetics::SyntheticTransitionSink},
    domain::{alerting::incident::Incident, synthetics::SyntheticStateTransition},
    shared::{Result, ids::Id},
};

#[derive(Default)]
pub struct BusinessServiceSignalQueue {
    organizations: Mutex<BTreeSet<String>>,
    wake: Notify,
}

impl BusinessServiceSignalQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request(&self, org_id: &Id) {
        if let Ok(mut organizations) = self.organizations.lock() {
            organizations.insert(org_id.0.clone());
        }

        self.wake.notify_one();
    }

    /// Waits until something is requested, or `at_most`.
    pub async fn wait(&self, at_most: Duration) {
        let _ = tokio::time::timeout(at_most, self.wake.notified()).await;
    }

    /// The organizations requested since the last call.
    pub fn take(&self) -> Vec<Id> {
        self.organizations
            .lock()
            .map(|mut organizations| std::mem::take(&mut *organizations))
            .unwrap_or_default()
            .into_iter()
            .map(Id)
            .collect()
    }
}

/// Asks for the business services of an organization to be worked out again
/// when one of its alert incidents changes.
pub struct BusinessServiceAlertSink {
    signals: Arc<BusinessServiceSignalQueue>,
}

impl BusinessServiceAlertSink {
    pub fn new(signals: Arc<BusinessServiceSignalQueue>) -> Self {
        Self { signals }
    }
}

#[async_trait]
impl AlertIncidentLifecycleSink for BusinessServiceAlertSink {
    async fn on_incident_changed(&self, incident: &Incident) -> Result<()> {
        self.signals.request(&incident.org_id);
        Ok(())
    }
}

/// Asks for the business services of an organization to be worked out again
/// when one of its synthetic monitors changes state.
pub struct BusinessServiceSyntheticSink {
    signals: Arc<BusinessServiceSignalQueue>,
}

impl BusinessServiceSyntheticSink {
    pub fn new(signals: Arc<BusinessServiceSignalQueue>) -> Self {
        Self { signals }
    }
}

#[async_trait]
impl SyntheticTransitionSink for BusinessServiceSyntheticSink {
    async fn on_transition(
        &self,
        transition: &SyntheticStateTransition,
        _alert_on_degraded: bool,
        _alert_on_flaky: bool,
    ) -> Result<()> {
        self.signals.request(&transition.organization_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_burst_of_requests_is_taken_once() {
        let signals = BusinessServiceSignalQueue::new();
        signals.request(&Id("org-a".into()));
        signals.request(&Id("org-a".into()));
        signals.request(&Id("org-b".into()));

        signals.wait(Duration::from_millis(1)).await;
        assert_eq!(signals.take(), [Id("org-a".into()), Id("org-b".into())]);
        assert!(signals.take().is_empty());
    }
}
