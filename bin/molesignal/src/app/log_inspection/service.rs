// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 定时巡检的增删改查、启停与手动触发。执行本身在 [`super::runner`]。
//!
//! 保存就是决定「查什么、通知谁」：每次保存都把执行身份改成保存者，并核对被引用的数据流、
//! 模型配置和通知渠道此刻都存在。

use std::sync::Arc;

use super::{
    InspectionLimits,
    agent::{AgentFailure, InspectionAgent},
    identity::{missing_run_permission, run_as_user},
    input::{self, InspectionInput, ValidInput},
    notification::NotificationSender,
    schedule,
};
use crate::{
    domain::{
        iam::IamContext,
        log_inspection::{
            InspectionNotifyRoute, LogInspection, LogInspectionRun, RunStatus, RunTrigger,
            repositories::{LogInspectionRepository, LogInspectionRunRepository},
        },
        stream::{StreamRepository, StreamType},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct LogInspectionService {
    inspections: Arc<dyn LogInspectionRepository>,
    runs: Arc<dyn LogInspectionRunRepository>,
    streams: Arc<dyn StreamRepository>,
    sender: Arc<dyn NotificationSender>,
    agent: Arc<dyn InspectionAgent>,
    limits: InspectionLimits,
}

impl LogInspectionService {
    pub fn new(
        inspections: Arc<dyn LogInspectionRepository>,
        runs: Arc<dyn LogInspectionRunRepository>,
        streams: Arc<dyn StreamRepository>,
        sender: Arc<dyn NotificationSender>,
        agent: Arc<dyn InspectionAgent>,
        limits: InspectionLimits,
    ) -> Self {
        Self {
            inspections,
            runs,
            streams,
            sender,
            agent,
            limits,
        }
    }

    pub fn limits(&self) -> InspectionLimits {
        self.limits
    }

    pub async fn list(&self, org_id: &Id) -> Result<Vec<LogInspection>> {
        self.inspections.list(org_id).await
    }

    pub async fn get(&self, org_id: &Id, id: &Id) -> Result<LogInspection> {
        self.inspections.get(org_id, id).await
    }

    /// 组织里此刻排队中或执行中的执行，每条巡检至多一条。
    pub async fn active_runs(&self, org_id: &Id) -> Result<Vec<LogInspectionRun>> {
        self.runs.list_active(org_id).await
    }

    pub async fn create(
        &self,
        saver: &IamContext,
        input: InspectionInput,
    ) -> Result<LogInspection> {
        let run_as_user_id = self.saver_identity(saver)?;
        let input = input::validate(input, &self.limits)?;
        self.check_references(&saver.org_id, &input).await?;

        let saved = self.inspections.count(&saver.org_id).await?;

        if saved >= i64::from(self.limits.max_per_org) {
            return Err(Error::conflict(format!(
                "this organization already has {saved} scheduled log inspections; the limit is {}",
                self.limits.max_per_org
            )));
        }

        let now = TimestampMicros::now();

        let next_run_at = input
            .enabled
            .then(|| schedule::next_run_after(&input.schedule, input.timezone, now));

        let inspection = LogInspection {
            id: Id::new(),
            org_id: saver.org_id.clone(),
            name: input.name,
            stream: input.stream,
            statement: input.statement,
            window_secs: input.window_secs,
            requirements: input.requirements,
            schedule: input.schedule,
            timezone: input.timezone.name().to_string(),
            provider_id: input.provider_id,
            model: input.model,
            notify_when: input.notify_when,
            notify_routes: input.notify_routes,
            language: input.language,
            enabled: input.enabled,
            run_as_user_id: run_as_user_id.clone(),
            next_run_at,
            last_run: None,
            created_by: run_as_user_id,
            created_at: now,
            updated_at: now,
        };

        self.inspections
            .create(inspection)
            .await
            .map_err(name_taken)
    }

    pub async fn update(
        &self,
        saver: &IamContext,
        id: &Id,
        input: InspectionInput,
    ) -> Result<LogInspection> {
        let run_as_user_id = self.saver_identity(saver)?;
        let input = input::validate(input, &self.limits)?;
        let existing = self.inspections.get(&saver.org_id, id).await?;
        self.check_references(&saver.org_id, &input).await?;

        let now = TimestampMicros::now();
        let timezone = input.timezone.name().to_string();

        // 周期没变的巡检不因为改了名字或通知渠道而重新起算。
        let next_run_at = if !input.enabled {
            None
        } else if existing.enabled
            && existing.next_run_at.is_some()
            && existing.schedule == input.schedule
            && existing.timezone == timezone
        {
            existing.next_run_at
        } else {
            Some(schedule::next_run_after(
                &input.schedule,
                input.timezone,
                now,
            ))
        };

        let inspection = LogInspection {
            name: input.name,
            stream: input.stream,
            statement: input.statement,
            window_secs: input.window_secs,
            requirements: input.requirements,
            schedule: input.schedule,
            timezone,
            provider_id: input.provider_id,
            model: input.model,
            notify_when: input.notify_when,
            notify_routes: input.notify_routes,
            language: input.language,
            enabled: input.enabled,
            // 每次保存都改成保存者：改了查询和通知渠道的人，用的是自己的权限。
            run_as_user_id,
            next_run_at,
            updated_at: now,
            ..existing
        };

        self.inspections
            .update(inspection)
            .await
            .map_err(name_taken)
    }

    /// 启停。只是暂停或恢复按周期执行，查什么、通知谁都没变，执行身份也不变。
    pub async fn set_enabled(&self, org_id: &Id, id: &Id, enabled: bool) -> Result<LogInspection> {
        let existing = self.inspections.get(org_id, id).await?;

        if existing.enabled == enabled {
            return Ok(existing);
        }

        let now = TimestampMicros::now();

        let next_run_at = if enabled {
            let timezone = schedule::parse_timezone(&existing.timezone)?;

            Some(schedule::next_run_after(&existing.schedule, timezone, now))
        } else {
            None
        };

        self.inspections
            .set_enabled(org_id, id, enabled, next_run_at, now)
            .await
    }

    pub async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        self.inspections.delete(org_id, id).await
    }

    /// 立即执行一次：排队，由后台执行。停用的巡检也可以手动执行，用来在启用前试一次。
    pub async fn run_now(&self, requester: &IamContext, id: &Id) -> Result<LogInspectionRun> {
        let inspection = self.inspections.get(&requester.org_id, id).await?;

        let run = LogInspectionRun {
            requested_by: Some(requester.user_id.clone()),
            ..new_run(
                &inspection,
                RunTrigger::Manual,
                RunStatus::Queued,
                TimestampMicros::now(),
            )
        };

        self.runs.enqueue(run).await.map_err(|error| match error {
            Error::Conflict(_) => {
                Error::conflict("this log inspection already has a run in progress")
            }
            other => other,
        })
    }

    pub async fn list_runs(
        &self,
        org_id: &Id,
        id: &Id,
        before: Option<TimestampMicros>,
        limit: i64,
    ) -> Result<Vec<LogInspectionRun>> {
        // 先确认巡检在这个组织里：别的组织的 id 得到的是「不存在」，不是一个空列表。
        self.inspections.get(org_id, id).await?;
        self.runs.list(org_id, id, before, limit).await
    }

    pub async fn get_run(&self, org_id: &Id, id: &Id, run_id: &Id) -> Result<LogInspectionRun> {
        self.runs.get(org_id, id, run_id).await
    }

    /// 保存者要能成为执行身份，并且自己就有执行需要的权限：存进去的巡检不该一执行就失败。
    fn saver_identity(&self, saver: &IamContext) -> Result<Id> {
        let user_id = run_as_user(saver)?;

        if let Some(permission) = missing_run_permission(saver) {
            return Err(Error::forbidden(format!(
                "a scheduled log inspection runs with the access of the user who saves it, and \
                 that takes {permission}"
            )));
        }

        Ok(user_id)
    }

    /// 被引用的数据流、模型配置和通知渠道此刻都存在、都能用。
    async fn check_references(&self, org_id: &Id, input: &ValidInput) -> Result<()> {
        match self
            .streams
            .get(org_id, &input.stream, StreamType::LOGS)
            .await
        {
            Ok(_) => {}
            Err(Error::NotFound(_)) => {
                return Err(Error::invalid(format!(
                    "`{}` is not a logs stream of this organization",
                    input.stream
                )));
            }
            Err(error) => return Err(error),
        }

        match self.agent.check_model(org_id, &input.provider_id).await {
            Ok(()) => {}
            Err(AgentFailure::Model(reason)) => {
                return Err(Error::invalid(format!(
                    "the AI model cannot be used: {reason}"
                )));
            }
            Err(AgentFailure::Call(reason)) => return Err(Error::internal(reason)),
            // 保存时只核对模型配置，不调用模型，预算与它无关。
            Err(AgentFailure::Budget(_)) => {}
        }

        for route in &input.notify_routes {
            self.check_route(org_id, route).await?;
        }

        Ok(())
    }

    async fn check_route(&self, org_id: &Id, route: &InspectionNotifyRoute) -> Result<()> {
        match self.sender.check(org_id, route).await {
            Err(Error::NotFound(_)) => Err(Error::invalid(format!(
                "notify connector `{}` does not exist in this organization",
                route.connector_id.0
            ))),
            checked => checked,
        }
    }
}

/// 巡检覆盖执行时刻之前的这段时间。
pub(super) fn window_start(inspection: &LogInspection, end: TimestampMicros) -> TimestampMicros {
    TimestampMicros(
        end.0
            .saturating_sub(inspection.window_secs.saturating_mul(1_000_000)),
    )
}

/// 一条新的执行记录：覆盖 `now` 之前的那段时间，报告和结果在结束时才有。排队中的记录到
/// 开始执行时会重新确定时间范围。
pub(super) fn new_run(
    inspection: &LogInspection,
    trigger: RunTrigger,
    status: RunStatus,
    now: TimestampMicros,
) -> LogInspectionRun {
    LogInspectionRun {
        id: Id::new(),
        inspection_id: inspection.id.clone(),
        org_id: inspection.org_id.clone(),
        trigger,
        status,
        level: None,
        window_start: window_start(inspection, now),
        window_end: now,
        summary: None,
        report: None,
        failure: None,
        error: None,
        provider_id: Some(inspection.provider_id.clone()),
        model: inspection.model.clone(),
        run_as_user_id: Some(inspection.run_as_user_id.clone()),
        requested_by: None,
        tool_calls: Vec::new(),
        prompt_tokens: 0,
        completion_tokens: 0,
        notifications: Vec::new(),
        created_at: now,
        started_at: (status == RunStatus::Running).then_some(now),
        finished_at: None,
    }
}

/// 组织内的名字唯一，由数据库的唯一索引保证；把它的报错换成说得清的一句。
fn name_taken(error: Error) -> Error {
    match error {
        Error::Conflict(_) => {
            Error::conflict("a scheduled log inspection with this name already exists")
        }
        other => other,
    }
}

#[cfg(test)]
mod tests;
