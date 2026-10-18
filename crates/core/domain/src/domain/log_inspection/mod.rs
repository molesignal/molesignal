// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 定时日志巡检：按周期让 Mole Agent 以只读方式巡检一个日志数据流，保存报告，判定有异常时通知。
//!
//! 一条巡检（[`LogInspection`]）说明查什么、多久查一次、用哪个模型、什么时候通知谁；每次执行
//! 留下一条 [`LogInspectionRun`]，报告正文和结论等级都在这一条上。

pub mod repositories;

use serde::{Deserialize, Serialize};

use crate::{
    domain::notify::connector::NotifyTargetType,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 多久执行一次。带时刻的周期按巡检的时区（[`LogInspection::timezone`]）解释。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InspectionSchedule {
    /// 每隔 `every_minutes` 分钟一次，从保存的时刻起算。
    Interval { every_minutes: u32 },
    /// 每天 `hour:minute`。
    Daily { hour: u8, minute: u8 },
    /// 每周 `weekday`（1 = 周一 … 7 = 周日）的 `hour:minute`。
    Weekly { weekday: u8, hour: u8, minute: u8 },
}

impl InspectionSchedule {
    /// 相邻两次执行之间的最短间隔（秒）。
    pub fn period_secs(&self) -> u64 {
        match self {
            Self::Interval { every_minutes } => u64::from(*every_minutes) * 60,
            Self::Daily { .. } => 24 * 3600,
            Self::Weekly { .. } => 7 * 24 * 3600,
        }
    }
}

/// 报告是什么结论时发通知。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyWhen {
    /// 只在结论是异常时。
    #[default]
    Abnormal,
    /// 结论是需关注或异常时。
    Attention,
    /// 每次都发。
    Always,
}

impl NotifyWhen {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Abnormal => "abnormal",
            Self::Attention => "attention",
            Self::Always => "always",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "abnormal" => Ok(Self::Abnormal),
            "attention" => Ok(Self::Attention),
            "always" => Ok(Self::Always),
            other => Err(Error::internal(format!(
                "unknown log inspection notify_when: {other}"
            ))),
        }
    }

    /// 这个等级的报告要不要通知。没有判定出等级的报告没人能说它正常，按异常对待。
    pub const fn covers(self, level: InspectionLevel) -> bool {
        match self {
            Self::Always => true,
            Self::Attention => !matches!(level, InspectionLevel::Normal),
            Self::Abnormal => matches!(level, InspectionLevel::Abnormal | InspectionLevel::Unknown),
        }
    }
}

/// 一份报告的结论等级，由模型在报告里给出。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionLevel {
    Normal,
    Attention,
    Abnormal,
    /// 报告里没有可读的等级。
    Unknown,
}

impl InspectionLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Attention => "attention",
            Self::Abnormal => "abnormal",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "normal" => Ok(Self::Normal),
            "attention" => Ok(Self::Attention),
            "abnormal" => Ok(Self::Abnormal),
            "unknown" => Ok(Self::Unknown),
            other => Err(Error::internal(format!(
                "unknown log inspection level: {other}"
            ))),
        }
    }
}

/// 通知发到哪里：一个通知连接器上的固定地址或固定群组。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectionNotifyRoute {
    pub connector_id: Id,
    pub target_type: NotifyTargetType,
    pub target: String,
}

/// 最近一次结束的执行，列表里用来显示巡检现在的状况。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastInspectionRun {
    pub at: TimestampMicros,
    pub status: RunStatus,
    /// 只有成功的执行才有结论等级。
    pub level: Option<InspectionLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogInspection {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    /// 被巡检的日志数据流。
    pub stream: String,
    /// 保存时页面上的查询语句；空表示巡检该数据流的全部日志。
    pub statement: String,
    /// 每次巡检覆盖执行时刻之前多长时间（秒）。
    pub window_secs: i64,
    /// 巡检要求：要关注什么、什么算预期之内。可以为空。
    pub requirements: String,
    pub schedule: InspectionSchedule,
    /// IANA 时区名：每天、每周的时刻按它解释，通知里的时间也按它写。
    pub timezone: String,
    /// 执行巡检的模型配置（`agent_model_providers.id`）。
    pub provider_id: Id,
    /// 模型名；`None` 表示每次执行时用该配置的默认模型。
    pub model: Option<String>,
    pub notify_when: NotifyWhen,
    pub notify_routes: Vec<InspectionNotifyRoute>,
    /// 报告与通知用的语言，小写语言代码（`en-us`、`zh-cn`）。
    pub language: String,
    pub enabled: bool,
    /// 最后一次保存这条巡检的登录用户：每次执行都以其当时的权限读取日志。
    pub run_as_user_id: Id,
    /// 下一次定时执行的时刻；停用时为 `None`。
    pub next_run_at: Option<TimestampMicros>,
    pub last_run: Option<LastInspectionRun>,
    pub created_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// 一次执行是谁发起的。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunTrigger {
    /// 到了周期。
    Schedule,
    /// 有人点了「立即执行」。
    Manual,
}

impl RunTrigger {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Schedule => "schedule",
            Self::Manual => "manual",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "schedule" => Ok(Self::Schedule),
            "manual" => Ok(Self::Manual),
            other => Err(Error::internal(format!(
                "unknown log inspection run trigger: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// 手动触发后等后台来执行。
    Queued,
    Running,
    Succeeded,
    Failed,
    /// 超过了单次巡检的总超时。
    TimedOut,
}

impl RunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "timed_out" => Ok(Self::TimedOut),
            other => Err(Error::internal(format!(
                "unknown log inspection run status: {other}"
            ))),
        }
    }

    /// 还没结束的执行：同一条巡检同一时刻只能有一条。
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }

    /// 没有产出报告的结束状态。
    pub const fn is_failure(self) -> bool {
        matches!(self, Self::Failed | Self::TimedOut)
    }
}

/// 一次执行为什么没有产出报告。页面和通知按这个原因写说明，`error` 里只放技术细节。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunFailure {
    /// 超过了单次巡检的总超时。
    TimedOut,
    /// 执行它的节点中途退出。
    Interrupted,
    /// 排队后一直没有节点来执行。
    NotStarted,
    /// 保存者的身份用不了了：账号不存在、不能登录，或失去了所需权限。
    Identity,
    /// 所选的模型配置不存在、已停用或没有密钥。
    Model,
    /// 模型调用失败。
    Agent,
    /// 组织今天的 Mole Agent token 预算用完了。
    Budget,
    Internal,
}

impl RunFailure {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TimedOut => "timed_out",
            Self::Interrupted => "interrupted",
            Self::NotStarted => "not_started",
            Self::Identity => "identity",
            Self::Model => "model",
            Self::Agent => "agent",
            Self::Budget => "budget",
            Self::Internal => "internal",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "timed_out" => Ok(Self::TimedOut),
            "interrupted" => Ok(Self::Interrupted),
            "not_started" => Ok(Self::NotStarted),
            "identity" => Ok(Self::Identity),
            "model" => Ok(Self::Model),
            "agent" => Ok(Self::Agent),
            "budget" => Ok(Self::Budget),
            "internal" => Ok(Self::Internal),
            other => Err(Error::internal(format!(
                "unknown log inspection run failure: {other}"
            ))),
        }
    }
}

/// 巡检过程中模型调用过的一个工具。只记名字和成败，不记查询结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectionToolCall {
    pub name: String,
    pub ok: bool,
}

/// 一次执行向一个通知渠道投递的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectionNotification {
    pub connector_id: Id,
    pub target_type: NotifyTargetType,
    /// 打过码的目标地址，与通知投递记录里的一致。
    pub target_masked: String,
    pub delivered: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogInspectionRun {
    pub id: Id,
    pub inspection_id: Id,
    pub org_id: Id,
    pub trigger: RunTrigger,
    pub status: RunStatus,
    pub level: Option<InspectionLevel>,
    /// 这次巡检覆盖的时间范围。排队中的执行在开始时才确定。
    pub window_start: TimestampMicros,
    pub window_end: TimestampMicros,
    /// 报告的一句话结论，列表里显示。
    pub summary: Option<String>,
    /// 模型写的报告原文。
    pub report: Option<String>,
    /// 没有产出报告的原因。
    pub failure: Option<RunFailure>,
    /// 失败的技术细节，原样来自出错的那一层。
    pub error: Option<String>,
    pub provider_id: Option<Id>,
    pub model: Option<String>,
    /// 这次执行用的是谁的权限。
    pub run_as_user_id: Option<Id>,
    /// 手动触发的人。
    pub requested_by: Option<Id>,
    pub tool_calls: Vec<InspectionToolCall>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub notifications: Vec<InspectionNotification>,
    pub created_at: TimestampMicros,
    pub started_at: Option<TimestampMicros>,
    pub finished_at: Option<TimestampMicros>,
}

/// 一次执行结束时要落到执行记录上的内容。
#[derive(Debug, Clone)]
pub struct RunCompletion {
    pub status: RunStatus,
    pub level: Option<InspectionLevel>,
    pub summary: Option<String>,
    pub report: Option<String>,
    pub failure: Option<RunFailure>,
    pub error: Option<String>,
    pub provider_id: Option<Id>,
    pub model: Option<String>,
    pub run_as_user_id: Option<Id>,
    pub tool_calls: Vec<InspectionToolCall>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub finished_at: TimestampMicros,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_when_covers_the_levels_it_names() {
        use InspectionLevel::{Abnormal, Attention, Normal, Unknown};

        for (when, notified) in [
            (NotifyWhen::Abnormal, [false, false, true, true]),
            (NotifyWhen::Attention, [false, true, true, true]),
            (NotifyWhen::Always, [true, true, true, true]),
        ] {
            let covered = [Normal, Attention, Abnormal, Unknown].map(|level| when.covers(level));
            assert_eq!(covered, notified, "{when:?}");
        }
    }

    #[test]
    fn stored_names_read_back_as_the_same_value() {
        for when in [
            NotifyWhen::Abnormal,
            NotifyWhen::Attention,
            NotifyWhen::Always,
        ] {
            assert_eq!(NotifyWhen::parse(when.as_str()).unwrap(), when);
        }

        for level in [
            InspectionLevel::Normal,
            InspectionLevel::Attention,
            InspectionLevel::Abnormal,
            InspectionLevel::Unknown,
        ] {
            assert_eq!(InspectionLevel::parse(level.as_str()).unwrap(), level);
        }

        for status in [
            RunStatus::Queued,
            RunStatus::Running,
            RunStatus::Succeeded,
            RunStatus::Failed,
            RunStatus::TimedOut,
        ] {
            assert_eq!(RunStatus::parse(status.as_str()).unwrap(), status);
        }

        for trigger in [RunTrigger::Schedule, RunTrigger::Manual] {
            assert_eq!(RunTrigger::parse(trigger.as_str()).unwrap(), trigger);
        }

        for failure in [
            RunFailure::TimedOut,
            RunFailure::Interrupted,
            RunFailure::NotStarted,
            RunFailure::Identity,
            RunFailure::Model,
            RunFailure::Agent,
            RunFailure::Internal,
        ] {
            assert_eq!(RunFailure::parse(failure.as_str()).unwrap(), failure);
        }
    }

    #[test]
    fn a_schedule_is_stored_with_its_kind_named() {
        let weekly = InspectionSchedule::Weekly {
            weekday: 1,
            hour: 9,
            minute: 30,
        };

        assert_eq!(
            serde_json::to_value(&weekly).unwrap(),
            serde_json::json!({ "type": "weekly", "weekday": 1, "hour": 9, "minute": 30 })
        );
        assert_eq!(weekly.period_secs(), 7 * 24 * 3600);
        assert_eq!(
            InspectionSchedule::Interval { every_minutes: 90 }.period_secs(),
            5400
        );
    }
}
