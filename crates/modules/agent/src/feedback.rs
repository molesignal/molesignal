// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 对 Mole Agent 产出的评价：👍 / 👎、👎 的原因与备注。
//!
//! 每人对每个对象只留一条，可以改、可以撤回。评价时记下被评价的回答当时用的模型与
//! prompt，之后按版本对比质量时不必再回头找。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

/// 备注最多这么多字。
pub const MAX_COMMENT_CHARS: usize = 2_000;

/// 一次最多查这么多个对象的评价。
pub const MAX_TARGETS_PER_LOOKUP: usize = 200;

/// 被评价的是什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackTarget {
    /// 对话里的一条回答，对象 ID 是消息 ID。
    ChatMessage,
    /// 事件的根因分析，对象 ID 是事件 ID。
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

/// 👎 的原因，可以多选。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackReason {
    /// 结论或根因是错的。
    WrongConclusion,
    /// 没有定位到问题。
    MissedCause,
    /// 给出的处置建议用不上。
    UnusableAdvice,
    /// 答非所问。
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
pub struct AgentFeedback {
    pub id: Id,
    pub org_id: Id,
    pub target_kind: FeedbackTarget,
    pub target_id: String,
    pub user_id: Id,
    pub rating: FeedbackRating,
    pub reasons: Vec<FeedbackReason>,
    pub comment: String,
    /// 被评价的回答当时用的模型服务、模型与 prompt 版本。
    pub provenance: serde_json::Value,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl AgentFeedback {
    /// 整理并检查一条评价：原因去重排序，👍 不带原因，备注不超长。
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
    /// 保存一个人对一个对象的评价；已有时替换评分、原因与备注，保留创建时间。
    async fn upsert(&self, feedback: AgentFeedback) -> Result<AgentFeedback>;

    /// 撤回一个人对一个对象的评价；原本没有时返回 `false`。
    async fn remove(
        &self,
        org_id: &Id,
        target_kind: FeedbackTarget,
        target_id: &str,
        user_id: &Id,
    ) -> Result<bool>;

    /// 一个人对这些对象给过的评价。
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
