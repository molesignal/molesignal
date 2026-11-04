// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 接入留下的记录：收到的每次请求，以及外部来源开的事件在来源这一侧的状态。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::alert::{AlertMember, ExternalAction};
use crate::{
    domain::alerting::incident::Severity,
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 一次请求最多记几条告警的结果。
pub const MAX_RECORDED_ALERTS: usize = 100;
/// 被拒的请求最多留多少字节的请求体。
pub const MAX_BODY_EXCERPT_BYTES: usize = 16 * 1024;
/// 每个来源保留最近多少次请求。
pub const KEPT_REQUESTS: u32 = 500;
/// 请求最长保留多久。
pub const KEPT_REQUESTS_MICROS: i64 = 7 * 86_400_000_000;

/// 一次请求的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntakeStatus {
    /// 读懂了，里面的告警都处理过（各条的结果见 [`IntakeAlert::outcome`]）。
    Accepted,
    /// 没处理：Token 之外的原因，例如请求体读不懂、来源已停用。
    Rejected,
    /// 交给了来源的接收集群。
    Forwarded,
}

impl IntakeStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Forwarded => "forwarded",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [Self::Accepted, Self::Rejected, Self::Forwarded]
            .into_iter()
            .find(|status| status.as_str() == value)
    }
}

/// 一条告警被怎么处理了。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertOutcome {
    /// 开了新事件。
    Opened,
    /// 改了进行中事件的严重度或分组成员。
    Updated,
    /// 进行中的事件已经是这样。
    Unchanged,
    Acknowledged,
    Resolved,
    /// 没动任何事件，原因见 [`IntakeAlert::reason`]。
    Ignored,
}

/// 一次请求里的一条告警，和它的结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntakeAlert {
    pub action: ExternalAction,
    pub dedup_key: String,
    pub summary: String,
    #[serde(default)]
    pub severity: Option<Severity>,
    pub outcome: AlertOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incident_id: Option<Id>,
}

/// 收到的一次请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntakeRequest {
    pub id: Id,
    pub org_id: Id,
    pub source_id: Id,
    pub received_at: TimestampMicros,
    /// 经哪个集群转来（集群 ID）；本集群直接收到的为空。
    #[serde(default)]
    pub via_cluster: String,
    pub status: IntakeStatus,
    /// 回给发送方的 HTTP 状态码。
    pub http_status: u16,
    /// 为什么被拒，或者为什么读懂了却什么都没做（例如 PagerDuty 的事件不是告警状态的变化）。
    #[serde(default)]
    pub message: Option<String>,
    /// 请求里一共几条告警；`alerts` 最多记 [`MAX_RECORDED_ALERTS`] 条。
    pub alert_count: u32,
    #[serde(default)]
    pub alerts: Vec<IntakeAlert>,
    /// 被拒的请求体开头，最多 [`MAX_BODY_EXCERPT_BYTES`] 字节。
    #[serde(default)]
    pub body_excerpt: Option<String>,
}

/// 外部来源开的事件在来源这一侧的状态。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceIncident {
    pub incident_id: Id,
    pub org_id: Id,
    pub source_id: Id,
    pub dedup_key: String,
    /// 这个去重键上用到过的最晚的发送方时间。
    #[serde(default)]
    pub last_event_at: Option<TimestampMicros>,
    /// 通知分组里的各条告警，每收到一次通知更新一次。
    #[serde(default)]
    pub members: Vec<AlertMember>,
    pub last_intake_at: TimestampMicros,
}

#[async_trait]
pub trait IntakeRequestRepository: Send + Sync {
    /// 记下一次请求，并删掉这个来源超出 [`KEPT_REQUESTS`] 次或 [`KEPT_REQUESTS_MICROS`] 的旧记录。
    async fn record(&self, request: IntakeRequest) -> Result<()>;
    /// 一个来源最近的请求，新的在前。
    async fn list(&self, org_id: &Id, source_id: &Id, limit: u32) -> Result<Vec<IntakeRequest>>;
}

#[async_trait]
pub trait SourceIncidentRepository: Send + Sync {
    async fn get(&self, incident_id: &Id) -> Result<Option<SourceIncident>>;
    /// 写入或更新；`last_event_at` 只会往后走。
    async fn upsert(&self, row: SourceIncident) -> Result<()>;
    /// 给 `key` 加一把跨节点的锁：同一个去重键上的告警一条一条处理，不管请求落在哪个节点上。
    /// 锁在 [`KeyLock::release`] 或丢弃时放开。
    async fn lock(&self, key: &str) -> Result<Box<dyn KeyLock>>;
}

/// [`SourceIncidentRepository::lock`] 拿到的锁。
#[async_trait]
pub trait KeyLock: Send {
    async fn release(self: Box<Self>) -> Result<()>;
}
