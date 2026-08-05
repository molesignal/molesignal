// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;

use super::{DashboardDraft, DraftConsumption};
use crate::{
    domain::dashboard::Dashboard,
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct ConsumeDashboardDraft {
    pub org_id: Id,
    pub actor: Id,
    pub draft_id: Id,
    pub expected_hash: String,
    pub compiler_version: String,
    pub now: TimestampMicros,
    pub dashboard: Dashboard,
}

#[async_trait]
pub trait DashboardDraftRepository: Send + Sync {
    async fn create(&self, draft: DashboardDraft) -> Result<DashboardDraft>;

    /// Lookup is always scoped to the trusted organization and lazily expires stale rows.
    async fn get(&self, org_id: &Id, draft_id: &Id, now: TimestampMicros)
    -> Result<DashboardDraft>;

    /// Atomically inserts the Dashboard and consumes a ready draft, returning the existing
    /// Dashboard when the draft has already been consumed.
    async fn consume_and_create(&self, request: ConsumeDashboardDraft) -> Result<DraftConsumption>;
}
