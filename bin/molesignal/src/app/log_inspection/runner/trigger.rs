// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一次执行是怎么开始的：到了周期，或有人点了「立即执行」；以及卡住的执行和过期的报告怎么
//! 收拾。后台循环只负责按节拍调用这里，并保证同一条巡检不被两个节点同时开始。

use std::time::Duration;

use super::InspectionRunner;
use crate::{
    app::log_inspection::{
        schedule,
        service::{new_run, window_start},
    },
    domain::log_inspection::{LogInspection, LogInspectionRun, RunStatus, RunTrigger},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 超过总超时这么久还标着「执行中」的记录，执行它的节点已经不在了。
const INTERRUPTED_GRACE: Duration = Duration::from_secs(120);

/// 排队这么久还没有节点来执行的手动触发不再等：没有 alert 角色的节点在运行，或都忙不过来。
const QUEUED_PATIENCE: Duration = Duration::from_secs(15 * 60);

impl InspectionRunner {
    /// 全部组织里到期的巡检，最早到期的在前。
    pub async fn due(&self, now: TimestampMicros, limit: i64) -> Result<Vec<LogInspection>> {
        self.inspections.list_due(now, limit).await
    }

    /// 排队中的手动触发，先排的在前。
    pub async fn queued(&self, limit: i64) -> Result<Vec<LogInspectionRun>> {
        self.runs.list_queued(limit).await
    }

    /// 卡住的执行：见 [`Self::abandon`]。
    pub async fn stale(&self, now: TimestampMicros) -> Result<Vec<LogInspectionRun>> {
        let running_before = before(now, self.limits.run_timeout + INTERRUPTED_GRACE);
        let queued_before = before(now, QUEUED_PATIENCE);

        self.runs.list_stale(running_before, queued_before).await
    }

    /// 为一条到期的巡检开始一次定时执行：写下执行记录，同时把下次执行时间推到下一个周期。
    ///
    /// 重新读一遍巡检再决定：列表可能是旧的，别的节点也许刚执行完，它也可能刚被停用或删除。
    /// 返回 `None` 表示这次不执行：已经不到期，或它还有一次没结束的执行（下一拍再看）。
    pub async fn begin_scheduled(
        &self,
        id: &Id,
        now: TimestampMicros,
    ) -> Result<Option<(LogInspection, LogInspectionRun)>> {
        let inspection = match self.inspections.get_by_id(id).await {
            Ok(inspection) => inspection,
            Err(Error::NotFound(_)) => return Ok(None),
            Err(error) => return Err(error),
        };

        if !inspection.enabled || inspection.next_run_at.is_none_or(|at| at > now) {
            return Ok(None);
        }

        let timezone = schedule::parse_timezone(&inspection.timezone)?;
        let next_run_at = schedule::next_run_after(&inspection.schedule, timezone, now);
        let run = new_run(&inspection, RunTrigger::Schedule, RunStatus::Running, now);

        match self.runs.begin_scheduled(run, Some(next_run_at)).await {
            Ok(run) => Ok(Some((inspection, run))),
            Err(Error::Conflict(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// 领走一条排队中的手动触发。返回 `None` 表示它已经被别的节点领走，或巡检已被删除。
    pub async fn begin_queued(
        &self,
        queued: &LogInspectionRun,
        now: TimestampMicros,
    ) -> Result<Option<(LogInspection, LogInspectionRun)>> {
        let inspection = match self.inspections.get_by_id(&queued.inspection_id).await {
            Ok(inspection) => inspection,
            Err(Error::NotFound(_)) => return Ok(None),
            Err(error) => return Err(error),
        };

        // 覆盖的时间范围以开始执行的时刻为准，不是点下按钮的时刻。
        let start = window_start(&inspection, now);

        if !self.runs.start_queued(&queued.id, now, start, now).await? {
            return Ok(None);
        }

        let run = LogInspectionRun {
            status: RunStatus::Running,
            window_start: start,
            window_end: now,
            started_at: Some(now),
            ..queued.clone()
        };

        Ok(Some((inspection, run)))
    }

    /// 删除过了保留期的执行记录，返回删除的条数。
    pub async fn purge_expired(&self, now: TimestampMicros) -> Result<u64> {
        self.runs
            .delete_before(before(now, self.limits.report_retention))
            .await
    }
}

fn before(now: TimestampMicros, age: Duration) -> TimestampMicros {
    let micros = i64::try_from(age.as_micros()).unwrap_or(i64::MAX);

    TimestampMicros(now.0.saturating_sub(micros))
}
