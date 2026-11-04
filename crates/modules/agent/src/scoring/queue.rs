// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 排队的打分与待审。

use serde::{Deserialize, Serialize};

use super::{ScoreTarget, TargetSource};
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

/// 一次打分最多试这么多次。
pub const MAX_JOB_ATTEMPTS: i32 = 3;
/// 审核备注最多这么多字。
pub const MAX_REVIEW_NOTE_CHARS: usize = 2_000;

/// 为什么排这一次打分。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobReason {
    /// 新的回答或 RCA。
    Answered,
    /// 提问人评价了它：刷新规则打分里的评价；被评为没帮助时评审必评。
    Feedback,
    /// 评审给了低分，再评一次取平均。
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
    /// 没有评：对象没了、评审停用了，或今天的 token 预算用完了。
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

/// 要排的一次打分。同一个对象、同一个打分器同时只排一次。
#[derive(Debug, Clone, PartialEq)]
pub struct NewJob {
    pub org_id: Id,
    pub target_kind: ScoreTarget,
    pub target_id: String,
    /// 对话回答所在的会话。
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
    /// 领走过几次，含这一次。
    pub attempts: i32,
    pub created_at: TimestampMicros,
}

/// 为什么进入待审。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewCause {
    /// 分数低于阈值。
    LowScore,
    /// 提问人评为没帮助。
    ThumbsDown,
    /// 随机抽到，用来校准评审。
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
    /// 审核人确认这个回答有问题：它进入回归集。
    Accepted,
    /// 审核人认为回答没有问题。
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

/// 待审的一个对象。一个对象只进一次，后来的原因不再另起一条。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub id: Id,
    pub org_id: Id,
    pub target_kind: ScoreTarget,
    pub target_id: String,
    pub source: TargetSource,
    pub cause: ReviewCause,
    /// 进入待审时的分数。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    pub status: ReviewStatus,
    /// 审核人写的：回答错在哪、应该怎么答。检查 Prompt 时交给评审参考。
    #[serde(default)]
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// 审核人的决定。
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
