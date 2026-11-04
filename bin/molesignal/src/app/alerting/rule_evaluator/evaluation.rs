// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use crate::domain::{
    alerting::rule::{AlertRule, ComparisonOp},
    query::QueryResult,
};

/// 结果集首行首列解析成 f64（数值列）；缺值 / 非数值返回 None。
pub(super) fn first_cell_f64(result: &QueryResult) -> Option<f64> {
    let value = result.rows.first()?.first()?;

    value
        .as_f64()
        .or_else(|| value.as_i64().map(|value| value as f64))
        .or_else(|| value.as_u64().map(|value| value as f64))
}

pub(super) fn compare_value(value: f64, operator: &ComparisonOp, threshold: f64) -> bool {
    match operator {
        ComparisonOp::Gt => value > threshold,
        ComparisonOp::Gte => value >= threshold,
        ComparisonOp::Lt => value < threshold,
        ComparisonOp::Lte => value <= threshold,
        ComparisonOp::Eq => (value - threshold).abs() < f64::EPSILON,
        ComparisonOp::Neq => (value - threshold).abs() >= f64::EPSILON,
    }
}

pub(super) fn compute_fingerprint(rule: &AlertRule) -> String {
    group_fingerprint(rule, &BTreeMap::new())
}

/// 按列拆分的规则里一组的指纹：规则的指纹再加上这一组的取值；组为空时就是规则的指纹。
pub(super) fn group_fingerprint(rule: &AlertRule, group: &BTreeMap<String, String>) -> String {
    let mut buffer = format!("{}|", rule.id.0);

    for (key, value) in &rule.labels {
        buffer.push_str(key);
        buffer.push('=');
        buffer.push_str(value);
        buffer.push(';');
    }

    if !group.is_empty() {
        buffer.push('|');
        buffer.push_str(&group_text(group));
    }

    blake3::hash(buffer.as_bytes()).to_hex().to_string()
}

/// 一组的去抖状态存在哪一行：整条规则为空，按列拆分的一组是它取值的摘要。
pub(super) fn group_state_key(group: &BTreeMap<String, String>) -> String {
    if group.is_empty() {
        return String::new();
    }

    blake3::hash(group_text(group).as_bytes())
        .to_hex()
        .to_string()
}

/// 取值里可能有 `=`、`;`，按 JSON 写出来，不同的组不会写成同一段文字。
fn group_text(group: &BTreeMap<String, String>) -> String {
    serde_json::to_string(group).unwrap_or_default()
}
