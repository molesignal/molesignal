// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 升级策略：PagerDuty 风格的多级派发。
//!
//! 一个 [`EscalationPolicy`] 由若干 [`EscalationStep`] 组成。
//! 每一步只指定接收人和 ack 超时；实际连接器、用户偏好与兜底由 Notify
//! Policy 统一解析。Incident 触发后从 step 0 开始，超时未 ack 则前进。

use serde::{Deserialize, Serialize};

use super::incident::Severity;
use crate::shared::ids::Id;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationPolicy {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub steps: Vec<EscalationStep>,
    /// 走完所有步骤后是否循环；true 时配合 max_loops。
    pub repeat: bool,
    pub max_loops: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationStep {
    pub targets: Vec<EscalationTarget>,
    /// 本步派发后多久没 ack 就升级，秒。
    pub ack_timeout_secs: u32,
    /// 仅当 incident.severity >= min_severity 时本步才生效（级别路由）。
    /// None = 不过滤。
    #[serde(default)]
    pub min_severity: Option<Severity>,
}

impl EscalationStep {
    /// 给定 incident 严重度，本 step 是否适用（级别路由过滤）。
    pub fn applies_to(&self, severity: Severity) -> bool {
        self.min_severity.is_none_or(|min| severity >= min)
    }
}

/// 派发目标。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EscalationTarget {
    /// 直接指定到具体用户。
    User { user_id: Id },
    /// 通过排班决定当前值班人。
    Schedule { schedule_id: Id },
    /// 广播到整个团队。
    Team { team_id: Id },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 3 个 target variant 都 round-trip 一致，避免 escalation_policies.steps JSON 写入 ↔ 读出
    /// 时丢失字段。
    #[test]
    fn escalation_target_serde_roundtrip() {
        let targets = vec![
            EscalationTarget::User {
                user_id: Id("u1".into()),
            },
            EscalationTarget::Schedule {
                schedule_id: Id("s1".into()),
            },
            EscalationTarget::Team {
                team_id: Id("t1".into()),
            },
        ];
        for t in &targets {
            let s = serde_json::to_string(t).expect("serialize");
            let parsed: EscalationTarget = serde_json::from_str(&s).expect("deserialize");
            assert_eq!(&parsed, t, "roundtrip mismatch for {s}");
        }
    }
}
