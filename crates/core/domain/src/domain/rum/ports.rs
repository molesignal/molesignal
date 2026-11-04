// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;

use super::RumAlertQuery;
use crate::{
    domain::intake::{IntakeBatch, IntakeResult, RawEvent},
    shared::{
        Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

/// Where RUM rows are written: the intake of the stream the batch names. The rows are already in
/// the shape of the stream; the intake only checks, stores and indexes them.
#[async_trait]
pub trait RumEventWriter: Send + Sync {
    async fn write(&self, batch: IntakeBatch) -> Result<IntakeResult>;
}

/// Keeps the issues the errors written to `rum_errors` belong to. Every error row arrives with the
/// id of its issue, worked out from the error alone; the tracker, which sees all the errors of an
/// organization on one node, knows what no single request can: which issues are new, which come
/// back, and how many an application may open in an hour.
#[async_trait]
pub trait RumIssueTracker: Send + Sync {
    /// Opens the issues of the rows that are new, notes the others as seen again, and moves a
    /// resolved issue seen in a newer version to regressed. A row of an application that has
    /// opened all the issues it may in the hour is given the id of the issue of the rest.
    ///
    /// The rows are written whatever happens here: a failure is logged and the rows keep the ids
    /// they came with.
    async fn track(&self, org_id: &Id, rows: &mut [RawEvent]);
}

/// Builds sessions out of views. What an SDK reports is views; the batches written to
/// `rum_sessions` carry the view rows as patches, and the intake of that stream hands them to the
/// merger, which answers with the session rows to write in their place.
#[async_trait]
pub trait RumSessionMerger: Send + Sync {
    /// One session row for each session the patches belong to.
    async fn merge(&self, org_id: &Id, patches: Vec<RawEvent>) -> Result<Vec<RawEvent>>;
}

/// A figure of RUM over a range, and how many rows it was made of.
#[derive(Debug, Clone, PartialEq)]
pub struct RumAlertReading {
    pub value: f64,
    /// The rows the figure came from: the sessions, the views, the requests. A count says how many
    /// it counted.
    pub samples: u64,
}

/// A session an incident is told by.
#[derive(Debug, Clone, PartialEq)]
pub struct RumAlertSample {
    pub session_id: String,
    /// When the thing that made it a sample happened.
    pub at: TimestampMicros,
    /// What the session had of the figure, in the unit of the figure.
    pub value: Option<f64>,
    /// The element, message or request that went with it.
    pub detail: Option<String>,
}

/// The figure at one bar of a series; `None` where it has no value there.
#[derive(Debug, Clone, PartialEq)]
pub struct RumAlertPoint {
    pub at: TimestampMicros,
    pub reading: Option<RumAlertReading>,
}

/// The RUM figures alert rules watch, read by the evaluator in place of a query. They are worked
/// out as the RUM pages work them out, so a rule, its preview and the page it was made from agree.
#[async_trait]
pub trait RumAlertSource: Send + Sync {
    /// The rule's figure over `range`. `None` where the figure has no value there: nothing was
    /// seen, or fewer rows than the rule needs, and no threshold matches.
    async fn read(
        &self,
        org_id: &Id,
        query: &RumAlertQuery,
        range: TimeRange,
    ) -> Result<Option<RumAlertReading>>;

    /// A few sessions that show the figure at its worst, newest first; none for a figure that is
    /// not about sessions.
    async fn samples(
        &self,
        org_id: &Id,
        query: &RumAlertQuery,
        range: TimeRange,
    ) -> Result<Vec<RumAlertSample>>;

    /// The figure over each stretch of `bar_micros` of `range`, oldest first.
    async fn series(
        &self,
        org_id: &Id,
        query: &RumAlertQuery,
        range: TimeRange,
        bar_micros: i64,
    ) -> Result<Vec<RumAlertPoint>>;
}
