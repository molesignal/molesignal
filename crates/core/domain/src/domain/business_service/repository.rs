// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;

use super::{BusinessService, BusinessServiceHealth};
use crate::shared::{Result, ids::Id, time::TimestampMicros};

/// What recording an evaluation did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthWrite {
    /// The status is the one stored; causes and the evaluation time were updated.
    Unchanged,
    /// The status changed through this write.
    Changed,
    /// Another evaluation changed the status first; nothing was written.
    Lost,
}

#[async_trait]
pub trait BusinessServiceRepository: Send + Sync {
    async fn create_business_service(&self, service: BusinessService) -> Result<BusinessService>;

    /// Saves the name, description and dependencies; the health is left alone.
    async fn update_business_service(&self, service: BusinessService) -> Result<BusinessService>;

    /// Unlinks the status-page components that stand for the service, drops it
    /// from the dependencies of other services and deletes it, in one
    /// transaction.
    async fn delete_business_service(
        &self,
        org_id: &Id,
        id: &Id,
        updated_at: TimestampMicros,
    ) -> Result<()>;

    async fn get_business_service(&self, org_id: &Id, id: &Id) -> Result<BusinessService>;
    async fn list_business_services(&self, org_id: &Id) -> Result<Vec<BusinessService>>;

    /// The organizations that have business services.
    async fn list_business_service_organizations(&self) -> Result<Vec<Id>>;

    /// Records an evaluation. A changed status is written only while the
    /// stored status and its change time are still those of `before`.
    async fn record_business_service_health(
        &self,
        org_id: &Id,
        id: &Id,
        before: &BusinessServiceHealth,
        after: &BusinessServiceHealth,
    ) -> Result<HealthWrite>;
}
