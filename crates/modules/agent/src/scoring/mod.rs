// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 产出的质量：打分、待审与 Prompt 晋级检查。
//!
//! 一个回答结束、一份事件 RCA 生成、一条评价提交之后排队打分：
//!
//! - 规则打分（[`rules`]）不调模型：工具成功率、回答引用的证据、查询能否解析、回答格式、是否
//!   完成，以及提问人的 👍 / 👎。
//! - 评审（[`evaluation`]）是组织配置的打分器：按比例抽样，用组织的模型照评分标准给
//!   1 到 5 分并写一句理由；被评为没帮助的必评，低分的再评一次取平均。
//!
//! 分数存进 Postgres，并写进 `_agent_score_traces`。分数低于阈值的、被评为没帮助的与随机抽到的
//! 进入待审，审核人采纳或驳回；采纳的成为回归集。把一个组织 Prompt 设为默认之前，用回归集与
//! 最近评过的回答检查新版本有没有变差（[`check`]）。
//!
//! 个人对话只做规则打分，不进评审、待审与回归集：除了本人，谁都读不到它们。

pub mod check;
mod config;
pub mod evaluation;
mod queue;
mod repository;
pub mod rules;

pub use config::{
    Evaluator, GateMode, MAX_EVALUATOR_NAME_CHARS, MAX_EVALUATORS_PER_ORG, MAX_RUBRIC_CHARS,
    ScoringSettings,
};
pub use queue::{
    JobReason, JobStatus, MAX_JOB_ATTEMPTS, MAX_REVIEW_NOTE_CHARS, NewJob, Review, ReviewCause,
    ReviewDecision, ReviewStatus, ScoreJob,
};
pub use repository::{
    DailyQuality, DimensionMean, GroupQuality, OverviewFilter, QualityOverview, ReviewFilter,
    ReviewItem, ScoredFilter, ScoredTarget, ScoringRepository,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

/// 规则打分在分数里的打分器名；评审用自己的 ID。
pub const RULES_SCORER: &str = "rules";

/// 分数写进的流。
pub const SCORE_STREAM: &str = "_agent_score_traces";

/// 被打分的对象。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreTarget {
    /// 对话里的一条回答，对象 ID 是消息 ID；自动化的运行也是它的会话里的回答。
    ChatAnswer,
    /// 事件的 RCA，对象 ID 是事件 ID。
    IncidentRca,
}

impl ScoreTarget {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ChatAnswer => "chat_answer",
            Self::IncidentRca => "incident_rca",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "chat_answer" => Ok(Self::ChatAnswer),
            "incident_rca" => Ok(Self::IncidentRca),
            other => Err(Error::invalid(format!("unknown score target `{other}`"))),
        }
    }
}

/// 对象出自哪里：评审按它挑对象，页面按它筛选。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetSource {
    PersonalChat,
    TeamChat,
    /// 自动化运行的回答，组织、团队与个人的规则都算。
    Automation,
    IncidentRca,
}

impl TargetSource {
    pub const ALL: [Self; 4] = [
        Self::PersonalChat,
        Self::TeamChat,
        Self::Automation,
        Self::IncidentRca,
    ];

    /// 评审能评的：个人对话除外。
    pub const EVALUATED: [Self; 3] = [Self::TeamChat, Self::Automation, Self::IncidentRca];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PersonalChat => "personal_chat",
            Self::TeamChat => "team_chat",
            Self::Automation => "automation",
            Self::IncidentRca => "incident_rca",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|source| source.as_str() == value)
            .ok_or_else(|| Error::invalid(format!("unknown score source `{value}`")))
    }

    /// 只有本人读得到：不进评审、待审与回归集，列表里也不出现。
    pub const fn is_private(self) -> bool {
        matches!(self, Self::PersonalChat)
    }

    pub const fn target(self) -> ScoreTarget {
        match self {
            Self::IncidentRca => ScoreTarget::IncidentRca,
            _ => ScoreTarget::ChatAnswer,
        }
    }
}

/// 规则打分的维度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleDimension {
    /// 没出错的工具调用占多少。
    ToolSuccess,
    /// 回答引用了证据，并且有成功的工具调用撑着。
    Evidence,
    /// 工具调用里的 SQL 与 PromQL 能解析。
    QueryValidity,
    /// 回答是要求的 JSON 形状。
    AnswerFormat,
    /// 这一轮做完了。
    Completion,
    /// 提问人的 👍 / 👎。
    Feedback,
}

impl RuleDimension {
    pub const ALL: [Self; 6] = [
        Self::ToolSuccess,
        Self::Evidence,
        Self::QueryValidity,
        Self::AnswerFormat,
        Self::Completion,
        Self::Feedback,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ToolSuccess => "tool_success",
            Self::Evidence => "evidence",
            Self::QueryValidity => "query_validity",
            Self::AnswerFormat => "answer_format",
            Self::Completion => "completion",
            Self::Feedback => "feedback",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|dimension| dimension.as_str() == value)
    }
}

/// 规则打分的一个维度：0 到 1 的分数，和算出它的事实（各维度的键见 [`rules`]）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DimensionScore {
    pub dimension: RuleDimension,
    pub score: f64,
    #[serde(default)]
    pub detail: Value,
}

/// 对象出自哪一轮、用了什么：按 Prompt 版本、模型与 Skill 看分数。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attribution {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<Id>,
    /// 提问的人；事件 RCA 没有。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_template_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_version: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// 这一轮用斜杠命令调用的 Skill：名称、版本。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_version: Option<String>,
}

/// 被打分的对象：出自哪里、那一轮用了什么，以及列表里显示的问题与回答的开头。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreSubject {
    pub org_id: Id,
    pub target_kind: ScoreTarget,
    pub target_id: String,
    pub source: TargetSource,
    /// 问题与回答的开头；个人对话不记。
    #[serde(default)]
    pub question: String,
    #[serde(default)]
    pub answer: String,
    pub attribution: Attribution,
    /// 对象产生的时间。
    pub target_at: TimestampMicros,
}

/// 一个对象在一个打分器下的分数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Score {
    pub id: Id,
    pub org_id: Id,
    pub target_kind: ScoreTarget,
    pub target_id: String,
    /// [`RULES_SCORER`]，或评审的 ID。
    pub scorer: String,
    /// 0 到 1。
    pub score: f64,
    /// 规则打分的各个维度；评审没有。
    #[serde(default)]
    pub dimensions: Vec<DimensionScore>,
    /// 评审的理由；规则打分为空，页面按维度的事实写说明。
    #[serde(default)]
    pub reason: String,
    /// 评审评了几次，分数是平均：低分会再评一次。
    pub samples: i32,
    /// 评审用的模型。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluator_model: Option<String>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl Score {
    pub fn is_rules(&self) -> bool {
        self.scorer == RULES_SCORER
    }

    /// 某个维度的分数。
    pub fn dimension(&self, dimension: RuleDimension) -> Option<f64> {
        self.dimensions
            .iter()
            .find(|candidate| candidate.dimension == dimension)
            .map(|candidate| candidate.score)
    }
}

/// 一个对象的分数：有评审评过时取评审的平均，否则取规则打分。
pub fn overall(scores: &[Score]) -> Option<f64> {
    let evaluated = scores
        .iter()
        .filter(|score| !score.is_rules())
        .map(|score| score.score)
        .collect::<Vec<_>>();

    if !evaluated.is_empty() {
        return Some(evaluated.iter().sum::<f64>() / evaluated.len() as f64);
    }

    scores
        .iter()
        .find(|score| score.is_rules())
        .map(|score| score.score)
}

/// 由 `seed` 决定的一个 0 到 99 的数：同一个对象、同一个打分器每次抽到的都一样。
pub fn draw(seed: &str) -> u8 {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(seed.as_bytes());
    let mut first = [0u8; 8];
    first.copy_from_slice(&digest[..8]);

    (u64::from_be_bytes(first) % 100) as u8
}

/// 按比例抽样：`percent` 为 0 时一个也不抽，100 时都抽。
pub fn sampled(seed: &str, percent: u8) -> bool {
    percent > 0 && draw(seed) < percent.min(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(scorer: &str, value: f64) -> Score {
        Score {
            id: Id::new(),
            org_id: Id::from_string("org"),
            target_kind: ScoreTarget::ChatAnswer,
            target_id: "m1".into(),
            scorer: scorer.into(),
            score: value,
            dimensions: Vec::new(),
            reason: String::new(),
            samples: 1,
            evaluator_model: None,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        }
    }

    #[test]
    fn names_round_trip() {
        for source in TargetSource::ALL {
            assert_eq!(TargetSource::parse(source.as_str()).unwrap(), source);
        }

        for target in [ScoreTarget::ChatAnswer, ScoreTarget::IncidentRca] {
            assert_eq!(ScoreTarget::parse(target.as_str()).unwrap(), target);
        }

        for dimension in RuleDimension::ALL {
            assert_eq!(RuleDimension::parse(dimension.as_str()), Some(dimension));
        }

        assert!(TargetSource::parse("dm").is_err());
        assert!(TargetSource::PersonalChat.is_private());
        assert!(!TargetSource::EVALUATED.contains(&TargetSource::PersonalChat));
    }

    #[test]
    fn an_evaluated_target_is_scored_by_its_evaluators() {
        assert_eq!(overall(&[score(RULES_SCORER, 0.9)]), Some(0.9));
        assert_eq!(
            overall(&[
                score(RULES_SCORER, 0.9),
                score("j1", 0.25),
                score("j2", 0.75)
            ]),
            Some(0.5)
        );
        assert_eq!(overall(&[]), None);
    }

    #[test]
    fn sampling_is_stable_and_bounded() {
        assert_eq!(draw("m1:j1"), draw("m1:j1"));
        assert!(!sampled("m1:j1", 0));
        assert!(sampled("m1:j1", 100));

        let hits = (0..1_000)
            .filter(|index| sampled(&format!("m{index}:j1"), 20))
            .count();

        assert!((120..=280).contains(&hits), "{hits}");
    }
}
