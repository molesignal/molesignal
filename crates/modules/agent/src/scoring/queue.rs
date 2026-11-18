// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Queued scoring jobs and pending reviews.

use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};

use super::{ScoreTarget, TargetSource};

/// The maximum number of attempts for a scoring job.
pub const MAX_JOB_ATTEMPTS: i32 = 3;
/// The maximum number of characters in a review note.
pub const MAX_REVIEW_NOTE_CHARS: usize = 2_000;

/// The reason for queuing this scoring job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobReason {
    /// A new answer or RCA.
    Answered,
    /// The asker rates it: refresh feedback in rule-based scoring; an unhelpful rating requires
    /// evaluator scoring.
    Feedback,
    /// An evaluator gives a low score, so score it again and take the average.
    Resample,
}

impl JobReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Answered => "answered",
            Self::Feedback => "feedback",
            Self::Resample => "resample",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "answered" => Ok(Self::Answered),
            "feedback" => Ok(Self::Feedback),
            "resample" => Ok(Self::Resample),
            other => Err(Error::invalid(format!(
                "unknown score job reason `{other}`"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    /// Scoring is skipped: the target is gone, the evaluator is disabled, or today's token budget
    /// is exhausted.
    Skipped,
    Failed,
}

impl JobStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Done => "done",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "done" => Ok(Self::Done),
            "skipped" => Ok(Self::Skipped),
            "failed" => Ok(Self::Failed),
            other => Err(Error::invalid(format!(
                "unknown score job status `{other}`"
            ))),
        }
    }
}

/// A scoring job to queue.
/// Only one job per target and scorer is queued at a time.
#[derive(Debug, Clone, PartialEq)]
pub struct NewJob {
    pub org_id: Id,
    pub target_kind: ScoreTarget,
    pub target_id: String,
    /// The session containing the chat answer.
    pub chat_id: Option<Id>,
    pub scorer: String,
    pub reason: JobReason,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScoreJob {
    pub id: Id,
    pub org_id: Id,
    pub target_kind: ScoreTarget,
    pub target_id: String,
    pub chat_id: Option<Id>,
    pub scorer: String,
    pub reason: JobReason,
    pub status: JobStatus,
    /// The number of times claimed, including this time.
    pub attempts: i32,
    pub created_at: TimestampMicros,
}

/// The reason for entering pending review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewCause {
    /// The score is below the threshold.
    LowScore,
    /// The asker rates it as unhelpful.
    ThumbsDown,
    /// Randomly sampled to calibrate the evaluator.
    Sampled,
}

impl ReviewCause {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LowScore => "low_score",
            Self::ThumbsDown => "thumbs_down",
            Self::Sampled => "sampled",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "low_score" => Ok(Self::LowScore),
            "thumbs_down" => Ok(Self::ThumbsDown),
            "sampled" => Ok(Self::Sampled),
            other => Err(Error::invalid(format!("unknown review cause `{other}`"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    Pending,
    /// The reviewer confirms that the answer has a problem: it enters the regression set.
    Accepted,
    /// The reviewer considers the answer correct.
    Rejected,
}

impl ReviewStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "accepted" => Ok(Self::Accepted),
            "rejected" => Ok(Self::Rejected),
            other => Err(Error::invalid(format!("unknown review status `{other}`"))),
        }
    }
}

/// A target pending review.
/// Each target enters only once; subsequent reasons do not create another entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub id: Id,
    pub org_id: Id,
    pub target_kind: ScoreTarget,
    pub target_id: String,
    pub source: TargetSource,
    pub cause: ReviewCause,
    /// The score when the target enters pending review.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    pub status: ReviewStatus,
    /// The reviewer's notes on what is wrong with the answer and how it should be answered.
    /// Provided to the evaluator for reference during Prompt checks.
    #[serde(default)]
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// The reviewer's decision.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewDecision {
    pub status: ReviewStatus,
    pub note: String,
    pub by: Id,
    pub at: TimestampMicros,
}

impl ReviewDecision {
    pub fn normalized(mut self) -> Result<Self> {
        if self.status == ReviewStatus::Pending {
            return Err(Error::invalid("a decision accepts or rejects the answer"));
        }

        self.note = self.note.trim().to_string();

        if self.note.chars().count() > MAX_REVIEW_NOTE_CHARS {
            return Err(Error::invalid(format!(
                "a review note must be at most {MAX_REVIEW_NOTE_CHARS} characters"
            )));
        }

        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for reason in [
            JobReason::Answered,
            JobReason::Feedback,
            JobReason::Resample,
        ] {
            assert_eq!(JobReason::parse(reason.as_str()).unwrap(), reason);
        }

        for status in [
            JobStatus::Queued,
            JobStatus::Running,
            JobStatus::Done,
            JobStatus::Skipped,
            JobStatus::Failed,
        ] {
            assert_eq!(JobStatus::parse(status.as_str()).unwrap(), status);
        }

        for cause in [
            ReviewCause::LowScore,
            ReviewCause::ThumbsDown,
            ReviewCause::Sampled,
        ] {
            assert_eq!(ReviewCause::parse(cause.as_str()).unwrap(), cause);
        }

        for status in [
            ReviewStatus::Pending,
            ReviewStatus::Accepted,
            ReviewStatus::Rejected,
        ] {
            assert_eq!(ReviewStatus::parse(status.as_str()).unwrap(), status);
        }
    }

    #[test]
    fn a_decision_settles_the_review_with_a_short_note() {
        let decision = |status, note: &str| ReviewDecision {
            status,
            note: note.into(),
            by: Id::from_string("u1"),
            at: TimestampMicros(1),
        };

        let accepted = decision(ReviewStatus::Accepted, "  It blamed the cache  ")
            .normalized()
            .unwrap();
        assert_eq!(accepted.note, "It blamed the cache");

        assert!(decision(ReviewStatus::Pending, "").normalized().is_err());
        assert!(
            decision(
                ReviewStatus::Rejected,
                &"x".repeat(MAX_REVIEW_NOTE_CHARS + 1)
            )
            .normalized()
            .is_err()
        );
    }
}
