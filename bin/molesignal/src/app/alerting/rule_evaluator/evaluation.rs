// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

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

pub(super) fn first_cell_matches(
    result: &QueryResult,
    operator: &ComparisonOp,
    threshold: f64,
) -> bool {
    match first_cell_f64(result) {
        Some(value) if !value.is_nan() => compare_value(value, operator, threshold),
        _ => false,
    }
}

pub(super) fn compute_fingerprint(rule: &AlertRule) -> String {
    let mut buffer = format!("{}|", rule.id.0);
    for (key, value) in &rule.labels {
        buffer.push_str(key);
        buffer.push('=');
        buffer.push_str(value);
        buffer.push(';');
    }
    blake3::hash(buffer.as_bytes()).to_hex().to_string()
}
