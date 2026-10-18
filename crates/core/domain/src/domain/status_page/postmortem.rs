// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The public postmortem of a resolved incident.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{StatusPageIncident, StatusPagePublicationState};
use crate::shared::{Result, ids::Id, time::TimestampMicros};

/// What the customers of a resolved incident are told about why it happened
/// and what changes because of it. The body is Markdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusPagePostmortem {
    pub body: String,
    pub publication_state: StatusPagePublicationState,
    /// When it was published; cleared when it goes back to a draft.
    pub published_at: Option<TimestampMicros>,
    /// When the published text last changed.
    pub revised_at: Option<TimestampMicros>,
    pub updated_by: Option<Id>,
    pub updated_at: TimestampMicros,
}

/// How a postmortem is saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostmortemWrite {
    /// Keep it a draft, or take a published one back to a draft.
    Draft,
    /// Write a draft only while nothing is published, refused otherwise in the
    /// same transaction: for writers that must not change the public page.
    DraftOnly,
    /// Publish it, or revise the published text. Subscribers are told only
    /// when `notify_subscribers` is set.
    Publish { notify_subscribers: bool },
}

#[async_trait]
pub trait StatusPagePostmortemRepository: Send + Sync {
    /// Saves the body and the state in one transaction. A publication that
    /// tells subscribers queues their deliveries in the same transaction.
    #[allow(clippy::too_many_arguments)]
    async fn save_postmortem(
        &self,
        org_id: &Id,
        page_id: &Id,
        incident_id: &Id,
        body: &str,
        write: PostmortemWrite,
        actor_id: Option<&Id>,
        now: TimestampMicros,
    ) -> Result<StatusPageIncident>;

    /// Deletes a draft. A published postmortem goes back to a draft first.
    async fn delete_postmortem_draft(
        &self,
        org_id: &Id,
        page_id: &Id,
        incident_id: &Id,
    ) -> Result<StatusPageIncident>;
}
