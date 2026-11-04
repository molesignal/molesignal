// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a status page component depends on — technical services, synthetic
//! monitors and alert rules — and the health that follows from them. A
//! component follows that health, or hands its changes to the automation rules
//! of its page.

mod evaluation;
mod model;

use async_trait::async_trait;
pub use evaluation::{Evaluation, HealthSignals, IncidentSignal, MonitorSignal, evaluate};
pub use model::*;

use super::StatusPageComponent;
use crate::shared::{Result, ids::Id};

#[async_trait]
pub trait StatusPageDependencyRepository: Send + Sync {
    /// The organizations with components that depend on something.
    async fn list_dependency_organizations(&self) -> Result<Vec<Id>>;

    /// The active components that depend on something, on the organization's
    /// pages that are not archived.
    async fn list_dependent_components(&self, org_id: &Id) -> Result<Vec<StatusPageComponent>>;

    /// Records an evaluation of the component's dependencies as they were
    /// read. A changed status is written only while the stored health is still
    /// `before`, and nothing is written once the dependencies have changed.
    async fn record_dependency_health(
        &self,
        component: &StatusPageComponent,
        after: &DependencyHealth,
    ) -> Result<HealthWrite>;
}
