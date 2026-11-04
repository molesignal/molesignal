// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 导出包：`{"format": "molesignal.alert-rules/v1", "rules": [...]}`。一条规则带着它全部的定义，
//! 但不带 id、组织、评估状态和时间；升级策略和组合规则的子规则按名称引用，所以同一份文件能搬到
//! 别的组织或别的集群。心跳规则的打卡地址不在规则里，不会被导出。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::domain::alerting::{
    anomaly::AnomalyParams,
    escalation::{DEFAULT_POLICY_ID, EscalationPolicy},
    incident::Severity,
    rule::{AlertQuery, AlertRule, AlertRuleKind, AlertTrigger, SeverityThreshold},
};

/// 导出包的格式名。
pub const BUNDLE_FORMAT: &str = "molesignal.alert-rules/v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleBundle {
    pub format: String,
    /// RFC 3339。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exported_at: Option<String>,
    pub rules: Vec<PortableRule>,
}

/// 一条规则的定义，引用都按名称。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableRule {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default)]
    pub kind: AlertRuleKind,
    /// 组合规则的 `composite.rules` 里是子规则的名称。
    pub query: AlertQuery,
    pub trigger: AlertTrigger,
    #[serde(default)]
    pub thresholds: Vec<SeverityThreshold>,
    #[serde(default)]
    pub severity: Option<Severity>,
    #[serde(default)]
    pub anomaly_params: Option<AnomalyParams>,
    /// 升级策略的名称；不写是默认策略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalation_policy: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub annotations: BTreeMap<String, String>,
}

fn enabled_by_default() -> bool {
    true
}

/// 导出一条规则：升级策略和子规则换成名称。
pub fn portable(
    rule: &AlertRule,
    policies: &[EscalationPolicy],
    rule_names: &BTreeMap<String, String>,
) -> PortableRule {
    let mut query = rule.query.clone();

    if let Some(composite) = &mut query.composite {
        for id in composite.rules.values_mut() {
            if let Some(name) = rule_names.get(id.as_str()) {
                *id = crate::shared::ids::Id(name.clone());
            }
        }
    }

    let escalation_policy = (rule.escalation_policy_id.as_str() != DEFAULT_POLICY_ID)
        .then(|| {
            policies
                .iter()
                .find(|policy| policy.id == rule.escalation_policy_id)
                .map(|policy| policy.name.clone())
        })
        .flatten();

    PortableRule {
        name: rule.name.clone(),
        description: rule.description.clone(),
        enabled: rule.enabled,
        kind: rule.kind,
        query,
        trigger: rule.trigger.clone(),
        thresholds: rule.thresholds.clone(),
        severity: rule.severity,
        anomaly_params: rule.anomaly_params.clone(),
        escalation_policy,
        labels: rule.labels.clone(),
        annotations: rule.annotations.clone(),
    }
}

/// 两份定义不一样的地方，按字段名列出。
pub fn changed_fields(before: &PortableRule, after: &PortableRule) -> Vec<&'static str> {
    let json = |value: &dyn erased::Json| value.json();
    let mut fields = Vec::new();

    let mut check = |name: &'static str, same: bool| {
        if !same {
            fields.push(name);
        }
    };

    check("description", before.description == after.description);
    check("enabled", before.enabled == after.enabled);
    check("kind", before.kind == after.kind);
    check("query", json(&before.query) == json(&after.query));
    check("trigger", json(&before.trigger) == json(&after.trigger));
    check(
        "thresholds",
        json(&before.thresholds) == json(&after.thresholds),
    );
    check("severity", before.severity == after.severity);
    check(
        "anomalyParams",
        json(&before.anomaly_params) == json(&after.anomaly_params),
    );
    check(
        "escalationPolicy",
        before.escalation_policy == after.escalation_policy,
    );
    check("labels", before.labels == after.labels);
    check("annotations", before.annotations == after.annotations);

    fields
}

/// 比较时按 JSON 比：有的字段（浮点阈值）没有实现相等比较。
mod erased {
    pub trait Json {
        fn json(&self) -> serde_json::Value;
    }

    impl<T: serde::Serialize> Json for T {
        fn json(&self) -> serde_json::Value {
            serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
        }
    }
}
