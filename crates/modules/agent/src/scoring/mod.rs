// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Quality of Mole Agent output: scoring, pending review, and Prompt promotion checks.
//!
//! Scoring is queued after an answer completes, an incident RCA is generated, or feedback is
//! submitted:
//!
//! - Rule-based scoring ([`rules`]) makes no model calls: tool success rate, evidence cited in the
//!   answer, query parseability, answer format, completion, and the asker's 👍 / 👎.
//! - Evaluators ([`evaluation`]) are organization-configured scorers: they sample targets and use
//!   the organization's model to score them from 1 to 5 against a rubric with a one-sentence
//!   reason; targets rated unhelpful are always evaluated, and low scores are evaluated again and
//!   averaged.
//!
//! Scores are stored in Postgres and written to `_agent_score_traces`. Targets with scores below
//! the threshold, targets rated unhelpful, and randomly selected targets enter pending review,
//! where a reviewer accepts or rejects them; accepted targets form the regression set. Before an
//! organization Prompt is set as the default, the regression set and recently evaluated answers are
//! used to check the new version for regressions ([`check`]).
//!
//! Personal conversations receive only rule-based scoring and do not enter evaluation, pending
//! review, or the regression set: only their owner can read them.
//!
//! Apart from scores, rule-based scoring keeps each answer's trajectory ([`trajectory`]): what its
//! tool calls say about the data it had to work with, summed up over a period as data gaps
//! ([`gaps`]). Drills ([`drills`]) investigate resolved incidents again and grade the root cause
//! found against the one people established.

pub mod check;
mod config;
pub mod drills;
pub mod evaluation;
pub mod gaps;
mod queue;
mod repository;
pub mod rules;
pub mod trajectory;

pub use config::{
    Evaluator, GateMode, MAX_EVALUATOR_NAME_CHARS, MAX_EVALUATORS_PER_ORG, MAX_RUBRIC_CHARS,
    ScoringSettings,
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
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
pub use trajectory::Trajectory;

/// The scorer name for rule-based scores; evaluators use their own IDs.
pub const RULES_SCORER: &str = "rules";

/// The stream to which scores are written.
pub const SCORE_STREAM: &str = "_agent_score_traces";

/// The target being scored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreTarget {
    /// An answer in a conversation, with the message ID as its target ID; an automation run is also
    /// an answer in its session.
    ChatAnswer,
    /// An incident's RCA, with the incident ID as its target ID.
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

/// The target's source, used by evaluators to select targets and by the page to filter them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetSource {
    PersonalChat,
    TeamChat,
    /// Answers from automation runs, including organization, team, and personal rules.
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

    /// Targets evaluators can evaluate, excluding personal conversations.
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

    /// Readable only by the owner: excluded from evaluation, pending review, regression sets, and
    /// lists.
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

/// A dimension of rule-based scoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleDimension {
    /// The proportion of tool calls that complete without errors.
    ToolSuccess,
    /// The answer cites evidence backed by successful tool calls.
    Evidence,
    /// SQL and PromQL in tool calls can be parsed.
    QueryValidity,
    /// The answer has the required JSON shape.
    AnswerFormat,
    /// The turn completes.
    Completion,
    /// The asker's 👍 / 👎.
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

/// A rule-based scoring dimension: a score from 0 to 1 and the facts used to calculate it (see
/// [`rules`] for each dimension's keys).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DimensionScore {
    pub dimension: RuleDimension,
    pub score: f64,
    #[serde(default)]
    pub detail: Value,
}

/// The turn a target comes from and what it uses, for viewing scores by Prompt version, model, and
/// Skill.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attribution {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<Id>,
    /// The person asking the question; absent for incident RCA.
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
    /// The Skill invoked by a slash command in this turn: name and version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_version: Option<String>,
}

/// The target being scored: its source, what its turn uses, and the beginnings of the question and
/// answer shown in lists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreSubject {
    pub org_id: Id,
    pub target_kind: ScoreTarget,
    pub target_id: String,
    pub source: TargetSource,
    /// The beginnings of the question and answer; not recorded for personal conversations.
    #[serde(default)]
    pub question: String,
    #[serde(default)]
    pub answer: String,
    pub attribution: Attribution,
    /// The time the target is produced.
    pub target_at: TimestampMicros,
}

/// A target's score from one scorer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Score {
    pub id: Id,
    pub org_id: Id,
    pub target_kind: ScoreTarget,
    pub target_id: String,
    /// [`RULES_SCORER`] or an evaluator's ID.
    pub scorer: String,
    /// 0 to 1.
    pub score: f64,
    /// The dimensions of rule-based scoring; absent for evaluators.
    #[serde(default)]
    pub dimensions: Vec<DimensionScore>,
    /// The evaluator's reason; empty for rule-based scoring, where the page describes each
    /// dimension using its facts.
    #[serde(default)]
    pub reason: String,
    /// The number of evaluations, whose scores are averaged: low scores trigger another evaluation.
    pub samples: i32,
    /// The model used by the evaluator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluator_model: Option<String>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl Score {
    pub fn is_rules(&self) -> bool {
        self.scorer == RULES_SCORER
    }

    /// The score for a dimension.
    pub fn dimension(&self, dimension: RuleDimension) -> Option<f64> {
        self.dimensions
            .iter()
            .find(|candidate| candidate.dimension == dimension)
            .map(|candidate| candidate.score)
    }
}

/// A target's score: the average evaluator score if evaluated, otherwise the rule-based score.
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

/// A number from 0 to 99 determined by `seed`: the same target and scorer get the same draw every
/// time.
pub fn draw(seed: &str) -> u8 {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(seed.as_bytes());
    let mut first = [0u8; 8];
    first.copy_from_slice(&digest[..8]);

    (u64::from_be_bytes(first) % 100) as u8
}

/// Samples by percentage: selects none when `percent` is 0 and all when it is 100.
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
