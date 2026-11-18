// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Feedback on Mole Agent output: 👍 / 👎, reasons for 👎, and comments.
//!
//! Each person has one entry per target, which they can edit or withdraw. Feedback records the
//! model and prompt used for the rated answer, so later quality comparisons by version do not
//! require looking them up again.

use async_trait::async_trait;
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};

/// The maximum number of characters in a comment.
pub const MAX_COMMENT_CHARS: usize = 2_000;

/// The maximum number of targets whose feedback can be looked up at once.
pub const MAX_TARGETS_PER_LOOKUP: usize = 200;

/// What the feedback is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackTarget {
    /// An answer in a conversation; the target ID is the message ID.
    ChatMessage,
    /// An incident's root cause analysis; the target ID is the incident ID.
    IncidentRca,
}

impl FeedbackTarget {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ChatMessage => "chat_message",
            Self::IncidentRca => "incident_rca",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "chat_message" => Ok(Self::ChatMessage),
            "incident_rca" => Ok(Self::IncidentRca),
            other => Err(Error::invalid(format!("unknown feedback target `{other}`"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackRating {
    Up,
    Down,
}

impl FeedbackRating {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            other => Err(Error::invalid(format!("unknown feedback rating `{other}`"))),
        }
    }
}

/// Reasons for 👎; multiple selections are allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackReason {
    /// The conclusion or root cause is wrong.
    WrongConclusion,
    /// Fails to identify the problem.
    MissedCause,
    /// The suggested remediation is unusable.
    UnusableAdvice,
    /// Does not answer the question.
    OffTopic,
    Other,
}

impl FeedbackReason {
    pub const ALL: [Self; 5] = [
        Self::WrongConclusion,
        Self::MissedCause,
        Self::UnusableAdvice,
        Self::OffTopic,
        Self::Other,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WrongConclusion => "wrong_conclusion",
            Self::MissedCause => "missed_cause",
            Self::UnusableAdvice => "unusable_advice",
            Self::OffTopic => "off_topic",
            Self::Other => "other",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|reason| reason.as_str() == value)
            .ok_or_else(|| Error::invalid(format!("unknown feedback reason `{value}`")))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentFeedback {
    pub id: Id,
    pub org_id: Id,
    pub target_kind: FeedbackTarget,
    pub target_id: String,
    pub user_id: Id,
    pub rating: FeedbackRating,
    pub reasons: Vec<FeedbackReason>,
    pub comment: String,
    /// The model service, model, and prompt version used for the rated answer.
    pub provenance: serde_json::Value,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl AgentFeedback {
    /// Normalizes and validates feedback: deduplicates and sorts reasons, removes reasons for 👍,
    /// and checks the comment length limit.
    pub fn normalized(mut self) -> Result<Self> {
        if self.target_id.trim().is_empty() {
            return Err(Error::invalid("feedback target id is required"));
        }

        self.comment = self.comment.trim().to_string();

        if self.comment.chars().count() > MAX_COMMENT_CHARS {
            return Err(Error::invalid(format!(
                "feedback comment must be at most {MAX_COMMENT_CHARS} characters"
            )));
        }

        if self.rating == FeedbackRating::Up {
            self.reasons.clear();
        }

        self.reasons.sort_unstable();
        self.reasons.dedup();

        Ok(self)
    }
}

#[async_trait]
pub trait AgentFeedbackRepository: Send + Sync {
    /// Saves a person's feedback on a target; replaces the rating, reasons, and comment if it
    /// exists, preserving the creation time.
    async fn upsert(&self, feedback: AgentFeedback) -> Result<AgentFeedback>;

    /// Withdraws a person's feedback on a target; returns `false` if none exists.
    async fn remove(
        &self,
        org_id: &Id,
        target_kind: FeedbackTarget,
        target_id: &str,
        user_id: &Id,
    ) -> Result<bool>;

    /// Everyone's feedback on a target, most recently edited first.
    async fn list_for_target(
        &self,
        org_id: &Id,
        target_kind: FeedbackTarget,
        target_id: &str,
    ) -> Result<Vec<AgentFeedback>>;

    /// A person's feedback on these targets.
    async fn list_for_user(
        &self,
        org_id: &Id,
        target_kind: FeedbackTarget,
        target_ids: &[String],
        user_id: &Id,
    ) -> Result<Vec<AgentFeedback>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feedback(
        rating: FeedbackRating,
        reasons: Vec<FeedbackReason>,
        comment: &str,
    ) -> AgentFeedback {
        AgentFeedback {
            id: Id("f1".into()),
            org_id: Id("org".into()),
            target_kind: FeedbackTarget::ChatMessage,
            target_id: "m1".into(),
            user_id: Id("u1".into()),
            rating,
            reasons,
            comment: comment.into(),
            provenance: serde_json::Value::Null,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        }
    }

    #[test]
    fn names_round_trip() {
        for target in [FeedbackTarget::ChatMessage, FeedbackTarget::IncidentRca] {
            assert_eq!(FeedbackTarget::parse(target.as_str()).unwrap(), target);
        }

        for rating in [FeedbackRating::Up, FeedbackRating::Down] {
            assert_eq!(FeedbackRating::parse(rating.as_str()).unwrap(), rating);
        }

        for reason in FeedbackReason::ALL {
            assert_eq!(FeedbackReason::parse(reason.as_str()).unwrap(), reason);
        }

        assert!(FeedbackReason::parse("too_slow").is_err());
    }

    #[test]
    fn down_reasons_are_deduplicated_and_up_carries_none() {
        let down = feedback(
            FeedbackRating::Down,
            vec![
                FeedbackReason::OffTopic,
                FeedbackReason::WrongConclusion,
                FeedbackReason::OffTopic,
            ],
            "  the cause was the database  ",
        )
        .normalized()
        .unwrap();

        assert_eq!(
            down.reasons,
            vec![FeedbackReason::WrongConclusion, FeedbackReason::OffTopic]
        );
        assert_eq!(down.comment, "the cause was the database");

        let up = feedback(FeedbackRating::Up, vec![FeedbackReason::Other], "")
            .normalized()
            .unwrap();

        assert!(up.reasons.is_empty());
    }

    #[test]
    fn long_comments_and_missing_targets_are_rejected() {
        let long = "x".repeat(MAX_COMMENT_CHARS + 1);
        assert!(
            feedback(FeedbackRating::Down, vec![], &long)
                .normalized()
                .is_err()
        );

        let mut missing = feedback(FeedbackRating::Up, vec![], "");
        missing.target_id = " ".into();
        assert!(missing.normalized().is_err());
    }
}
