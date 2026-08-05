// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    domain::query::QueryLanguage,
    shared::{ids::Id, time::TimestampMicros},
};

/// 告警事件 — 规则触发后产生的实例，是排班升级机制的主对象。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incident {
    pub id: Id,
    pub org_id: Id,
    pub rule_id: Id,
    /// 关联的升级策略 ID。创建时根据 `AlertRule.escalation_policy_id` 拷贝，
    /// dispatcher 直接读它，避免再 `list_policies` 拍脑袋取第一条。
    pub escalation_policy_id: Id,
    pub status: IncidentStatus,
    pub severity: Severity,
    pub summary: String,
    /// 唯一去重键：`blake3(rule_id || sorted_labels)`，对同一规则+同一 label 集
    /// 的连续触发只产生一个 open incident。**创建后不可变。**
    pub fingerprint: String,
    /// 当前位于升级策略的第几层（0 起）
    pub current_step: u32,
    /// 升级策略已循环遍数（repeat/max_loops），从 0 起。
    #[serde(default)]
    pub current_loop: u32,
    /// 当前层的派发起始时间，用于 ack 超时判断
    pub current_step_started_at: TimestampMicros,
    pub assignees: Vec<Id>,
    /// 触发时从规则 + 查询结果 freeze 的 label 快照。rule 之后被修改不会改写本字段。
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// 触发时从规则 freeze 的 annotation 快照（runbook 链接、备注等）。
    #[serde(default)]
    pub annotations: BTreeMap<String, String>,
    /// 触发查询采样到的 trace_id 列表（最多 10 条），用于前端跨信号跳转。
    #[serde(default)]
    pub trace_ids: Vec<String>,
    /// 触发查询采样到的主机标识列表，去重保留顺序。
    #[serde(default)]
    pub host_ids: Vec<String>,
    /// 触发查询采样到的受影响服务列表，去重保留顺序。
    #[serde(default)]
    pub affected_services: Vec<String>,
    /// 触发查询元数据（语言、原始语句、触发时刻的样本行）。仅在 detail 视图返回完整 sample_values。
    #[serde(default)]
    pub triggering_query: Option<TriggeringQuery>,
    pub created_at: TimestampMicros,
    pub acknowledged_at: Option<TimestampMicros>,
    pub acknowledged_by: Option<Id>,
    pub resolved_at: Option<TimestampMicros>,
    pub resolved_by: Option<Id>,
}

/// 触发 Incident 的查询元数据，与 sample 行一起组成 drawer 里的"原始触发上下文"。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggeringQuery {
    pub language: QueryLanguage,
    pub statement: String,
    #[serde(default)]
    pub sample_values: Vec<TriggeringSample>,
}

/// 触发查询返回的一行样本：时间戳 + 数值 + 该行携带的 label。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggeringSample {
    pub ts: TimestampMicros,
    pub value: f64,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

/// AI 生成的 incident 根因分析（RCA）。后台 sweeper 调 LLM 产出后写回，每个
/// incident 至多一条；`GET /alerts/incidents/{id}/rca` 读取。provider/model/token/
/// prompt_hash 一并落库，便于审计与成本追溯。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentRca {
    pub incident_id: Id,
    pub org_id: Id,
    /// LLM 产出的根因摘要正文。
    pub summary: String,
    /// 产出所用 provider 标识（openai / anthropic / openai_compatible）。
    pub provider: Option<String>,
    /// 产出所用模型名。
    pub model: Option<String>,
    /// 解析出的 prompt builtin_key 与渲染后哈希（可追溯到具体模板版本）。
    pub prompt_builtin_key: Option<String>,
    pub prompt_hash: Option<String>,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub finish_reason: Option<String>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentStatus {
    Open,
    Acknowledged,
    Resolved,
    Closed,
}

impl IncidentStatus {
    /// 稳定字符串（与 serde snake_case 一致），用于模板渲染 / API。
    pub fn as_str(&self) -> &'static str {
        match self {
            IncidentStatus::Open => "open",
            IncidentStatus::Acknowledged => "acknowledged",
            IncidentStatus::Resolved => "resolved",
            IncidentStatus::Closed => "closed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
    Critical,
}

impl Severity {
    /// 解析 label 文本为 [`Severity`]（大小写不敏感；`warn`/`crit` 作别名）。
    /// 无法识别返回 `None`，由 caller 决定回退。
    pub fn from_label(s: &str) -> Option<Severity> {
        match s.trim().to_ascii_lowercase().as_str() {
            "info" => Some(Severity::Info),
            "warning" | "warn" => Some(Severity::Warning),
            "error" => Some(Severity::Error),
            "critical" | "crit" => Some(Severity::Critical),
            _ => None,
        }
    }

    /// 稳定字符串（与 serde snake_case 一致），用于模板渲染 / 持久化 / API。
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
            Severity::Critical => "critical",
        }
    }
}

impl Incident {
    /// 当前层是否已超时未 ack。
    pub fn step_timed_out(&self, ack_timeout_secs: u32, now: TimestampMicros) -> bool {
        if self.status != IncidentStatus::Open {
            return false;
        }
        let elapsed_us = now.0.saturating_sub(self.current_step_started_at.0);
        elapsed_us >= (ack_timeout_secs as i64) * 1_000_000
    }
}

/// 决定 incident 严重度：
/// 1. 冻结 labels 里显式的 `severity`（规则上用户配置的意图）优先；
/// 2. 否则按 anomaly 偏离相对量级分级（`ratio≥10`→Critical、`≥3`→Error）；
/// 3. 兜底 [`Severity::Warning`]（保持与历史行为一致）。
///
/// `deviation_ratio` 仅 anomaly 规则提供（见 [`super::anomaly::AnomalyDecision::deviation_ratio`]）；
/// scheduled / realtime 传 `None`。
pub fn determine_severity(
    labels: &BTreeMap<String, String>,
    deviation_ratio: Option<f64>,
) -> Severity {
    if let Some(sev) = labels.get("severity").and_then(|v| Severity::from_label(v)) {
        return sev;
    }
    if let Some(ratio) = deviation_ratio {
        if ratio >= 10.0 {
            return Severity::Critical;
        }
        if ratio >= 3.0 {
            return Severity::Error;
        }
    }
    Severity::Warning
}

/// 决定 incident 最终严重度，优先级：
/// 多档命中的最高档 `fired` > 规则显式 `explicit` > `labels["severity"]` > anomaly ratio > Warning。
/// 后三者由 [`determine_severity`] 兜底。
pub fn resolve_incident_severity(
    fired: Option<Severity>,
    explicit: Option<Severity>,
    labels: &BTreeMap<String, String>,
    deviation_ratio: Option<f64>,
) -> Severity {
    fired
        .or(explicit)
        .unwrap_or_else(|| determine_severity(labels, deviation_ratio))
}

/// 生成简洁 incident 标题：规则名 + 受影响服务
/// （最多 2 个，多余以 `+N` 折叠）。无受影响服务时退回裸规则名——绝不比现状更差。
pub fn generate_title(rule_name: &str, affected_services: &[String]) -> String {
    if affected_services.is_empty() {
        return rule_name.to_string();
    }
    let shown = affected_services
        .iter()
        .take(2)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if affected_services.len() > 2 {
        format!("{rule_name} · {shown} +{}", affected_services.len() - 2)
    } else {
        format!("{rule_name} · {shown}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_label_takes_precedence_over_ratio() {
        let labels = [("severity".to_string(), "info".to_string())].into();
        // 即便偏离极大，显式 label 优先。
        assert_eq!(determine_severity(&labels, Some(999.0)), Severity::Info);
    }

    #[test]
    fn severity_falls_back_to_ratio_tiers() {
        let empty = BTreeMap::new();
        assert_eq!(determine_severity(&empty, Some(12.0)), Severity::Critical);
        assert_eq!(determine_severity(&empty, Some(4.0)), Severity::Error);
        assert_eq!(determine_severity(&empty, Some(1.0)), Severity::Warning);
        // 非 anomaly（无 ratio）兜底 Warning。
        assert_eq!(determine_severity(&empty, None), Severity::Warning);
    }

    #[test]
    fn severity_label_aliases_and_case_insensitive() {
        let labels = [("severity".to_string(), "CRIT".to_string())].into();
        assert_eq!(determine_severity(&labels, None), Severity::Critical);
        // 无法识别的 label 回退到 ratio / 兜底。
        let bogus = [("severity".to_string(), "spicy".to_string())].into();
        assert_eq!(determine_severity(&bogus, None), Severity::Warning);
    }

    #[test]
    fn title_appends_affected_services() {
        assert_eq!(generate_title("High errors", &[]), "High errors");
        assert_eq!(
            generate_title("High errors", &["api".into()]),
            "High errors · api"
        );
        assert_eq!(
            generate_title("High errors", &["api".into(), "web".into()]),
            "High errors · api, web"
        );
        assert_eq!(
            generate_title(
                "High errors",
                &["api".into(), "web".into(), "db".into(), "cache".into()]
            ),
            "High errors · api, web +2"
        );
    }
}
