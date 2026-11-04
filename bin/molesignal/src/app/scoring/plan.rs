// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一个对象要排哪些打分。
//!
//! 新的回答或 RCA：规则一次；评这个来源的评审按自己的比例抽，抽到的各一次。被评为没帮助：规则
//! 重新算一次（评价是其中一个维度），评这个来源的评审都评。个人对话只有规则。抽样由对象与评审
//! 决定，同一个对象排几次都抽到同样的结果。

use crate::{
    agent::scoring::{
        Evaluator, JobReason, NewJob, RULES_SCORER, ScoreTarget, ScoringRepository, TargetSource,
        sampled,
    },
    shared::{Result, ids::Id},
};

/// 新的回答或 RCA 要排的打分。
pub fn plan_answered(
    org_id: &Id,
    target: (ScoreTarget, &str),
    chat_id: Option<&Id>,
    source: TargetSource,
    evaluators: &[Evaluator],
) -> Vec<NewJob> {
    plan(
        org_id,
        target,
        chat_id,
        source,
        evaluators,
        JobReason::Answered,
        |evaluator| {
            sampled(
                &format!("{}:{}", target.1, evaluator.id.0),
                evaluator.sample_percent,
            )
        },
    )
}

/// 一条评价之后要排的打分。`down` 表示评为没帮助。
pub fn plan_rated(
    org_id: &Id,
    target: (ScoreTarget, &str),
    chat_id: Option<&Id>,
    source: TargetSource,
    down: bool,
    evaluators: &[Evaluator],
) -> Vec<NewJob> {
    plan(
        org_id,
        target,
        chat_id,
        source,
        evaluators,
        JobReason::Feedback,
        |_| down,
    )
}

fn plan(
    org_id: &Id,
    (kind, target_id): (ScoreTarget, &str),
    chat_id: Option<&Id>,
    source: TargetSource,
    evaluators: &[Evaluator],
    reason: JobReason,
    drawn: impl Fn(&Evaluator) -> bool,
) -> Vec<NewJob> {
    let job = |scorer: String| NewJob {
        org_id: org_id.clone(),
        target_kind: kind,
        target_id: target_id.to_string(),
        chat_id: chat_id.cloned(),
        scorer,
        reason,
    };

    let mut jobs = vec![job(RULES_SCORER.to_string())];

    if source.is_private() {
        return jobs;
    }

    jobs.extend(
        evaluators
            .iter()
            .filter(|evaluator| evaluator.scores(source) && drawn(evaluator))
            .map(|evaluator| job(evaluator.id.0.clone())),
    );

    jobs
}

/// 排上新的回答或 RCA 的打分。
pub async fn enqueue_answered(
    scoring: &dyn ScoringRepository,
    org_id: &Id,
    target: (ScoreTarget, &str),
    chat_id: Option<&Id>,
    source: TargetSource,
) -> Result<()> {
    let evaluators = scoring.list_evaluators(org_id).await?;

    scoring
        .enqueue(&plan_answered(org_id, target, chat_id, source, &evaluators))
        .await
        .map(|_| ())
}

/// 排上一条评价之后的打分。
pub async fn enqueue_rated(
    scoring: &dyn ScoringRepository,
    org_id: &Id,
    target: (ScoreTarget, &str),
    chat_id: Option<&Id>,
    source: TargetSource,
    down: bool,
) -> Result<()> {
    let evaluators = scoring.list_evaluators(org_id).await?;

    scoring
        .enqueue(&plan_rated(
            org_id,
            target,
            chat_id,
            source,
            down,
            &evaluators,
        ))
        .await
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::time::TimestampMicros;

    fn evaluator(
        id: &str,
        targets: Vec<TargetSource>,
        sample_percent: u8,
        enabled: bool,
    ) -> Evaluator {
        Evaluator {
            id: Id::from_string(id),
            org_id: Id::from_string("org"),
            name: id.into(),
            rubric: "Right?".into(),
            targets,
            sample_percent,
            provider_id: None,
            model: None,
            temperature: 0.0,
            enabled,
            language: "en-us".into(),
            created_by: Id::from_string("u1"),
            updated_by: Id::from_string("u1"),
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        }
    }

    fn scorers(jobs: &[NewJob]) -> Vec<&str> {
        jobs.iter().map(|job| job.scorer.as_str()).collect()
    }

    #[test]
    fn a_new_answer_gets_rules_and_the_evaluators_that_draw_it() {
        let evaluators = vec![
            evaluator("all", vec![TargetSource::TeamChat], 100, true),
            evaluator("none", vec![TargetSource::TeamChat], 0, true),
            evaluator("off", vec![TargetSource::TeamChat], 100, false),
            evaluator("rca", vec![TargetSource::IncidentRca], 100, true),
        ];
        let org = Id::from_string("org");
        let chat = Id::from_string("chat-1");

        let jobs = plan_answered(
            &org,
            (ScoreTarget::ChatAnswer, "m1"),
            Some(&chat),
            TargetSource::TeamChat,
            &evaluators,
        );

        assert_eq!(scorers(&jobs), vec![RULES_SCORER, "all"]);
        assert!(jobs.iter().all(|job| job.reason == JobReason::Answered));
        assert_eq!(jobs[0].chat_id, Some(chat));
    }

    #[test]
    fn a_thumbs_down_brings_every_evaluator_and_a_personal_chat_only_rules() {
        let evaluators = vec![
            evaluator("sampled_out", vec![TargetSource::TeamChat], 0, true),
            evaluator("rca", vec![TargetSource::IncidentRca], 0, true),
        ];
        let org = Id::from_string("org");
        let target = (ScoreTarget::ChatAnswer, "m1");

        let down = plan_rated(
            &org,
            target,
            None,
            TargetSource::TeamChat,
            true,
            &evaluators,
        );
        assert_eq!(scorers(&down), vec![RULES_SCORER, "sampled_out"]);

        let up = plan_rated(
            &org,
            target,
            None,
            TargetSource::TeamChat,
            false,
            &evaluators,
        );
        assert_eq!(scorers(&up), vec![RULES_SCORER]);

        let personal = plan_rated(
            &org,
            target,
            None,
            TargetSource::PersonalChat,
            true,
            &evaluators,
        );
        assert_eq!(scorers(&personal), vec![RULES_SCORER]);
    }
}
