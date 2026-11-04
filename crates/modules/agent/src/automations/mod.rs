// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 自动化：不用人在场，让 Mole Agent 做一件事。
//!
//! 一条规则 = 名称 + 作用域（组织 / 团队 / 个人）+ 任务提示 + 触发 + 输出。每触发一次是一次运行：
//! 在一个隐藏的会话里排一轮回答（Run，来源 `automation`，事件触发的是 `incident`），以最后保存
//! 规则的人的身份执行；结束后按输出发通知、写回事件的 RCA，并把结果记在运行记录里。
//!
//! - 触发：Cron（带时区，最短 15 分钟一次）、API（规则自己的令牌）、新事件（按严重级别、告警规则、
//!   服务与标签筛选，同一事件只分析一次）、新的自动发现（按种类、最低严重度和对象筛选，同一条发现只
//!   调查一次）、手动。
//! - 运行状态见 [`AutomationRunStatus`]；没有人可问时，提问、确认与缺凭据都不等，记为阻塞。

mod findings;
mod incidents;

use std::collections::BTreeMap;

use async_trait::async_trait;
pub use findings::FindingFilter;
pub use incidents::{IncidentFacts, MERGE_WINDOW_MICROS};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    agent::scope::Scope,
    domain::notify::connector::NotifyTargetType,
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 一个组织最多这么多条规则。
pub const MAX_AUTOMATIONS_PER_ORG: usize = 50;
/// 任务提示最长这么多字符。
pub const MAX_PROMPT_CHARS: usize = 8_000;
/// 一条规则最多发到这么多个通知渠道。
pub const MAX_CHANNELS: usize = 5;
/// API 触发时请求体里的 `text` 最长这么多字符。
pub const MAX_INPUT_CHARS: usize = 8_000;
/// 一次运行默认最多跑这么久，可设 1 到 60 分钟。
pub const DEFAULT_TIMEOUT_SECS: u32 = 600;
pub const MIN_TIMEOUT_SECS: u32 = 60;
pub const MAX_TIMEOUT_SECS: u32 = 3_600;
/// 手动运行之间至少隔这么久。
pub const MANUAL_RUN_INTERVAL_MICROS: i64 = 60_000_000;
/// 每条规则留这么多条运行记录，更早的删掉。
pub const KEPT_RUNS_PER_AUTOMATION: i64 = 100;

/// 什么时候运行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationTrigger {
    /// 按 Cron 表达式（五段：分 时 日 月 周）在一个时区里定时运行。
    Schedule { cron: String, timezone: String },
    /// 外部系统带着规则的令牌调接口触发；请求体里的 `text` 是这一次的上下文。
    Api,
    /// 新事件触发。
    Incident(IncidentFilter),
    /// 自动发现新找到的问题触发。
    Finding(FindingFilter),
    /// 只手动运行。
    Manual,
}

impl AutomationTrigger {
    pub fn kind(&self) -> TriggerKind {
        match self {
            Self::Schedule { .. } => TriggerKind::Schedule,
            Self::Api => TriggerKind::Api,
            Self::Incident(_) => TriggerKind::Incident,
            Self::Finding(_) => TriggerKind::Finding,
            Self::Manual => TriggerKind::Manual,
        }
    }

    /// 事件或发现触发：要从游标往后读新的。
    pub fn reads_events(&self) -> bool {
        matches!(self, Self::Incident(_) | Self::Finding(_))
    }
}

/// 哪些新事件触发：每一项留空表示不限，填了的都要满足。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncidentFilter {
    /// `info`、`warning`、`error`、`critical`。
    #[serde(default)]
    pub severities: Vec<String>,
    /// 告警规则的 ID。
    #[serde(default)]
    pub rule_ids: Vec<String>,
    /// 受影响的服务。
    #[serde(default)]
    pub services: Vec<String>,
    /// 事件标签，值要相等。
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

/// 一次运行是怎么触发的。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKind {
    Schedule,
    Api,
    Incident,
    Finding,
    Manual,
}

impl TriggerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Schedule => "schedule",
            Self::Api => "api",
            Self::Incident => "incident",
            Self::Finding => "finding",
            Self::Manual => "manual",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Schedule,
            Self::Api,
            Self::Incident,
            Self::Finding,
            Self::Manual,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == value)
    }
}

/// 结果发到哪里：一个通知连接器上的固定地址或固定群组。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationChannel {
    pub connector_id: Id,
    pub target_type: NotifyTargetType,
    pub target: String,
}

/// 一次运行结束后做什么。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationOutputs {
    /// 把结果发到这些渠道。
    #[serde(default)]
    pub channels: Vec<AutomationChannel>,
    /// 事件触发的运行把结论写成事件的 RCA。
    #[serde(default)]
    pub write_rca: bool,
}

/// 最近一次运行，列表里显示规则现在的状况。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastAutomationRun {
    pub id: Id,
    pub at: TimestampMicros,
    pub status: AutomationRunStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Automation {
    pub id: Id,
    pub org_id: Id,
    /// 每次运行的会话作用域；创建后不改，换作用域用克隆。
    pub scope: Scope,
    pub name: String,
    pub description: String,
    /// 每次运行时作为问题交给 Mole Agent。
    pub prompt: String,
    pub enabled: bool,
    pub trigger: AutomationTrigger,
    pub outputs: AutomationOutputs,
    /// 以谁的身份运行：最后保存规则的人。
    pub run_as_user_id: Id,
    /// 保存时的界面语言：回答与通知都用它。
    pub language: String,
    pub timeout_secs: u32,
    /// 从哪个内置模板建的。
    pub template: Option<String>,
    /// API 触发令牌的前缀；没有令牌时为空。令牌本身只在生成时出现一次。
    pub api_token_prefix: Option<String>,
    /// 下一次定时运行的时刻。
    pub next_run_at: Option<TimestampMicros>,
    /// 事件、发现触发：这之后创建的事件、发现才算新的。
    pub incident_cursor: Option<TimestampMicros>,
    pub last_run: Option<LastAutomationRun>,
    pub created_by: Id,
    pub updated_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// 一次运行到了哪一步、怎么结束的。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRunStatus {
    /// Mole Agent 正在做。
    Running,
    /// 做完了，结果都送到了。
    Succeeded,
    /// 做完了，但有的通知没发出去，或 RCA 没写成。
    Partial,
    /// 没做完：模型出错、超时，或执行身份不再可用。
    Failed,
    /// 没有开始：预算用完、上一次还没做完，或同一事件刚分析过。
    Skipped,
    /// 做的时候要有人回答、确认或补凭据，没有人可问。
    Blocked,
    /// 被停下或中断了。
    Abandoned,
}

impl AutomationRunStatus {
    pub const ALL: [Self; 7] = [
        Self::Running,
        Self::Succeeded,
        Self::Partial,
        Self::Failed,
        Self::Skipped,
        Self::Blocked,
        Self::Abandoned,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Partial => "partial",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Blocked => "blocked",
            Self::Abandoned => "abandoned",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == value)
    }

    pub fn is_over(self) -> bool {
        self != Self::Running
    }
}

/// 运行没有开始或没有做好的原因：固定的代码，页面写成读者的语言。
pub mod reasons {
    /// 组织今天的 Mole Agent token 预算用完了。
    pub const BUDGET_EXHAUSTED: &str = "budget_exhausted";
    /// 上一次运行还没结束。
    pub const STILL_RUNNING: &str = "still_running";
    /// 同一事件（或同一组事件）刚分析过。
    pub const ALREADY_ANALYZED: &str = "already_analyzed";
    /// 执行身份不再可用：账号停用、离开组织或失去权限。
    pub const RUN_AS_UNAVAILABLE: &str = "run_as_unavailable";
    /// 没有可用的模型服务。
    pub const NO_MODEL: &str = "no_model";
    /// 超过了规则的总时限。
    pub const TIMEOUT: &str = "timeout";
    /// 模型或工具出错。
    pub const ERROR: &str = "error";
    /// 要人回答一个问题。
    pub const QUESTION: &str = "question";
    /// 要人确认执行一个工具。
    pub const CONFIRMATION: &str = "confirmation";
    /// 一台 MCP 服务器要执行身份自己的凭据，而他还没存。
    pub const CREDENTIAL: &str = "credential";
    /// 有通知没有发出去。
    pub const DELIVERY: &str = "delivery";
    /// 事件的 RCA 没写成。
    pub const RCA: &str = "rca";
    /// 被人停下。
    pub const STOPPED: &str = "stopped";
    /// 执行它的服务器停了。
    pub const INTERRUPTED: &str = "interrupted";
}

/// 一次运行。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRun {
    pub id: Id,
    pub org_id: Id,
    pub automation_id: Id,
    pub trigger: TriggerKind,
    pub status: AutomationRunStatus,
    /// [`reasons`] 里的代码，多个时用逗号隔开。
    pub reason: Option<String>,
    /// 隐藏的会话：点开能看到全过程。
    pub chat_id: Option<Id>,
    /// 这一轮回答的 Run。
    pub run_id: Option<Id>,
    /// 事件触发时的事件。
    pub incident_id: Option<Id>,
    /// 同一组事件的键（事件的指纹）：一段时间内只分析一次。
    pub group_key: Option<String>,
    /// 这一次的上下文：API 请求带的文字，或事件的摘要。
    pub input: Option<String>,
    /// 回答的开头，列表里看。
    pub summary: Option<String>,
    /// 通知与 RCA 的结果：`{"deliveries": [...], "rca": true}`。
    pub outputs: Value,
    /// 手动运行的人。
    pub started_by: Option<Id>,
    pub created_at: TimestampMicros,
    pub finished_at: Option<TimestampMicros>,
}

/// 一次运行怎么结束的，回写到运行记录。
#[derive(Debug, Clone, PartialEq)]
pub struct RunEnding {
    pub status: AutomationRunStatus,
    pub reason: Option<String>,
    pub summary: Option<String>,
    pub outputs: Value,
}

#[async_trait]
pub trait AutomationRepository: Send + Sync {
    async fn list(&self, org_id: &Id) -> Result<Vec<Automation>>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<Automation>;
    /// 不知道组织时按 ID 找（API 触发只带规则 ID），连同令牌的哈希。
    async fn get_with_token(&self, id: &Id) -> Result<Option<(Automation, Option<String>)>>;
    async fn count(&self, org_id: &Id) -> Result<usize>;
    async fn create(&self, automation: Automation) -> Result<Automation>;
    async fn update(&self, automation: Automation) -> Result<Automation>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<bool>;
    async fn set_enabled(&self, org_id: &Id, id: &Id, enabled: bool) -> Result<Automation>;
    /// 换上新令牌（哈希与前缀）；`None` 收回令牌。
    async fn set_api_token(
        &self,
        org_id: &Id,
        id: &Id,
        token: Option<(String, String)>,
    ) -> Result<()>;
    /// 领走到点该运行的定时规则，最早的在前：领走时把下一次先推到 `hold_until`，别的节点就不会
    /// 再领；算出真正的下一次后由 [`Self::set_next_run`] 写回。节点在两步之间没了，规则晚一点
    /// 照常运行。
    async fn claim_due(
        &self,
        now: TimestampMicros,
        hold_until: TimestampMicros,
        limit: i64,
    ) -> Result<Vec<Automation>>;
    async fn set_next_run(&self, id: &Id, next: Option<TimestampMicros>) -> Result<()>;
    /// 启用中的事件触发和发现触发的规则，各组织的。
    async fn incident_rules(&self) -> Result<Vec<Automation>>;
    /// 把事件（或发现）游标从 `from` 推到 `to`；别的节点已经推过时返回 false，这一批由它处理。
    async fn advance_incident_cursor(
        &self,
        id: &Id,
        from: Option<TimestampMicros>,
        to: TimestampMicros,
    ) -> Result<bool>;
    /// 组织有没有启用中、会写事件 RCA 的事件触发规则。
    async fn writes_rca(&self, org_id: &Id) -> Result<bool>;

    async fn create_run(&self, run: AutomationRun) -> Result<AutomationRun>;
    async fn attach_run(&self, id: &Id, chat_id: &Id, run_id: &Id) -> Result<()>;
    async fn finish_run(&self, id: &Id, ending: &RunEnding) -> Result<Option<AutomationRun>>;
    async fn get_run(&self, org_id: &Id, id: &Id) -> Result<AutomationRun>;
    async fn run_for_agent_run(&self, run_id: &Id) -> Result<Option<AutomationRun>>;
    async fn list_runs(
        &self,
        org_id: &Id,
        automation_id: &Id,
        limit: i64,
    ) -> Result<Vec<AutomationRun>>;
    /// 还在运行的记录，各组织的：执行它的节点没了时由这里收尾。
    async fn running_runs(&self, limit: i64) -> Result<Vec<AutomationRun>>;
    /// 这条规则分析过这个事件，或在 `since` 之后分析过同一组事件。
    async fn analyzed(
        &self,
        automation_id: &Id,
        incident_id: &Id,
        group_key: &str,
        since: TimestampMicros,
    ) -> Result<bool>;
    /// 每条规则只留最近 `keep` 条运行记录。
    async fn purge_runs(&self, keep: i64) -> Result<u64>;
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn triggers_read_back_with_their_kind() {
        let schedule = AutomationTrigger::Schedule {
            cron: "0 9 * * 1-5".into(),
            timezone: "Asia/Shanghai".into(),
        };
        let value = serde_json::to_value(&schedule).unwrap();
        assert_eq!(value["kind"], "schedule");
        assert_eq!(
            serde_json::from_value::<AutomationTrigger>(value).unwrap(),
            schedule
        );

        let incident: AutomationTrigger = serde_json::from_value(json!({
            "kind": "incident", "severities": ["critical"], "labels": {"env": "prod"}
        }))
        .unwrap();
        let AutomationTrigger::Incident(filter) = &incident else {
            panic!("an incident trigger");
        };
        assert_eq!(filter.severities, vec!["critical".to_string()]);
        assert!(filter.rule_ids.is_empty());
        assert_eq!(incident.kind(), TriggerKind::Incident);

        assert_eq!(
            serde_json::to_value(AutomationTrigger::Api).unwrap(),
            json!({"kind": "api"})
        );
    }

    #[test]
    fn statuses_and_trigger_kinds_round_trip() {
        for status in AutomationRunStatus::ALL {
            assert_eq!(AutomationRunStatus::parse(status.as_str()), Some(status));
        }
        assert!(!AutomationRunStatus::Running.is_over());
        assert!(AutomationRunStatus::Blocked.is_over());

        for kind in [
            TriggerKind::Schedule,
            TriggerKind::Api,
            TriggerKind::Incident,
            TriggerKind::Finding,
            TriggerKind::Manual,
        ] {
            assert_eq!(TriggerKind::parse(kind.as_str()), Some(kind));
        }
    }
}
