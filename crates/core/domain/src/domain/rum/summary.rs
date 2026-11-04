// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a model made of a session: a few sentences and the chapters of its replay, in the language
//! of whoever asked, kept so that nobody pays for the same answer twice.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Result, ids::Id, time::TimestampMicros};

/// One stretch of a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RumSummaryChapter {
    /// When it begins, in microseconds since the epoch.
    pub at_time: i64,
    pub title: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RumSessionSummary {
    pub org_id: Id,
    pub application_id: String,
    pub session_id: String,
    pub language: String,
    pub summary: String,
    pub chapters: Vec<RumSummaryChapter>,
    pub model: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    /// What the model was told, as a hash: the same events are not asked about again.
    pub input_digest: String,
    pub generated_by: Option<Id>,
    pub generated_at: TimestampMicros,
}

#[async_trait]
pub trait RumSessionSummaryRepository: Send + Sync {
    async fn get(
        &self,
        org_id: &Id,
        session_id: &str,
        language: &str,
    ) -> Result<Option<RumSessionSummary>>;

    /// The newest summary of a session, in whichever language it was asked for.
    async fn latest(&self, org_id: &Id, session_id: &str) -> Result<Option<RumSessionSummary>>;

    /// Keeps a summary, in place of the one a session had in that language.
    async fn put(&self, summary: &RumSessionSummary) -> Result<()>;

    /// Removes up to `limit` summaries older than `cutoff`, as the replays they are about go, and
    /// says how many.
    async fn delete_older_than(&self, cutoff: TimestampMicros, limit: usize) -> Result<usize>;
}
