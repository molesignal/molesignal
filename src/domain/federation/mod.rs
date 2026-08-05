// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 跨集群事件总线的统一协议（CloudEvents 1.0 兼容信封）。
//!
//! 一个集群对 org 维度资源（alert rule / dashboard / regex pattern …）做 CUD 时，
//! 落一条 [`CloudEvent`] 进 outbox，后台 worker 推到配了 org 映射的远端集群，接收端
//! 去重 + 映射 + 按 **Lamport 版本**（时钟无关）应用到本地 repo。
//!
//! **统一协议唯一定义点**：加一种资源 = 加一个 [`ResourceKind`] 变体 + 接收端一行 apply
//! 派发，不逐资源膨胀。`event_type` 编码 `资源.动作`，`subject` 编码 `org/资源id`，
//! `data` 携带资源全量 JSON，`id` 是去重键，扩展属性 `xmsversion`/`xmswriter` 做冲突解决。

use serde::{Deserialize, Serialize};

/// `event_type` 前缀：`com.molesignal.<resource>.<action>`。
pub const EVENT_TYPE_PREFIX: &str = "com.molesignal.";

/// 可跨集群同步的 org 维度资源类型。`as_str` 是 `event_type` 中段，也是稳定线协议。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    RegexPattern,
    AlertRule,
    Dashboard,
    EscalationPolicy,
    Schedule,
    LogPattern,
    SemanticGroup,
    MuteRule,
    NotifyTemplate,
}

impl ResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ResourceKind::RegexPattern => "regex_pattern",
            ResourceKind::AlertRule => "alert_rule",
            ResourceKind::Dashboard => "dashboard",
            ResourceKind::EscalationPolicy => "escalation_policy",
            ResourceKind::Schedule => "schedule",
            ResourceKind::LogPattern => "log_pattern",
            ResourceKind::SemanticGroup => "semantic_group",
            ResourceKind::MuteRule => "mute_rule",
            ResourceKind::NotifyTemplate => "notify_template",
        }
    }

    // 稳定线协议解析（非 std `FromStr`：返 Option 而非 Result，名字保持稳定）。
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "regex_pattern" => ResourceKind::RegexPattern,
            "alert_rule" => ResourceKind::AlertRule,
            "dashboard" => ResourceKind::Dashboard,
            "escalation_policy" => ResourceKind::EscalationPolicy,
            "schedule" => ResourceKind::Schedule,
            "log_pattern" => ResourceKind::LogPattern,
            "semantic_group" => ResourceKind::SemanticGroup,
            "mute_rule" => ResourceKind::MuteRule,
            "notify_template" => ResourceKind::NotifyTemplate,
            _ => return None,
        })
    }
}

/// 创建 / 更新 / 删除。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CudAction {
    Created,
    Updated,
    Deleted,
}

impl CudAction {
    pub fn as_str(self) -> &'static str {
        match self {
            CudAction::Created => "created",
            CudAction::Updated => "updated",
            CudAction::Deleted => "deleted",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "created" => CudAction::Created,
            "updated" => CudAction::Updated,
            "deleted" => CudAction::Deleted,
            _ => return None,
        })
    }
}

/// `com.molesignal.<resource>.<action>`。
pub fn event_type(kind: ResourceKind, action: CudAction) -> String {
    format!("{EVENT_TYPE_PREFIX}{}.{}", kind.as_str(), action.as_str())
}

/// 反解 `event_type` → (资源, 动作)；非本协议串返回 None。
pub fn parse_event_type(s: &str) -> Option<(ResourceKind, CudAction)> {
    let rest = s.strip_prefix(EVENT_TYPE_PREFIX)?;
    let (kind_s, action_s) = rest.rsplit_once('.')?;
    Some((
        ResourceKind::from_str(kind_s)?,
        CudAction::from_str(action_s)?,
    ))
}

/// `<org_id>/<resource_id>`（org/资源 id 均为 KSUID，无 `/`）。
pub fn subject(org_id: &str, resource_id: &str) -> String {
    format!("{org_id}/{resource_id}")
}

/// 反解 subject → (org_id, resource_id)。
pub fn parse_subject(s: &str) -> Option<(&str, &str)> {
    s.split_once('/')
}

/// 冲突解决（**时钟无关**）：incoming 是否应覆盖 local。
/// 按 `(version, writer)` 字典序——版本高者胜；版本相同则发起集群 id 大者胜（确定性 tiebreak）。
pub fn wins(incoming: (u64, &str), local: (u64, &str)) -> bool {
    incoming > local
}

/// CloudEvents 1.0 兼容信封（JSON 字段名严格对齐 spec；`xms*` 为合法扩展属性）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudEvent {
    pub specversion: String,
    pub id: String,
    pub source: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub subject: String,
    pub time: String,
    pub datacontenttype: String,
    pub data: serde_json::Value,
    /// 该资源的 Lamport 版本（冲突解决，非 wall-time）。
    pub xmsversion: u64,
    /// 该版本的发起集群 id（version 相同时 tiebreak）。
    pub xmswriter: String,
}

impl CloudEvent {
    /// 构造一条 CUD 事件。`time` 由调用方传 RFC3339 串（信封不依赖时钟做冲突解决）。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        cluster_id: &str,
        kind: ResourceKind,
        action: CudAction,
        org_id: &str,
        resource_id: &str,
        version: u64,
        data: serde_json::Value,
        time: String,
    ) -> Self {
        Self {
            specversion: "1.0".into(),
            id,
            source: format!("cluster:{cluster_id}"),
            event_type: event_type(kind, action),
            subject: subject(org_id, resource_id),
            time,
            datacontenttype: "application/json".into(),
            data,
            xmsversion: version,
            xmswriter: cluster_id.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn event_type_round_trip() {
        for kind in [
            ResourceKind::RegexPattern,
            ResourceKind::AlertRule,
            ResourceKind::Dashboard,
            ResourceKind::SemanticGroup,
        ] {
            for action in [CudAction::Created, CudAction::Updated, CudAction::Deleted] {
                let s = event_type(kind, action);
                assert_eq!(parse_event_type(&s), Some((kind, action)));
            }
        }
        assert_eq!(parse_event_type("com.molesignal.unknown.created"), None);
        assert_eq!(parse_event_type("not.our.prefix.created"), None);
    }

    #[test]
    fn subject_round_trip() {
        let s = subject("orgABC", "res123");
        assert_eq!(parse_subject(&s), Some(("orgABC", "res123")));
    }

    #[test]
    fn wins_is_clock_free_and_deterministic() {
        // 版本高者胜。
        assert!(wins((5, "a"), (4, "z")));
        assert!(!wins((4, "z"), (5, "a")));
        // 版本相同 → writer 大者胜（确定性 tiebreak，与时钟无关）。
        assert!(wins((5, "clusterB"), (5, "clusterA")));
        assert!(!wins((5, "clusterA"), (5, "clusterB")));
        // 完全相同 → 不覆盖（幂等）。
        assert!(!wins((5, "x"), (5, "x")));
    }

    #[test]
    fn cloud_event_serializes_cloudevents_shape() {
        let ev = CloudEvent::new(
            "evt-1".into(),
            "cl-A",
            ResourceKind::RegexPattern,
            CudAction::Created,
            "org-1",
            "rp-9",
            7,
            json!({"id": "rp-9", "name": "ssn"}),
            "2026-06-13T00:00:00Z".into(),
        );
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["specversion"], "1.0");
        assert_eq!(v["type"], "com.molesignal.regex_pattern.created");
        assert_eq!(v["source"], "cluster:cl-A");
        assert_eq!(v["subject"], "org-1/rp-9");
        assert_eq!(v["xmsversion"], 7);
        assert_eq!(v["xmswriter"], "cl-A");
        assert_eq!(v["datacontenttype"], "application/json");
        // round-trip
        let back: CloudEvent = serde_json::from_value(v).unwrap();
        assert_eq!(back.id, "evt-1");
        assert_eq!(
            parse_event_type(&back.event_type),
            Some((ResourceKind::RegexPattern, CudAction::Created))
        );
    }
}
