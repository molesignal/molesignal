// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 执行排队的打分：规则打分与评审打分，之后按评价与分数决定进不进待审。
//!
//! - 规则打分：被评为没帮助的进待审；分数低于阈值、又没有评审要评它的进待审；新对象再按比例
//!   随机抽一些进待审。
//! - 评审：被评为没帮助时已经评过的不再评；低于阈值的再评一次取平均，平均仍低的进待审。
//!   今天的 token 预算用完时不评。
//!
//! 出错的过一会儿再试，最多 [`MAX_JOB_ATTEMPTS`] 次。

use super::{
    evaluating,
    facts::{Facts, gather},
};
use crate::{
    agent::{
        automations::reasons,
        scoring::{
            JobReason, JobStatus, MAX_JOB_ATTEMPTS, NewJob, RULES_SCORER, Review, ReviewCause,
            ReviewStatus, SCORE_STREAM, Score, ScoreJob, ScoreSubject, ScoringSettings, rules,
            sampled,
        },
    },
    api::AppState,
    app::scoring::score_event,
    domain::{intake::IntakeBatch, stream::StreamType},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 领走的打分多久没做完算节点没了：一批评审调用可能要好几分钟。
const LEASE_MICROS: i64 = 15 * 60 * 1_000_000;
/// 一次最多领这么多个。
const BATCH: i64 = 10;
/// 出错后第 n 次再试之前等 n 倍这么久。
const RETRY_MICROS: i64 = 2 * 60 * 1_000_000;

enum Outcome {
    Done,
    Skipped(&'static str),
}

/// 领走到点的打分并做完；返回领了几个。
pub(crate) async fn process_due(state: &AppState) -> Result<usize> {
    let now = TimestampMicros::now();
    let jobs = state
        .agent
        .scoring
        .claim_jobs(now, TimestampMicros(now.0 + LEASE_MICROS), BATCH)
        .await?;

    for job in &jobs {
        process(state, job).await;
    }

    Ok(jobs.len())
}

async fn process(state: &AppState, job: &ScoreJob) {
    let scoring = &state.agent.scoring;

    let written = if job.attempts > MAX_JOB_ATTEMPTS {
        scoring
            .finish_job(
                &job.id,
                JobStatus::Failed,
                Some("gave up after repeated attempts"),
            )
            .await
    } else {
        let outcome = if job.scorer == RULES_SCORER {
            by_rules(state, job).await
        } else {
            by_evaluator(state, job).await
        };

        match outcome {
            Ok(Outcome::Done) => scoring.finish_job(&job.id, JobStatus::Done, None).await,
            Ok(Outcome::Skipped(reason)) => {
                scoring
                    .finish_job(&job.id, JobStatus::Skipped, Some(reason))
                    .await
            }
            Err(error) if job.attempts < MAX_JOB_ATTEMPTS => {
                let wait = RETRY_MICROS * i64::from(job.attempts.max(1));
                let at = TimestampMicros(TimestampMicros::now().0 + wait);

                scoring.retry_job(&job.id, at, &error.to_string()).await
            }
            Err(error) => {
                scoring
                    .finish_job(&job.id, JobStatus::Failed, Some(&error.to_string()))
                    .await
            }
        }
    };

    if let Err(error) = written {
        tracing::warn!(job = %job.id.0, %error, "a score job could not be settled");
    }
}

async fn by_rules(state: &AppState, job: &ScoreJob) -> Result<Outcome> {
    let Some(facts) = gather(
        state,
        &job.org_id,
        job.target_kind,
        &job.target_id,
        job.chat_id.as_ref(),
    )
    .await?
    else {
        return Ok(Outcome::Skipped("the answer is gone"));
    };

    let dimensions = facts.rules();
    let now = TimestampMicros::now();

    let score = Score {
        id: Id::new(),
        org_id: job.org_id.clone(),
        target_kind: job.target_kind,
        target_id: job.target_id.clone(),
        scorer: RULES_SCORER.into(),
        score: rules::mean(&dimensions),
        dimensions,
        reason: String::new(),
        samples: 1,
        evaluator_model: None,
        created_at: now,
        updated_at: now,
    };

    save(state, &facts.subject, &score).await?;

    if facts.subject.source.is_private() {
        return Ok(Outcome::Done);
    }

    let settings = state.agent.scoring.settings(&job.org_id).await?;

    let cause = if facts.thumbs_down() {
        Some(ReviewCause::ThumbsDown)
    } else if settings.is_low(score.score)
        && !state
            .agent
            .scoring
            .evaluator_planned(&job.org_id, job.target_kind, &job.target_id)
            .await?
    {
        Some(ReviewCause::LowScore)
    } else if job.reason == JobReason::Answered
        && sampled(
            &format!("{}:review", job.target_id),
            settings.review_sample_percent,
        )
    {
        Some(ReviewCause::Sampled)
    } else {
        None
    };

    if let Some(cause) = cause {
        open_review(state, &facts, cause, score.score).await?;
    }

    Ok(Outcome::Done)
}

async fn by_evaluator(state: &AppState, job: &ScoreJob) -> Result<Outcome> {
    let scoring = &state.agent.scoring;

    let evaluator = match scoring
        .get_evaluator(&job.org_id, &Id(job.scorer.clone()))
        .await
    {
        Ok(evaluator) if evaluator.enabled => evaluator,
        Ok(_) => return Ok(Outcome::Skipped("the evaluator is switched off")),
        Err(Error::NotFound(_)) => return Ok(Outcome::Skipped("the evaluator was deleted")),
        Err(error) => return Err(error),
    };

    let Some(facts) = gather(
        state,
        &job.org_id,
        job.target_kind,
        &job.target_id,
        job.chat_id.as_ref(),
    )
    .await?
    else {
        return Ok(Outcome::Skipped("the answer is gone"));
    };

    if !evaluator.scores(facts.subject.source) {
        return Ok(Outcome::Skipped(
            "the evaluator does not score these answers",
        ));
    }

    let previous = scoring
        .scored(&job.org_id, job.target_kind, &job.target_id)
        .await?
        .and_then(|scored| {
            scored
                .scores
                .into_iter()
                .find(|score| score.scorer == job.scorer)
        });

    // 被评为没帮助时必评：已经评过的不用再评。
    if job.reason == JobReason::Feedback && previous.is_some() {
        return Ok(Outcome::Done);
    }

    if state.agent.usage.gate(&job.org_id, false).await.refused() {
        return Ok(Outcome::Skipped(reasons::BUDGET_EXHAUSTED));
    }

    let evaluated = evaluating::evaluate(state, &evaluator, &facts.material(None)).await?;

    let (value, samples) = match (&previous, job.reason) {
        (Some(previous), JobReason::Resample) => {
            let samples = previous.samples.max(1);
            let total = previous.score * f64::from(samples) + evaluated.verdict.score;

            (total / f64::from(samples + 1), samples + 1)
        }
        _ => (evaluated.verdict.score, 1),
    };

    let now = TimestampMicros::now();

    let score = Score {
        id: Id::new(),
        org_id: job.org_id.clone(),
        target_kind: job.target_kind,
        target_id: job.target_id.clone(),
        scorer: job.scorer.clone(),
        score: value,
        dimensions: Vec::new(),
        reason: evaluated.verdict.reason,
        samples,
        evaluator_model: Some(evaluated.model),
        created_at: previous
            .as_ref()
            .map_or(now, |previous| previous.created_at),
        updated_at: now,
    };

    save(state, &facts.subject, &score).await?;

    let settings: ScoringSettings = scoring.settings(&job.org_id).await?;

    if !settings.is_low(value) {
        return Ok(Outcome::Done);
    }

    // 低分先再评一次，平均仍低才进待审。
    if samples == 1 {
        scoring
            .enqueue(&[NewJob {
                org_id: job.org_id.clone(),
                target_kind: job.target_kind,
                target_id: job.target_id.clone(),
                chat_id: job.chat_id.clone(),
                scorer: job.scorer.clone(),
                reason: JobReason::Resample,
            }])
            .await?;
    } else {
        open_review(state, &facts, ReviewCause::LowScore, value).await?;
    }

    Ok(Outcome::Done)
}

/// 记下对象与分数，并写进 `_agent_score_traces`；流写不进去只记日志。
async fn save(state: &AppState, subject: &ScoreSubject, score: &Score) -> Result<()> {
    state.agent.scoring.put_subject(subject).await?;
    state.agent.scoring.put_score(score).await?;

    let written = state
        .intake
        .intake_internal_telemetry(IntakeBatch {
            batch_id: Id::new(),
            org_id: subject.org_id.clone(),
            stream: SCORE_STREAM.into(),
            stream_type: StreamType::LOGS,
            events: vec![score_event(subject, score)],
            received_at: TimestampMicros::now(),
        })
        .await;

    if let Err(error) = written {
        tracing::debug!(org = %subject.org_id.0, %error, "a score was not written to its stream");
    }

    Ok(())
}

async fn open_review(
    state: &AppState,
    facts: &Facts,
    cause: ReviewCause,
    score: f64,
) -> Result<()> {
    let now = TimestampMicros::now();
    let subject = &facts.subject;

    state
        .agent
        .scoring
        .open_review(&Review {
            id: Id::new(),
            org_id: subject.org_id.clone(),
            target_kind: subject.target_kind,
            target_id: subject.target_id.clone(),
            source: subject.source,
            cause,
            score: Some(score),
            status: ReviewStatus::Pending,
            note: String::new(),
            decided_by: None,
            decided_at: None,
            created_at: now,
            updated_at: now,
        })
        .await
        .map(|_| ())
}
