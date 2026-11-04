// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 打分数据存在哪：评审与设置、排队的打分、分数、待审与 Prompt 检查。

use async_trait::async_trait;
use serde::Serialize;

use super::{
    Evaluator, JobStatus, NewJob, Review, ReviewDecision, ReviewStatus, RuleDimension, Score,
    ScoreJob, ScoreSubject, ScoreTarget, ScoringSettings, TargetSource, check::PromptCheck,
};
use crate::shared::{Result, ids::Id, time::TimestampMicros};

/// 一个对象和它的全部分数：规则打分在前，评审按评分的先后。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScoredTarget {
    #[serde(flatten)]
    pub subject: ScoreSubject,
    pub scores: Vec<Score>,
}

/// 待审的一条，和它的对象（对象的分数被删掉时没有）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReviewItem {
    #[serde(flatten)]
    pub review: Review,
    pub subject: Option<ScoreSubject>,
}

/// 列出打过分的对象：最新的在前，个人对话不在其中。
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredFilter {
    pub since: TimestampMicros,
    pub source: Option<TargetSource>,
    /// 只要有一个分数低于它的。
    pub below: Option<f64>,
    /// 从这个对象之后（更早）接着列：产生时间与对象 ID。
    pub before: Option<(TimestampMicros, String)>,
    pub limit: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewFilter {
    pub status: Option<ReviewStatus>,
    /// 从这一条之后（更早）接着列：进入待审的时间与 ID。
    pub before: Option<(TimestampMicros, Id)>,
    pub limit: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OverviewFilter {
    pub since: TimestampMicros,
    pub source: Option<TargetSource>,
}

/// 一段时间里的质量。规则打分覆盖每个对象；评审只评抽到的。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityOverview {
    /// 打过分的对象。
    pub answers: i64,
    pub rules_mean: Option<f64>,
    /// 评审评过的对象。
    pub evaluated: i64,
    pub evaluator_mean: Option<f64>,
    pub thumbs_up: i64,
    pub thumbs_down: i64,
    pub pending_reviews: i64,
    pub daily: Vec<DailyQuality>,
    pub dimensions: Vec<DimensionMean>,
    pub sources: Vec<GroupQuality>,
    pub prompts: Vec<GroupQuality>,
    pub skills: Vec<GroupQuality>,
    pub models: Vec<GroupQuality>,
}

/// 一天（UTC）的质量。`day` 是从 1970-01-01 起的天数。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyQuality {
    pub day: i64,
    pub answers: i64,
    pub rules_mean: Option<f64>,
    pub evaluated: i64,
    pub evaluator_mean: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DimensionMean {
    pub dimension: RuleDimension,
    pub mean: f64,
    /// 有这个维度的对象：没调工具的回答没有工具成功率。
    pub answers: i64,
}

/// 按来源、Prompt 版本、Skill 或模型分组的质量。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupQuality {
    pub key: String,
    /// Prompt 的版本、Skill 的版本。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub answers: i64,
    pub rules_mean: Option<f64>,
    pub evaluated: i64,
    pub evaluator_mean: Option<f64>,
}

#[async_trait]
pub trait ScoringRepository: Send + Sync {
    async fn list_evaluators(&self, org_id: &Id) -> Result<Vec<Evaluator>>;
    async fn get_evaluator(&self, org_id: &Id, id: &Id) -> Result<Evaluator>;
    /// 新建评审；名称在组织里唯一，超过 [`super::MAX_EVALUATORS_PER_ORG`] 个时拒绝。
    async fn create_evaluator(&self, evaluator: Evaluator) -> Result<Evaluator>;
    async fn update_evaluator(&self, evaluator: Evaluator) -> Result<Evaluator>;
    /// 删除评审；它给过的分数留着，排着的打分作废。
    async fn delete_evaluator(&self, org_id: &Id, id: &Id) -> Result<()>;

    /// 组织的设置；没设过时是默认值。
    async fn settings(&self, org_id: &Id) -> Result<ScoringSettings>;
    async fn put_settings(
        &self,
        org_id: &Id,
        settings: &ScoringSettings,
    ) -> Result<ScoringSettings>;

    /// 排进队列；同一个对象、同一个打分器已经排着的跳过。返回新排的个数。
    async fn enqueue(&self, jobs: &[NewJob]) -> Result<u64>;
    /// 领走到点的打分（含领走后节点没了、租约过期的）。
    async fn claim_jobs(
        &self,
        now: TimestampMicros,
        lease_until: TimestampMicros,
        limit: i64,
    ) -> Result<Vec<ScoreJob>>;
    async fn finish_job(&self, id: &Id, status: JobStatus, error: Option<&str>) -> Result<()>;
    /// 出错了，过一会儿再试。
    async fn retry_job(&self, id: &Id, not_before: TimestampMicros, error: &str) -> Result<()>;
    /// 有评审要评或已经评过这个对象。
    async fn evaluator_planned(
        &self,
        org_id: &Id,
        kind: ScoreTarget,
        target_id: &str,
    ) -> Result<bool>;
    /// 删掉结束早于 `before` 的打分记录。
    async fn purge_jobs(&self, before: TimestampMicros) -> Result<u64>;

    /// 记下被打分的对象，已有时替换：分数之前先记它。
    async fn put_subject(&self, subject: &ScoreSubject) -> Result<()>;
    /// 保存一个对象在一个打分器下的分数，已有时替换。
    async fn put_score(&self, score: &Score) -> Result<()>;
    /// 一个对象和它的全部分数；没打过分时为 `None`。
    async fn scored(
        &self,
        org_id: &Id,
        kind: ScoreTarget,
        target_id: &str,
    ) -> Result<Option<ScoredTarget>>;
    async fn list_scored(&self, org_id: &Id, filter: &ScoredFilter) -> Result<Vec<ScoredTarget>>;
    async fn overview(&self, org_id: &Id, filter: &OverviewFilter) -> Result<QualityOverview>;
    /// 这个评审最近评过的对象，最新的在前。
    async fn recent_evaluated(
        &self,
        org_id: &Id,
        evaluator: &Id,
        sources: &[TargetSource],
        limit: i64,
    ) -> Result<Vec<ScoredTarget>>;

    /// 进入待审；对象已在待审里（不论审没审过）时什么都不做，返回 `false`。
    async fn open_review(&self, review: &Review) -> Result<bool>;
    async fn get_review(&self, org_id: &Id, id: &Id) -> Result<Review>;
    async fn review_of(
        &self,
        org_id: &Id,
        kind: ScoreTarget,
        target_id: &str,
    ) -> Result<Option<Review>>;
    async fn list_reviews(&self, org_id: &Id, filter: &ReviewFilter) -> Result<Vec<ReviewItem>>;
    async fn decide_review(
        &self,
        org_id: &Id,
        id: &Id,
        decision: &ReviewDecision,
    ) -> Result<Review>;
    /// 采纳过的待审，最新的在前：回归集。
    async fn regression_set(
        &self,
        org_id: &Id,
        sources: &[TargetSource],
        limit: i64,
    ) -> Result<Vec<Review>>;

    async fn create_check(&self, check: &PromptCheck) -> Result<()>;
    async fn get_check(&self, org_id: &Id, id: &Id) -> Result<PromptCheck>;
    /// 一个 Prompt 最近的一次检查。
    async fn latest_check(&self, org_id: &Id, prompt_id: &Id) -> Result<Option<PromptCheck>>;
    /// 领走一次排着的检查（含领走后节点没了、租约过期的）。
    async fn claim_check(
        &self,
        now: TimestampMicros,
        lease_until: TimestampMicros,
    ) -> Result<Option<PromptCheck>>;
    /// 记下进度或结果，并续上租约。
    async fn save_check(&self, check: &PromptCheck, lease_until: TimestampMicros) -> Result<()>;
}
