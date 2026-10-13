// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;

use super::{InspectionNotification, LogInspection, LogInspectionRun, RunCompletion};
use crate::shared::{Result, ids::Id, time::TimestampMicros};

/// 定时巡检的定义存在哪里。
#[async_trait]
pub trait LogInspectionRepository: Send + Sync {
    /// 组织内重名时返回 `Conflict`。
    async fn create(&self, inspection: LogInspection) -> Result<LogInspection>;

    /// 保存定义（查什么、周期、模型、通知、启停、执行身份、下次执行时间）；最近一次执行的
    /// 状况不受影响。
    async fn update(&self, inspection: LogInspection) -> Result<LogInspection>;

    async fn get(&self, org_id: &Id, id: &Id) -> Result<LogInspection>;

    /// 不限定组织，后台执行用。
    async fn get_by_id(&self, id: &Id) -> Result<LogInspection>;

    async fn list(&self, org_id: &Id) -> Result<Vec<LogInspection>>;

    async fn count(&self, org_id: &Id) -> Result<i64>;

    /// 连同它的全部执行记录一起删除。
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;

    /// 启停。停用时 `next_run_at` 为 `None`；执行身份不变。
    async fn set_enabled(
        &self,
        org_id: &Id,
        id: &Id,
        enabled: bool,
        next_run_at: Option<TimestampMicros>,
        now: TimestampMicros,
    ) -> Result<LogInspection>;

    /// 全部组织里启用中、下次执行时间不晚于 `now` 的巡检，最早到期的在前。
    async fn list_due(&self, now: TimestampMicros, limit: i64) -> Result<Vec<LogInspection>>;
}

/// 巡检的执行记录存在哪里。
///
/// 同一条巡检同一时刻至多有一条没结束的执行（排队中或执行中）：写入第二条时返回
/// [`crate::shared::Error::Conflict`]，定时触发与手动触发因此不会叠在一起。
#[async_trait]
pub trait LogInspectionRunRepository: Send + Sync {
    /// 定时触发：写入一条执行中的记录，并把这条巡检的下次执行时间推到 `next_run_at`。
    /// 两件事在一个事务里：记录写不进去（已有没结束的执行）时，下次执行时间也不动。
    async fn begin_scheduled(
        &self,
        run: LogInspectionRun,
        next_run_at: Option<TimestampMicros>,
    ) -> Result<LogInspectionRun>;

    /// 手动触发：写入一条排队中的记录。
    async fn enqueue(&self, run: LogInspectionRun) -> Result<LogInspectionRun>;

    /// 排队中的执行，先排的在前。
    async fn list_queued(&self, limit: i64) -> Result<Vec<LogInspectionRun>>;

    /// 把一条排队中的执行改为执行中，并写上它覆盖的时间范围。已经不在排队（被别的节点
    /// 领走、被判为过期）时返回 `false`。
    async fn start_queued(
        &self,
        run_id: &Id,
        started_at: TimestampMicros,
        window_start: TimestampMicros,
        window_end: TimestampMicros,
    ) -> Result<bool>;

    /// 执行结束：落报告或失败原因，并把结果记到巡检的「最近一次执行」上。只对还没结束的
    /// 记录生效；记录已经被别处结束（例如被判为中断）时返回 `false`，什么都不改，调用方
    /// 据此不再发通知。
    async fn finish(&self, run_id: &Id, completion: RunCompletion) -> Result<bool>;

    async fn record_notifications(
        &self,
        run_id: &Id,
        notifications: &[InspectionNotification],
    ) -> Result<()>;

    /// 卡住的执行：`running_before` 之前开始、至今仍在执行中的（节点中途退出），以及
    /// `queued_before` 之前排队、至今没人执行的。只读，由调用方逐条 [`Self::finish`]。
    async fn list_stale(
        &self,
        running_before: TimestampMicros,
        queued_before: TimestampMicros,
    ) -> Result<Vec<LogInspectionRun>>;

    /// 组织里还没结束的执行（排队中或执行中），不带报告正文。每条巡检至多一条，列表用它
    /// 标出哪些巡检此刻正在执行。
    async fn list_active(&self, org_id: &Id) -> Result<Vec<LogInspectionRun>>;

    /// 一条巡检的执行记录，最近的在前，不带报告正文。`before` 用于翻页：只要创建时间
    /// 早于它的。
    async fn list(
        &self,
        org_id: &Id,
        inspection_id: &Id,
        before: Option<TimestampMicros>,
        limit: i64,
    ) -> Result<Vec<LogInspectionRun>>;

    /// 带报告正文的单条执行记录。
    async fn get(&self, org_id: &Id, inspection_id: &Id, run_id: &Id) -> Result<LogInspectionRun>;

    /// 删除 `cutoff` 之前创建且已结束的执行记录，返回删除的条数。
    async fn delete_before(&self, cutoff: TimestampMicros) -> Result<u64>;
}
