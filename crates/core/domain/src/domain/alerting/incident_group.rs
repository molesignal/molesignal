// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Incident grouping（spec incidents-grouping capability）。
//!
//! 相关 incident 按 (scope, group fingerprint) 聚成一个 [`IncidentGroup`]，在时间窗内
//! 复用同一组（count++、last_at 推后）。grouping fingerprint 由 evaluator 在创建
//! incident 时算出——默认用 incident 自身 fingerprint，命中 [`super::semantic_group::
//! SemanticGroup`] 时改用其 `group_by` 派生 key（把跨规则的相关 incident 收拢到一组，
//! 类似 Alertmanager group_by）。
//!
//! 类型 + trait 放 domain（原先在 infra）：app 层 evaluator 创建 incident 时需要直接
//! 调用 [`IncidentGroupRepository::upsert_for_incident`]，而 app 不依赖 infra。Pg 实装
//! 仍在 `molesignal_infra`。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentGroupState {
    Open,
    Acked,
    Resolved,
}

impl IncidentGroupState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Acked => "acked",
            Self::Resolved => "resolved",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "acked" => Self::Acked,
            "resolved" => Self::Resolved,
            _ => Self::Open,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentGroup {
    pub id: Id,
    pub org_id: Id,
    /// 分组作用域 id：默认是触发 incident 的 alert_rule id；命中语义分组时是
    /// `SemanticGroup.id`，于是不同规则的相关 incident 能收拢进同一组。
    pub alert_rule_id: Id,
    pub fingerprint: String,
    pub state: IncidentGroupState,
    pub incident_count: i32,
    pub first_at: TimestampMicros,
    pub last_at: TimestampMicros,
    pub acked_by: Option<Id>,
    pub acked_at: Option<TimestampMicros>,
    pub resolved_at: Option<TimestampMicros>,
}

#[async_trait]
pub trait IncidentGroupRepository: Send + Sync {
    /// 触发时 upsert：同 (org, scope_id, fingerprint) 命中且 last_at 在窗口内、state 非
    /// resolved 则 last_at 推后 + count++；否则新建。返回最终的 group。
    async fn upsert_for_incident(
        &self,
        org_id: &Id,
        alert_rule_id: &Id,
        fingerprint: &str,
        at: TimestampMicros,
        window_secs: i64,
    ) -> Result<IncidentGroup>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<IncidentGroup>;
    async fn list(
        &self,
        org_id: &Id,
        state_filter: Option<IncidentGroupState>,
        limit: i64,
    ) -> Result<Vec<IncidentGroup>>;
    async fn ack(&self, org_id: &Id, id: &Id, acked_by: &Id, at: TimestampMicros) -> Result<()>;
    async fn resolve(&self, org_id: &Id, id: &Id, at: TimestampMicros) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_string_roundtrip() {
        for s in [
            IncidentGroupState::Open,
            IncidentGroupState::Acked,
            IncidentGroupState::Resolved,
        ] {
            assert_eq!(IncidentGroupState::parse(s.as_str()), s);
        }
    }
}
