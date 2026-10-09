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

/// 规则不指定升级策略时的取值。它不是真实的策略 id，而是别名：解析时落到所在组织里
/// 名为 `default` 的策略，见 [`EscalationPolicy::resolve`]。
pub const DEFAULT_POLICY_ID: &str = "default";

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

impl EscalationPolicy {
    /// 在同一组织的策略里找 `reference` 指向的那条：先按 id 精确匹配；`reference` 是
    /// [`DEFAULT_POLICY_ID`] 别名时，再找名为 `default` 的策略（不区分大小写）。
    pub fn resolve<'a>(policies: &'a [Self], reference: &Id) -> Option<&'a Self> {
        policies
            .iter()
            .find(|policy| policy.id == *reference)
            .or_else(|| {
                if reference.0 != DEFAULT_POLICY_ID {
                    return None;
                }
                policies
                    .iter()
                    .find(|policy| policy.name.trim().eq_ignore_ascii_case(DEFAULT_POLICY_ID))
            })
    }
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

/// 创建或更新策略时提交的升级步骤。`ack_timeout_secs` 可省略，省略时取
/// `[alert].default_ack_timeout_secs`，保存后每一步都带有确定的超时。
#[derive(Debug, Clone, Deserialize)]
pub struct EscalationStepInput {
    pub targets: Vec<EscalationTarget>,
    #[serde(default)]
    pub ack_timeout_secs: Option<u32>,
    #[serde(default)]
    pub min_severity: Option<Severity>,
}

impl EscalationStepInput {
    pub fn into_step(self, default_ack_timeout_secs: u32) -> EscalationStep {
        EscalationStep {
            targets: self.targets,
            ack_timeout_secs: self.ack_timeout_secs.unwrap_or(default_ack_timeout_secs),
            min_severity: self.min_severity,
        }
    }
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

    fn policy(id: &str, name: &str) -> EscalationPolicy {
        EscalationPolicy {
            id: Id::from_string(id),
            org_id: Id::from_string("org"),
            name: name.to_owned(),
            steps: Vec::new(),
            repeat: false,
            max_loops: 1,
        }
    }

    #[test]
    fn resolve_prefers_exact_id_then_default_name() {
        let policies = vec![policy("p1", "Primary"), policy("p2", " Default ")];

        let exact = EscalationPolicy::resolve(&policies, &Id::from_string("p1"));
        assert_eq!(exact.map(|p| p.id.0.as_str()), Some("p1"));

        let alias = EscalationPolicy::resolve(&policies, &Id::from_string(DEFAULT_POLICY_ID));
        assert_eq!(alias.map(|p| p.id.0.as_str()), Some("p2"));

        assert!(EscalationPolicy::resolve(&policies, &Id::from_string("missing")).is_none());
    }

    #[test]
    fn default_alias_without_a_default_policy_matches_nothing() {
        let policies = vec![policy("p1", "Primary")];
        assert!(
            EscalationPolicy::resolve(&policies, &Id::from_string(DEFAULT_POLICY_ID)).is_none()
        );
    }

    #[test]
    fn omitted_ack_timeout_takes_the_configured_default() {
        let omitted: EscalationStepInput =
            serde_json::from_str(r#"{"targets":[{"kind":"user","user_id":"u1"}]}"#)
                .expect("ack_timeout_secs is optional");
        assert_eq!(omitted.into_step(420).ack_timeout_secs, 420);

        let explicit: EscalationStepInput =
            serde_json::from_str(r#"{"targets":[],"ack_timeout_secs":0,"min_severity":null}"#)
                .expect("explicit timeout");
        assert_eq!(
            explicit.into_step(420).ack_timeout_secs,
            0,
            "an explicit zero is kept"
        );
    }

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
