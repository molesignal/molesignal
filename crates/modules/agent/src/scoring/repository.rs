// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Storage for scoring data: evaluators and settings, queued scoring jobs, scores, pending reviews,
//! and Prompt checks.

use async_trait::async_trait;
use kernel::{Result, ids::Id, time::TimestampMicros};
use serde::Serialize;

use super::{
    Evaluator, JobStatus, NewJob, Review, ReviewDecision, ReviewStatus, RuleDimension, Score,
    ScoreJob, ScoreSubject, ScoreTarget, ScoringSettings, TargetSource, check::PromptCheck,
    drills::DrillRepository, gaps::TrajectoryRow, trajectory::Trajectory,
};

/// A target and all its scores: rule-based scoring first, then evaluators in scoring order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScoredTarget {
    #[serde(flatten)]
    pub subject: ScoreSubject,
    pub scores: Vec<Score>,
}

/// A review entry and its target (absent when the target's scores are deleted).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReviewItem {
    #[serde(flatten)]
    pub review: Review,
    pub subject: Option<ScoreSubject>,
}

/// Lists scored targets, newest first, excluding personal chats.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredFilter {
    pub since: TimestampMicros,
    pub source: Option<TargetSource>,
    /// Includes targets with at least one score below this value.
    pub below: Option<f64>,
    /// Continues listing after this target (earlier targets): creation time and target ID.
    pub before: Option<(TimestampMicros, String)>,
    pub limit: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewFilter {
    pub status: Option<ReviewStatus>,
    /// Continues listing after this entry (earlier entries): time of entering pending review and
    /// ID.
    pub before: Option<(TimestampMicros, Id)>,
    pub limit: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OverviewFilter {
    pub since: TimestampMicros,
    pub source: Option<TargetSource>,
}

/// Quality over a period of time.
/// Rule-based scoring covers every target; evaluators score only sampled targets.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityOverview {
    /// Scored targets.
    pub answers: i64,
    pub rules_mean: Option<f64>,
    /// Targets scored by evaluators.
    pub evaluated: i64,
    pub evaluator_mean: Option<f64>,
    pub thumbs_up: i64,
    pub thumbs_down: i64,
    pub pending_reviews: i64,
    pub daily: Vec<DailyQuality>,
    pub dimensions: Vec<DimensionMean>,
    pub sources: Vec<GroupQuality>,
    pub prompts: Vec<GroupQuality>,
    pub skills: Vec<GroupQuality>,
    pub models: Vec<GroupQuality>,
}

/// Quality for one day (UTC).
/// `day` is the number of days since 1970-01-01.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyQuality {
    pub day: i64,
    pub answers: i64,
    pub rules_mean: Option<f64>,
    pub evaluated: i64,
    pub evaluator_mean: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DimensionMean {
    pub dimension: RuleDimension,
    pub mean: f64,
    /// Targets with this dimension: answers without tool calls have no tool success rate.
    pub answers: i64,
}

/// Quality grouped by source, Prompt version, Skill, or model.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupQuality {
    pub key: String,
    /// The Prompt version or Skill version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub answers: i64,
    pub rules_mean: Option<f64>,
    pub evaluated: i64,
    pub evaluator_mean: Option<f64>,
}

/// Storage of everything quality keeps; drills ([`DrillRepository`]) are part of it.
#[async_trait]
pub trait ScoringRepository: DrillRepository + Send + Sync {
    async fn list_evaluators(&self, org_id: &Id) -> Result<Vec<Evaluator>>;
    async fn get_evaluator(&self, org_id: &Id, id: &Id) -> Result<Evaluator>;
    /// Creates an evaluator with a name unique within the organization; rejects creation if the
    /// count exceeds [`super::MAX_EVALUATORS_PER_ORG`].
    async fn create_evaluator(&self, evaluator: Evaluator) -> Result<Evaluator>;
    async fn update_evaluator(&self, evaluator: Evaluator) -> Result<Evaluator>;
    /// Deletes an evaluator, retaining its scores and invalidating its queued scoring jobs.
    async fn delete_evaluator(&self, org_id: &Id, id: &Id) -> Result<()>;

    /// The organization's settings, using defaults when none are set.
    async fn settings(&self, org_id: &Id) -> Result<ScoringSettings>;
    async fn put_settings(
        &self,
        org_id: &Id,
        settings: &ScoringSettings,
    ) -> Result<ScoringSettings>;

    /// Enqueues jobs, skipping any target and scorer pair already queued.
    /// Returns the number of newly queued jobs.
    async fn enqueue(&self, jobs: &[NewJob]) -> Result<u64>;
    /// Claims due scoring jobs, including those whose claiming node is gone and whose lease
    /// expires.
    async fn claim_jobs(
        &self,
        now: TimestampMicros,
        lease_until: TimestampMicros,
        limit: i64,
    ) -> Result<Vec<ScoreJob>>;
    async fn finish_job(&self, id: &Id, status: JobStatus, error: Option<&str>) -> Result<()>;
    /// Retries later after an error.
    async fn retry_job(&self, id: &Id, not_before: TimestampMicros, error: &str) -> Result<()>;
    /// An evaluator plans to score or has already scored this target.
    async fn evaluator_planned(
        &self,
        org_id: &Id,
        kind: ScoreTarget,
        target_id: &str,
    ) -> Result<bool>;
    /// Deletes scoring job records completed before `before`.
    async fn purge_jobs(&self, before: TimestampMicros) -> Result<u64>;

    /// Records the scored target, replacing it if present; record it before its scores.
    async fn put_subject(&self, subject: &ScoreSubject) -> Result<()>;
    /// Saves a target's score from a scorer, replacing it if present.
    async fn put_score(&self, score: &Score) -> Result<()>;
    /// Keeps what the target's tool calls say about its data with its subject; `None` clears it
    /// (an answer that called no tool, an incident RCA). Record the subject first.
    async fn put_trajectory(
        &self,
        org_id: &Id,
        kind: ScoreTarget,
        target_id: &str,
        trajectory: Option<&Trajectory>,
    ) -> Result<()>;
    /// Answers with a trajectory produced since `since`, newest first, at most `limit`.
    async fn trajectories(
        &self,
        org_id: &Id,
        since: TimestampMicros,
        limit: i64,
    ) -> Result<Vec<TrajectoryRow>>;
    /// A target and all its scores, or `None` if it has not been scored.
    async fn scored(
        &self,
        org_id: &Id,
        kind: ScoreTarget,
        target_id: &str,
    ) -> Result<Option<ScoredTarget>>;
    async fn list_scored(&self, org_id: &Id, filter: &ScoredFilter) -> Result<Vec<ScoredTarget>>;
    async fn overview(&self, org_id: &Id, filter: &OverviewFilter) -> Result<QualityOverview>;
    /// Targets recently scored by this evaluator, newest first.
    async fn recent_evaluated(
        &self,
        org_id: &Id,
        evaluator: &Id,
        sources: &[TargetSource],
        limit: i64,
    ) -> Result<Vec<ScoredTarget>>;

    /// Opens a review; does nothing and returns `false` if the target already has a review entry,
    /// whether reviewed or not.
    async fn open_review(&self, review: &Review) -> Result<bool>;
    async fn get_review(&self, org_id: &Id, id: &Id) -> Result<Review>;
    async fn review_of(
        &self,
        org_id: &Id,
        kind: ScoreTarget,
        target_id: &str,
    ) -> Result<Option<Review>>;
    async fn list_reviews(&self, org_id: &Id, filter: &ReviewFilter) -> Result<Vec<ReviewItem>>;
    async fn decide_review(
        &self,
        org_id: &Id,
        id: &Id,
        decision: &ReviewDecision,
    ) -> Result<Review>;
    /// Accepted review entries, newest first: the regression set.
    async fn regression_set(
        &self,
        org_id: &Id,
        sources: &[TargetSource],
        limit: i64,
    ) -> Result<Vec<Review>>;

    async fn create_check(&self, check: &PromptCheck) -> Result<()>;
    async fn get_check(&self, org_id: &Id, id: &Id) -> Result<PromptCheck>;
    /// The most recent check for a Prompt.
    async fn latest_check(&self, org_id: &Id, prompt_id: &Id) -> Result<Option<PromptCheck>>;
    /// Claims a queued check, including one whose claiming node is gone and whose lease expires.
    async fn claim_check(
        &self,
        now: TimestampMicros,
        lease_until: TimestampMicros,
    ) -> Result<Option<PromptCheck>>;
    /// Records progress or results and renews the lease.
    async fn save_check(&self, check: &PromptCheck, lease_until: TimestampMicros) -> Result<()>;
}
