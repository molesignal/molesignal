// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Drills: incidents already resolved, investigated again by Mole Agent as if they were happening,
//! to see how well their root cause is found.
//!
//! A drill is an incident with a reference root cause: the root-cause section of the incident's
//! internal postmortem, or one written by hand. Each run of it is a hidden, unattended chat bound to
//! the incident and pinned to the time it happened, run as the person who starts it, with the tools
//! that read what people found later denied ([`DENIED_TOOLS`]). When the run ends, a model compares
//! the answer's root cause with the reference and scores it from 1 to 5 ([`grading`]). Runs are
//! kept: running every drill again after a change of model, prompt or tools shows whether
//! investigations got better.

pub mod grading;

use async_trait::async_trait;
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};

/// At most this many drills in an organization.
pub const MAX_DRILLS_PER_ORG: usize = 100;
/// Incidents resolved within this many days can become drills from their postmortem.
pub const CANDIDATE_DAYS: i64 = 180;
pub const MAX_REFERENCE_CHARS: usize = 4_000;
/// At most this many runs start at once.
pub const MAX_BATCH: usize = 50;
/// A run is stopped after this long, as an automation is.
pub const RUN_DEADLINE_SECS: u64 = 600;
/// Grading is tried this many times before the run is given up as failed.
pub const MAX_GRADING_ATTEMPTS: i32 = 3;

/// Tools a drill's investigation may not call: they read what people found and did after the
/// incident began (its notes, timeline, root-cause analyses, postmortems, tickets, approvals and
/// executions, later incidents like it), or write to the incident.
pub const DENIED_TOOLS: [&str; 18] = [
    "get_incident",
    "get_incident_rca",
    "get_incident_timeline",
    "get_incident_tickets",
    "get_similar_incidents",
    "draft_incident_postmortem",
    "list_agent_investigations",
    "get_agent_investigation",
    "list_agent_approvals",
    "get_agent_approval",
    "list_agent_executions",
    "get_agent_execution",
    "list_status_page_incidents",
    "get_status_page_incident",
    "draft_status_page_postmortem",
    "list_notification_deliveries",
    "get_notification_delivery",
    "list_audit_events",
];

/// Why a run ended without a score.
pub mod reasons {
    pub const BUDGET_EXHAUSTED: &str = "budget_exhausted";
    pub const NO_MODEL: &str = "no_model";
    pub const INCIDENT_GONE: &str = "incident_gone";
    pub const START_FAILED: &str = "start_failed";
    pub const RUN_FAILED: &str = "run_failed";
    pub const RUN_CANCELLED: &str = "run_cancelled";
    pub const RUN_INTERRUPTED: &str = "run_interrupted";
    pub const NO_ANSWER: &str = "no_answer";
    pub const GRADING_FAILED: &str = "grading_failed";
}

/// Where a drill's reference root cause comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrillSource {
    /// The root-cause section of the incident's internal postmortem.
    Postmortem,
    /// Written by the person who made the drill.
    Manual,
}

impl DrillSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Postmortem => "postmortem",
            Self::Manual => "manual",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "postmortem" => Ok(Self::Postmortem),
            "manual" => Ok(Self::Manual),
            other => Err(Error::invalid(format!("unknown drill source `{other}`"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrillRunStatus {
    /// The investigation runs, or has ended and waits to be graded.
    Running,
    /// A node is grading it.
    Grading,
    Done,
    Failed,
}

impl DrillRunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Grading => "grading",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "running" => Ok(Self::Running),
            "grading" => Ok(Self::Grading),
            "done" => Ok(Self::Done),
            "failed" => Ok(Self::Failed),
            other => Err(Error::invalid(format!(
                "unknown drill run status `{other}`"
            ))),
        }
    }

    pub const fn is_over(self) -> bool {
        matches!(self, Self::Done | Self::Failed)
    }
}

/// An incident to investigate again, with the root cause its answer is compared with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Drill {
    pub id: Id,
    pub org_id: Id,
    pub incident_id: Id,
    /// The incident's summary when the drill was made.
    pub title: String,
    pub reference: String,
    pub source: DrillSource,
    pub created_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// One run of a drill.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrillRun {
    pub id: Id,
    pub org_id: Id,
    pub drill_id: Id,
    /// The hidden chat it ran in, and its Run.
    pub chat_id: Option<Id>,
    pub run_id: Option<Id>,
    pub status: DrillRunStatus,
    /// 0 to 1, once graded.
    pub score: Option<f64>,
    /// The grading model's reason, in `language`.
    pub reasoning: String,
    /// Why it ended without a score ([`reasons`]), and what was said about it.
    pub reason: Option<String>,
    pub detail: Option<String>,
    /// The model that investigated, and the version of the prompt it answered with.
    pub model: Option<String>,
    pub prompt_version: Option<i32>,
    pub grader_model: Option<String>,
    /// The interface language of the person who started it.
    pub language: String,
    /// Runs started together share it.
    pub batch_id: Option<Id>,
    /// Grading attempts so far.
    pub attempts: i32,
    pub started_by: Id,
    pub created_at: TimestampMicros,
    pub finished_at: Option<TimestampMicros>,
    pub updated_at: TimestampMicros,
    /// Not looked at again before then: a grading node's lease, or a later try.
    #[serde(default, skip_serializing)]
    pub not_before: Option<TimestampMicros>,
}

impl DrillRun {
    /// Ends the run without a score.
    pub fn fail(&mut self, reason: &str, detail: Option<String>, now: TimestampMicros) {
        self.status = DrillRunStatus::Failed;
        self.reason = Some(reason.to_string());
        self.detail = detail;
        self.finished_at = Some(now);
        self.updated_at = now;
        self.not_before = None;
    }
}

/// A drill with how its runs went.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DrillSummary {
    #[serde(flatten)]
    pub drill: Drill,
    pub runs: i64,
    /// The mean score of its graded runs.
    pub mean_score: Option<f64>,
    pub last_run: Option<DrillRun>,
}

/// An incident a drill can be made from, as stored: the postmortem's whole body, which
/// [`grading::root_cause_of`] reads.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateRow {
    pub incident_id: Id,
    pub summary: String,
    pub severity: String,
    pub created_at: TimestampMicros,
    pub resolved_at: Option<TimestampMicros>,
    pub body: String,
    /// Whether a drill was already made from it.
    pub drilled: bool,
}

#[async_trait]
pub trait DrillRepository: Send + Sync {
    /// Makes a drill: refused past [`MAX_DRILLS_PER_ORG`], and a conflict when the incident already
    /// has one.
    async fn create_drill(&self, drill: &Drill) -> Result<()>;
    async fn get_drill(&self, org_id: &Id, id: &Id) -> Result<Drill>;
    /// The organization's drills, newest first, each with its runs counted and its last run.
    async fn list_drills(&self, org_id: &Id) -> Result<Vec<DrillSummary>>;
    /// Deletes a drill and its runs; whether there was one.
    async fn delete_drill(&self, org_id: &Id, id: &Id) -> Result<bool>;
    /// Incidents resolved since `since` that have an internal postmortem, newest first.
    async fn drill_candidates(
        &self,
        org_id: &Id,
        since: TimestampMicros,
        limit: i64,
    ) -> Result<Vec<CandidateRow>>;

    async fn create_drill_run(&self, run: &DrillRun) -> Result<()>;
    /// Writes a run's state back.
    async fn save_drill_run(&self, run: &DrillRun) -> Result<()>;
    /// A drill's runs, newest first.
    async fn list_drill_runs(
        &self,
        org_id: &Id,
        drill_id: &Id,
        limit: i64,
    ) -> Result<Vec<DrillRun>>;
    /// Claims runs to grade: those whose Run has ended, and those a node took and did not finish
    /// before `now`; each gets a lease until `lease_until` and one more attempt.
    async fn claim_drill_runs(
        &self,
        now: TimestampMicros,
        lease_until: TimestampMicros,
        limit: i64,
    ) -> Result<Vec<DrillRun>>;
}
