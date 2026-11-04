// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一个分数在 `_agent_score_traces` 里的样子：每次打分一条，字段名不带点（SQL 里点是限定符）。
//! 规则打分的各个维度各占一个字段（`tool_success`、`evidence`……），没有的维度不写。

use serde_json::{Map, Value, json};

use crate::{
    agent::scoring::{RuleDimension, Score, ScoreSubject},
    domain::intake::RawEvent,
};

/// 评审的理由最多写这么多字符。
const REASON_CHARS: usize = 500;

pub fn score_event(subject: &ScoreSubject, score: &Score) -> RawEvent {
    let mut fields = Map::new();
    let attribution = &subject.attribution;

    fields.insert("target_kind".into(), json!(score.target_kind.as_str()));
    fields.insert("target_id".into(), json!(score.target_id));
    fields.insert("source".into(), json!(subject.source.as_str()));
    fields.insert("scorer".into(), json!(score.scorer));
    fields.insert(
        "scorer_kind".into(),
        json!(if score.is_rules() {
            "rules"
        } else {
            "evaluator"
        }),
    );
    fields.insert("score".into(), json!(score.score));
    fields.insert("samples".into(), json!(score.samples));
    fields.insert("target_at".into(), json!(subject.target_at.0));

    let id = |value: &Option<crate::shared::ids::Id>| value.as_ref().map(|id| json!(id.0));

    let optional = [
        ("chat_id", id(&attribution.chat_id)),
        ("team_id", id(&attribution.team_id)),
        ("automation_id", id(&attribution.automation_id)),
        ("user_id", id(&attribution.user_id)),
        (
            "prompt_template_id",
            attribution
                .prompt_template_id
                .as_ref()
                .map(|value| json!(value)),
        ),
        (
            "prompt_version",
            attribution.prompt_version.map(|value| json!(value)),
        ),
        (
            "prompt_key",
            attribution.prompt_key.as_ref().map(|value| json!(value)),
        ),
        ("provider_id", id(&attribution.provider_id)),
        (
            "model",
            attribution.model.as_ref().map(|value| json!(value)),
        ),
        (
            "skill",
            attribution.skill.as_ref().map(|value| json!(value)),
        ),
        (
            "skill_version",
            attribution.skill_version.as_ref().map(|value| json!(value)),
        ),
        (
            "evaluator_model",
            score.evaluator_model.as_ref().map(|value| json!(value)),
        ),
        (
            "reason",
            (!score.reason.is_empty())
                .then(|| json!(score.reason.chars().take(REASON_CHARS).collect::<String>())),
        ),
    ];

    for (name, value) in optional {
        if let Some(value) = value {
            fields.insert(name.into(), value);
        }
    }

    for dimension in RuleDimension::ALL {
        if let Some(value) = score.dimension(dimension) {
            fields.insert(dimension.as_str().into(), Value::from(value));
        }
    }

    RawEvent {
        timestamp: score.updated_at,
        fields,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::scoring::{Attribution, DimensionScore, ScoreTarget, TargetSource},
        shared::{ids::Id, time::TimestampMicros},
    };

    #[test]
    fn a_rules_score_carries_its_dimensions_and_attribution() {
        let subject = ScoreSubject {
            org_id: Id::from_string("org"),
            target_kind: ScoreTarget::ChatAnswer,
            target_id: "m1".into(),
            source: TargetSource::Automation,
            question: "Why?".into(),
            answer: "Because.".into(),
            attribution: Attribution {
                automation_id: Some(Id::from_string("a1")),
                prompt_version: Some(3),
                ..Attribution::default()
            },
            target_at: TimestampMicros(5),
        };

        let score = Score {
            id: Id::from_string("s1"),
            org_id: Id::from_string("org"),
            target_kind: ScoreTarget::ChatAnswer,
            target_id: "m1".into(),
            scorer: "rules".into(),
            score: 0.75,
            dimensions: vec![DimensionScore {
                dimension: RuleDimension::ToolSuccess,
                score: 0.5,
                detail: json!({ "failed": 1, "total": 2 }),
            }],
            reason: String::new(),
            samples: 1,
            evaluator_model: None,
            created_at: TimestampMicros(6),
            updated_at: TimestampMicros(7),
        };

        let event = score_event(&subject, &score);

        assert_eq!(event.timestamp, TimestampMicros(7));
        assert_eq!(event.fields["scorer_kind"], "rules");
        assert_eq!(event.fields["source"], "automation");
        assert_eq!(event.fields["tool_success"], 0.5);
        assert_eq!(event.fields["automation_id"], "a1");
        assert_eq!(event.fields["prompt_version"], 3);
        assert_eq!(event.fields["target_at"], 5);
        assert!(!event.fields.contains_key("reason"));
        assert!(!event.fields.contains_key("evidence"));
        assert!(!event.fields.contains_key("question"));
    }
}
