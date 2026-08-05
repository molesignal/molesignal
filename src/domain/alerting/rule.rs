// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use super::{anomaly::AnomalyParams, incident::Severity};
use crate::{
    domain::query::{QueryLanguage, StreamHint},
    shared::{ids::Id, time::TimestampMicros},
};

/// 规则求值类型。不同 kind 分派到不同评估管线：
/// - [`Scheduled`](AlertRuleKind::Scheduled)：周期性阈值评估（默认，向后兼容）。
/// - [`RealTime`](AlertRuleKind::RealTime)：ingester 内逐事件匹配（当前 evaluator 仍按
///   scheduled 周期跑，realtime matcher 接入是单独 follow-up）。
/// - [`Anomaly`](AlertRuleKind::Anomaly)：基线对比，携带 [`AnomalyParams`]。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertRuleKind {
    #[default]
    Scheduled,
    RealTime,
    Anomaly,
}

impl AlertRuleKind {
    /// 持久化到 `alert_rules.kind`（VARCHAR(16)）用的稳定字符串。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Scheduled => "scheduled",
            Self::RealTime => "real_time",
            Self::Anomaly => "anomaly",
        }
    }
    /// 从持久化字符串解析；未知值回退 [`Scheduled`](AlertRuleKind::Scheduled)。
    pub fn parse(s: &str) -> Self {
        match s {
            "anomaly" => Self::Anomaly,
            "real_time" | "realtime" => Self::RealTime,
            _ => Self::Scheduled,
        }
    }
}

/// 告警规则。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    /// 求值类型；缺省（历史 JSON）反序列化为 [`Scheduled`](AlertRuleKind::Scheduled)。
    #[serde(default)]
    pub kind: AlertRuleKind,
    pub query: AlertQuery,
    pub trigger: AlertTrigger,
    /// 仅 `kind = Anomaly` 携带；其它 kind 为 `None`。
    #[serde(default)]
    pub anomaly_params: Option<AnomalyParams>,
    /// 多档严重度阈值；空 = 走单档 `trigger`（向后兼容）。逐档评估，取最高命中档定 severity。
    #[serde(default)]
    pub thresholds: Vec<SeverityThreshold>,
    /// 单档规则的显式兜底严重度；`None` = 由评估器推导（determine_severity）。
    #[serde(default)]
    pub severity: Option<Severity>,
    /// 关联的升级策略 ID（由 [`super::escalation::EscalationPolicy`] 定义）
    pub escalation_policy_id: Id,
    pub labels: std::collections::BTreeMap<String, String>,
    pub annotations: std::collections::BTreeMap<String, String>,
    /// 上一次评估完成的时间（None = 从未评估过）。
    pub last_eval_at: Option<TimestampMicros>,
    /// 上一次评估完成后规则所处状态；evaluator 重启会丢，但 fingerprint 去重兜底。
    #[serde(default)]
    pub last_state: RuleState,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// 评估状态机：用于 evaluator 内存里跟踪 `for_periods` 计数 + 触发去抖。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "consecutive")]
pub enum RuleState {
    /// 最近一次评估未命中阈值。
    #[default]
    Healthy,
    /// 已连续命中 N 次（< for_periods），还在去抖窗口。
    Pending(u32),
    /// 已触发，并创建/更新过 incident。
    Firing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertQuery {
    pub language: QueryLanguage,
    pub statement: String,
    /// 评估窗口（秒）。
    pub period_secs: u32,
    /// 命中查询的 stream（表）名 + 类型。当前查询引擎要求显式指定，
    /// evaluator 据此把 `QueryRequest.stream` 填出来。
    /// 历史持久化的 JSON 可能没有该字段：反序列化为 None，
    /// evaluator 看到 None 会跳过该规则并打一条 warn，不再调底层引擎。
    #[serde(default)]
    pub stream: Option<StreamHint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertTrigger {
    pub operator: ComparisonOp,
    pub threshold: f64,
    /// 满足条件持续多少个评估周期才真正触发，防抖。
    pub for_periods: u32,
    /// 触发后静默多久不再重复触发（秒）。
    pub silence_secs: u32,
}

/// 多档阈值的一档：值满足 `operator` 与 `threshold` 比较、且连续 `for_periods` 个周期命中时，
/// 该档触发并把 incident 严重度映射为 `severity`。一条规则的多档逐档独立评估、取最高命中档。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeverityThreshold {
    pub severity: Severity,
    pub operator: ComparisonOp,
    pub threshold: f64,
    pub for_periods: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonOp {
    Gt,
    Gte,
    Lt,
    Lte,
    Eq,
    Neq,
}
