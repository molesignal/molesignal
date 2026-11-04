// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 的质量数据：`agent_evaluators`、`agent_score_jobs`、`agent_scores`、`agent_score_reviews`、
//! `agent_prompt_checks`，以及 `org_settings` 里的 `agent.scoring` 一节。
//!
//! 评审与设置的 SQL 在 [`config`]，排队的打分在 [`jobs`]，分数与列表在 [`scores`]，汇总在
//! [`overview`]，待审在 [`reviews`]，Prompt 检查在 [`checks`]。

mod checks;
mod config;
mod jobs;
mod overview;
mod reviews;
mod scores;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::{PgPool, Row, postgres::PgRow, types::Json};

use super::super::sqlx_err;
use crate::{
    agent::scoring::{
        Evaluator, JobStatus, NewJob, OverviewFilter, QualityOverview, Review, ReviewDecision,
        ReviewFilter, ReviewItem, Score, ScoreJob, ScoreSubject, ScoreTarget, ScoredFilter,
        ScoredTarget, ScoringRepository, ScoringSettings, TargetSource, check::PromptCheck,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgScoringRepository {
    pool: PgPool,
}

impl PgScoringRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn json<T: DeserializeOwned>(row: &PgRow, column: &str) -> Result<T> {
    let value: Json<Value> = row.try_get(column).map_err(sqlx_err)?;

    serde_json::from_value(value.0)
        .map_err(|error| Error::internal(format!("stored score `{column}`: {error}")))
}

fn text(row: &PgRow, column: &str) -> Result<String> {
    row.try_get(column).map_err(sqlx_err)
}

fn optional_text(row: &PgRow, column: &str) -> Result<Option<String>> {
    row.try_get(column).map_err(sqlx_err)
}

fn micros(row: &PgRow, column: &str) -> Result<TimestampMicros> {
    row.try_get(column).map_err(sqlx_err)
}

fn optional_micros(row: &PgRow, column: &str) -> Result<Option<TimestampMicros>> {
    row.try_get(column).map_err(sqlx_err)
}

fn sources(sources: &[TargetSource]) -> Vec<&'static str> {
    sources.iter().map(|source| source.as_str()).collect()
}

/// 唯一约束冲突。
fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database) if database.code().as_deref() == Some("23505"))
}

#[async_trait]
impl ScoringRepository for PgScoringRepository {
    async fn list_evaluators(&self, org_id: &Id) -> Result<Vec<Evaluator>> {
        config::list_evaluators(&self.pool, org_id).await
    }

    async fn get_evaluator(&self, org_id: &Id, id: &Id) -> Result<Evaluator> {
        config::get_evaluator(&self.pool, org_id, id).await
    }

    async fn create_evaluator(&self, evaluator: Evaluator) -> Result<Evaluator> {
        config::create_evaluator(&self.pool, evaluator).await
    }

    async fn update_evaluator(&self, evaluator: Evaluator) -> Result<Evaluator> {
        config::update_evaluator(&self.pool, evaluator).await
    }

    async fn delete_evaluator(&self, org_id: &Id, id: &Id) -> Result<()> {
        config::delete_evaluator(&self.pool, org_id, id).await
    }

    async fn settings(&self, org_id: &Id) -> Result<ScoringSettings> {
        config::settings(&self.pool, org_id).await
    }

    async fn put_settings(
        &self,
        org_id: &Id,
        settings: &ScoringSettings,
    ) -> Result<ScoringSettings> {
        config::put_settings(&self.pool, org_id, settings).await
    }

    async fn enqueue(&self, jobs: &[NewJob]) -> Result<u64> {
        jobs::enqueue(&self.pool, jobs).await
    }

    async fn claim_jobs(
        &self,
        now: TimestampMicros,
        lease_until: TimestampMicros,
        limit: i64,
    ) -> Result<Vec<ScoreJob>> {
        jobs::claim(&self.pool, now, lease_until, limit).await
    }

    async fn finish_job(&self, id: &Id, status: JobStatus, error: Option<&str>) -> Result<()> {
        jobs::finish(&self.pool, id, status, error).await
    }

    async fn retry_job(&self, id: &Id, not_before: TimestampMicros, error: &str) -> Result<()> {
        jobs::retry(&self.pool, id, not_before, error).await
    }

    async fn evaluator_planned(
        &self,
        org_id: &Id,
        kind: ScoreTarget,
        target_id: &str,
    ) -> Result<bool> {
        jobs::evaluator_planned(&self.pool, org_id, kind, target_id).await
    }

    async fn purge_jobs(&self, before: TimestampMicros) -> Result<u64> {
        jobs::purge(&self.pool, before).await
    }

    async fn put_subject(&self, subject: &ScoreSubject) -> Result<()> {
        scores::put_subject(&self.pool, subject).await
    }

    async fn put_score(&self, score: &Score) -> Result<()> {
        scores::put(&self.pool, score).await
    }

    async fn scored(
        &self,
        org_id: &Id,
        kind: ScoreTarget,
        target_id: &str,
    ) -> Result<Option<ScoredTarget>> {
        scores::scored(&self.pool, org_id, kind, target_id).await
    }

    async fn list_scored(&self, org_id: &Id, filter: &ScoredFilter) -> Result<Vec<ScoredTarget>> {
        scores::list(&self.pool, org_id, filter).await
    }

    async fn overview(&self, org_id: &Id, filter: &OverviewFilter) -> Result<QualityOverview> {
        overview::overview(&self.pool, org_id, filter).await
    }

    async fn recent_evaluated(
        &self,
        org_id: &Id,
        evaluator: &Id,
        sources: &[TargetSource],
        limit: i64,
    ) -> Result<Vec<ScoredTarget>> {
        scores::recent_evaluated(&self.pool, org_id, evaluator, sources, limit).await
    }

    async fn open_review(&self, review: &Review) -> Result<bool> {
        reviews::open(&self.pool, review).await
    }

    async fn get_review(&self, org_id: &Id, id: &Id) -> Result<Review> {
        reviews::get(&self.pool, org_id, id).await
    }

    async fn review_of(
        &self,
        org_id: &Id,
        kind: ScoreTarget,
        target_id: &str,
    ) -> Result<Option<Review>> {
        reviews::of_target(&self.pool, org_id, kind, target_id).await
    }

    async fn list_reviews(&self, org_id: &Id, filter: &ReviewFilter) -> Result<Vec<ReviewItem>> {
        reviews::list(&self.pool, org_id, filter).await
    }

    async fn decide_review(
        &self,
        org_id: &Id,
        id: &Id,
        decision: &ReviewDecision,
    ) -> Result<Review> {
        reviews::decide(&self.pool, org_id, id, decision).await
    }

    async fn regression_set(
        &self,
        org_id: &Id,
        sources: &[TargetSource],
        limit: i64,
    ) -> Result<Vec<Review>> {
        reviews::regression_set(&self.pool, org_id, sources, limit).await
    }

    async fn create_check(&self, check: &PromptCheck) -> Result<()> {
        checks::create(&self.pool, check).await
    }

    async fn get_check(&self, org_id: &Id, id: &Id) -> Result<PromptCheck> {
        checks::get(&self.pool, org_id, id).await
    }

    async fn latest_check(&self, org_id: &Id, prompt_id: &Id) -> Result<Option<PromptCheck>> {
        checks::latest(&self.pool, org_id, prompt_id).await
    }

    async fn claim_check(
        &self,
        now: TimestampMicros,
        lease_until: TimestampMicros,
    ) -> Result<Option<PromptCheck>> {
        checks::claim(&self.pool, now, lease_until).await
    }

    async fn save_check(&self, check: &PromptCheck, lease_until: TimestampMicros) -> Result<()> {
        checks::save(&self.pool, check, lease_until).await
    }
}
