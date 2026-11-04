// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 事件的时间线：谁在什么时候对事件做了什么，以及升级走到了哪一步。
//!
//! 触发和自动恢复从事件本身读出，不另记；通知了谁从投递记录读出。这里记的是其余的经过。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

/// 一条备注最多多少字。
pub const MAX_NOTE_CHARS: usize = 4000;

/// 时间线上一条记录是什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineKind {
    /// 有人确认了事件。`detail.via` 说从哪里确认的。
    Acknowledged,
    /// 有人解决了事件。
    Resolved,
    /// 升级到了某一步：`detail` 有 `step`、`loop`、`reason`、`targets`。
    Escalated,
    /// 转派：`detail.to` 是新的成员、团队或升级策略，`detail.from` 是原来的。
    Reassigned,
    /// 增派了成员：`detail.user_ids`。
    RespondersAdded,
    /// 暂缓到 `detail.until`。
    Snoozed,
    /// 取消了暂缓。
    SnoozeCancelled,
    /// 退回未确认，重新通知：`detail.reason` 是 `snooze_ended` 或 `ack_timeout`。
    Retriggered,
    /// 改了严重度：`detail.from`、`detail.to`。
    SeverityChanged,
    /// 这个事件并入了 `detail.into`。
    Merged,
    /// 别的事件并入了这个事件：`detail.incidents`。
    MergedIn,
    /// 并入的事件 `detail.incident_id` 自己恢复了。
    MergedRecovered,
    /// 备注：`detail.text`。
    Note,
    /// 发起了修复动作：`detail.run_id`、`detail.action`，等审批。
    RemediationRequested,
    /// 修复动作执行完：`detail.status`（succeeded / failed）、`detail.output`。
    RemediationExecuted,
    /// 修复动作的验证：`detail.passed`、`detail.by`。
    RemediationVerified,
    /// 建了工单：`detail.system`、`detail.key`、`detail.url`。
    TicketCreated,
    /// 工单状态变了：`detail.key`、`detail.status`、`detail.done`。
    TicketUpdated,
    /// 宣布为重大事件：`detail.auto` 说是不是自动规则宣布的。
    DeclaredMajor,
    /// 指派了角色：`detail.role`、`detail.user_id`（空为撤下）。
    RoleAssigned,
    /// 建了作战室：`detail.platform`、`detail.url`；`detail.meeting_url` 是会议链接。
    WarRoomCreated,
    /// 加了任务：`detail.title`。
    TaskAdded,
    /// 完成了任务：`detail.title`。
    TaskDone,
    /// 发了进展：`detail.impact`、`detail.current`、`detail.next`。
    StatusUpdate,
    /// 这一级谁都收不到告警（没人值班，或点名的人都没设好通知方式），没等确认超时就升到下一级：
    /// `detail.step`（从 0 数，同升级）、`detail.user_ids`。
    StepUnreachable,
    /// 改了优先级：`detail.from`、`detail.to`（空为不设）。
    PriorityChanged,
    /// 改了紧急度：`detail.from`、`detail.to`、`detail.reason`（手动为空，`support_hours` 是支持时段开始）。
    UrgencyChanged,
    /// 开事件前经过一条告警流水线：`detail.pipeline_id`、`detail.pipeline`、`detail.changes`（改了哪些
    /// 字段：`field`、`from`、`to`）、`detail.more`（记不下的还有几处），出错时 `detail.error`。
    PipelineProcessed,
    /// 被抑制了，通知压着：`detail.rule_id`、`detail.rule_name`、`detail.source_incident_id`、
    /// `detail.source_summary`。
    Inhibited,
    /// 抑制结束，压着的通知发出：`detail.reason`（`source_recovered` 源事件恢复，`rule_changed` 规则改了或
    /// 删了，`lifted` 有人说了照常通知）。
    InhibitionEnded,
}

impl TimelineKind {
    pub const ALL: [Self; 30] = [
        Self::Acknowledged,
        Self::Resolved,
        Self::Escalated,
        Self::Reassigned,
        Self::RespondersAdded,
        Self::Snoozed,
        Self::SnoozeCancelled,
        Self::Retriggered,
        Self::SeverityChanged,
        Self::Merged,
        Self::MergedIn,
        Self::MergedRecovered,
        Self::Note,
        Self::RemediationRequested,
        Self::RemediationExecuted,
        Self::RemediationVerified,
        Self::TicketCreated,
        Self::TicketUpdated,
        Self::DeclaredMajor,
        Self::RoleAssigned,
        Self::WarRoomCreated,
        Self::TaskAdded,
        Self::TaskDone,
        Self::StatusUpdate,
        Self::StepUnreachable,
        Self::PriorityChanged,
        Self::UrgencyChanged,
        Self::PipelineProcessed,
        Self::Inhibited,
        Self::InhibitionEnded,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Acknowledged => "acknowledged",
            Self::Resolved => "resolved",
            Self::Escalated => "escalated",
            Self::Reassigned => "reassigned",
            Self::RespondersAdded => "responders_added",
            Self::Snoozed => "snoozed",
            Self::SnoozeCancelled => "snooze_cancelled",
            Self::Retriggered => "retriggered",
            Self::SeverityChanged => "severity_changed",
            Self::Merged => "merged",
            Self::MergedIn => "merged_in",
            Self::MergedRecovered => "merged_recovered",
            Self::Note => "note",
            Self::RemediationRequested => "remediation_requested",
            Self::RemediationExecuted => "remediation_executed",
            Self::RemediationVerified => "remediation_verified",
            Self::TicketCreated => "ticket_created",
            Self::TicketUpdated => "ticket_updated",
            Self::DeclaredMajor => "declared_major",
            Self::RoleAssigned => "role_assigned",
            Self::WarRoomCreated => "war_room_created",
            Self::TaskAdded => "task_added",
            Self::TaskDone => "task_done",
            Self::StatusUpdate => "status_update",
            Self::StepUnreachable => "step_unreachable",
            Self::PriorityChanged => "priority_changed",
            Self::UrgencyChanged => "urgency_changed",
            Self::PipelineProcessed => "pipeline_processed",
            Self::Inhibited => "inhibited",
            Self::InhibitionEnded => "inhibition_ended",
        }
    }

    pub fn parse(word: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == word)
            .ok_or_else(|| Error::internal(format!("unknown incident timeline kind `{word}`")))
    }
}

/// 操作从哪里来：时间线上写「在飞书里确认」之类。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionVia {
    #[default]
    Web,
    Api,
    Im,
    Sms,
    Phone,
    Agent,
}

impl ActionVia {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Api => "api",
            Self::Im => "im",
            Self::Sms => "sms",
            Self::Phone => "phone",
            Self::Agent => "agent",
        }
    }
}

/// 时间线上的一条记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEntry {
    pub id: Id,
    pub org_id: Id,
    pub incident_id: Id,
    pub at: TimestampMicros,
    pub kind: TimelineKind,
    /// 谁做的；系统做的（升级、暂缓到点、并入的事件自己恢复）为空。
    pub actor: Option<Id>,
    #[serde(default)]
    pub detail: Value,
}

impl TimelineEntry {
    pub fn new(
        org_id: &Id,
        incident_id: &Id,
        at: TimestampMicros,
        kind: TimelineKind,
        actor: Option<Id>,
        detail: Value,
    ) -> Self {
        Self {
            id: Id::new(),
            org_id: org_id.clone(),
            incident_id: incident_id.clone(),
            at,
            kind,
            actor,
            detail,
        }
    }
}

/// 整理一条备注：去掉两头空白，不能是空的，也不能太长。
pub fn note_text(text: &str) -> Result<String> {
    let text = text.trim();

    if text.is_empty() {
        return Err(Error::invalid("a note cannot be empty"));
    }

    if text.chars().count() > MAX_NOTE_CHARS {
        return Err(Error::invalid(format!(
            "a note is at most {MAX_NOTE_CHARS} characters"
        )));
    }

    Ok(text.to_string())
}

#[async_trait]
pub trait IncidentTimelineRepository: Send + Sync {
    async fn append(&self, entry: TimelineEntry) -> Result<TimelineEntry>;
    /// 一个事件的全部记录，按时间先后。
    async fn list(&self, org_id: &Id, incident_id: &Id) -> Result<Vec<TimelineEntry>>;
    /// 一个事件最近的几条备注，新的在前。
    async fn recent_notes(
        &self,
        org_id: &Id,
        incident_id: &Id,
        limit: u32,
    ) -> Result<Vec<TimelineEntry>>;
    /// 这些事件里这几种记录，按时间先后：洞察一次读完一段时间的事件。
    async fn list_for_incidents(
        &self,
        org_id: &Id,
        incident_ids: &[Id],
        kinds: &[TimelineKind],
    ) -> Result<Vec<TimelineEntry>> {
        let mut entries = Vec::new();

        for incident_id in incident_ids {
            entries.extend(
                self.list(org_id, incident_id)
                    .await?
                    .into_iter()
                    .filter(|entry| kinds.contains(&entry.kind)),
            );
        }

        entries.sort_by_key(|entry| entry.at.0);
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_reads_back_from_its_word() {
        for kind in TimelineKind::ALL {
            assert_eq!(
                TimelineKind::parse(kind.as_str()).expect("known word"),
                kind
            );

            let json = serde_json::to_string(&kind).expect("serialize");
            assert_eq!(json, format!("\"{}\"", kind.as_str()));
        }

        assert!(TimelineKind::parse("nope").is_err());
    }

    #[test]
    fn a_note_is_trimmed_and_bounded() {
        assert_eq!(note_text("  回滚了 v2.3 ").expect("note"), "回滚了 v2.3");
        assert!(note_text("   ").is_err());
        assert!(note_text(&"字".repeat(MAX_NOTE_CHARS)).is_ok());
        assert!(note_text(&"字".repeat(MAX_NOTE_CHARS + 1)).is_err());
    }
}
