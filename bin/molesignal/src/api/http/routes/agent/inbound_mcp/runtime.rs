// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use dashmap::DashMap;
use rmcp::model::RequestStateCodec;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use super::admission::{AdmissionGuard, AdmissionRejection, CredentialAdmission};

#[derive(Debug, Clone, Copy)]
pub(super) enum CatalogEvent {
    Tools,
    Prompts,
    Resources,
}

pub(super) struct InboundMcpAdapterRuntime {
    admissions: DashMap<String, Arc<CredentialAdmission>>,
    admission_requests: AtomicU64,
    pub task_cancellations: DashMap<String, CancellationToken>,
    pub request_state: RequestStateCodec,
    events: DashMap<String, broadcast::Sender<CatalogEvent>>,
}

impl InboundMcpAdapterRuntime {
    pub fn new(request_state_key: [u8; 32]) -> Self {
        Self {
            admissions: DashMap::new(),
            admission_requests: AtomicU64::new(0),
            task_cancellations: DashMap::new(),
            request_state: RequestStateCodec::new(request_state_key),
            events: DashMap::new(),
        }
    }

    pub fn admit(
        &self,
        credential_key: String,
        max_concurrent: u32,
        calls_per_minute: usize,
    ) -> Result<AdmissionGuard, AdmissionRejection> {
        let admission = self
            .admissions
            .entry(credential_key)
            .or_insert_with(|| Arc::new(CredentialAdmission::default()))
            .clone();
        let guard = admission.enter(max_concurrent, calls_per_minute)?;
        if self
            .admission_requests
            .fetch_add(1, Ordering::Relaxed)
            .is_multiple_of(1_024)
        {
            let now = Instant::now();
            self.admissions
                .retain(|_, admission| !admission.is_idle_at(now));
        }
        Ok(guard)
    }

    pub fn notify_catalog_changed(&self, org_id: &crate::shared::ids::Id) {
        let sender = self.sender(org_id);
        let _ = sender.send(CatalogEvent::Tools);
        let _ = sender.send(CatalogEvent::Prompts);
        let _ = sender.send(CatalogEvent::Resources);
    }

    pub fn subscribe(&self, org_id: &crate::shared::ids::Id) -> broadcast::Receiver<CatalogEvent> {
        self.sender(org_id).subscribe()
    }

    fn sender(&self, org_id: &crate::shared::ids::Id) -> broadcast::Sender<CatalogEvent> {
        self.events
            .entry(org_id.0.clone())
            .or_insert_with(|| broadcast::channel(128).0)
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::shared::ids::Id;

    #[tokio::test]
    async fn catalog_notifications_are_isolated_by_organization() {
        let runtime = InboundMcpAdapterRuntime::new([7; 32]);
        let org_a = Id::from_string("org-a");
        let org_b = Id::from_string("org-b");
        let mut receiver = runtime.subscribe(&org_a);

        runtime.notify_catalog_changed(&org_b);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), receiver.recv())
                .await
                .is_err()
        );

        runtime.notify_catalog_changed(&org_a);
        assert!(matches!(receiver.recv().await, Ok(CatalogEvent::Tools)));
    }

    #[test]
    fn admission_is_shared_by_credential_key() {
        let runtime = InboundMcpAdapterRuntime::new([7; 32]);
        let first = runtime.admit("org:token".into(), 1, 60).unwrap();
        assert!(matches!(
            runtime.admit("org:token".into(), 1, 60),
            Err(AdmissionRejection::Concurrent)
        ));
        drop(first);
        assert!(runtime.admit("org:token".into(), 1, 60).is_ok());
    }
}
