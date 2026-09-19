// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use super::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum BinaryOutput {
    Value(f64),
    Comparison(bool),
}

/// 匹配签名：`on(l...)` 只保留这些 label；`ignoring(l...)` 去掉这些 label；
/// 无 modifier → 整套 label（默认 1:1）。
pub(super) fn match_signature(labels: &LabelSet, matching: Option<&LabelModifier>) -> LabelSet {
    match matching {
        None => labels.clone(),
        Some(LabelModifier::Include(on)) => {
            let mut out = LabelSet::new();
            for key in on.labels.iter() {
                if let Some(value) = labels.get(key) {
                    out.insert(key.clone(), value.clone());
                }
            }
            out
        }
        Some(LabelModifier::Exclude(ignoring)) => {
            let mut out = labels.clone();
            for key in ignoring.labels.iter() {
                out.remove(key);
            }
            out
        }
    }
}

/// vector ↔ vector：按 `binary` 的集合运算 / 匹配基数派生结果项。
pub(super) fn match_vectors(
    binary: &BinaryExpr,
    lhs: Vec<(LabelSet, f64)>,
    rhs: Vec<(LabelSet, f64)>,
) -> Result<Vec<(LabelSet, f64)>> {
    let matching = binary.modifier.as_ref().and_then(|m| m.matching.as_ref());
    if binary.op.is_set_operator() {
        return Ok(apply_set_operator(binary.op, matching, lhs, rhs));
    }
    let return_bool = binary_return_bool(binary);
    let card = binary
        .modifier
        .as_ref()
        .map(|m| &m.card)
        .unwrap_or(&VectorMatchCardinality::OneToOne);
    match card {
        VectorMatchCardinality::OneToOne => {
            one_to_one(binary.op, matching, return_bool, &lhs, &rhs)
        }
        VectorMatchCardinality::ManyToOne(extra) => group_match(
            binary.op,
            matching,
            return_bool,
            &lhs,
            &rhs,
            &extra.labels,
            true,
        ),
        VectorMatchCardinality::OneToMany(extra) => group_match(
            binary.op,
            matching,
            return_bool,
            &lhs,
            &rhs,
            &extra.labels,
            false,
        ),
        VectorMatchCardinality::ManyToMany => Err(Error::invalid(
            "promql many-to-many matching is only valid for set operators (and/or/unless)",
        )),
    }
}

fn apply_set_operator(
    op: TokenType,
    matching: Option<&LabelModifier>,
    lhs: Vec<(LabelSet, f64)>,
    rhs: Vec<(LabelSet, f64)>,
) -> Vec<(LabelSet, f64)> {
    let rhs_sigs: HashSet<LabelSet> = rhs
        .iter()
        .map(|(labels, _)| match_signature(labels, matching))
        .collect();
    match op.id() {
        // and：保留 lhs 中签名出现在 rhs 的项。
        T_LAND => lhs
            .into_iter()
            .filter(|(labels, _)| rhs_sigs.contains(&match_signature(labels, matching)))
            .collect(),
        // unless：保留 lhs 中签名不在 rhs 的项。
        T_LUNLESS => lhs
            .into_iter()
            .filter(|(labels, _)| !rhs_sigs.contains(&match_signature(labels, matching)))
            .collect(),
        // or：lhs 全保留 + rhs 中签名未在 lhs 出现的项。
        T_LOR => {
            let lhs_sigs: HashSet<LabelSet> = lhs
                .iter()
                .map(|(labels, _)| match_signature(labels, matching))
                .collect();
            let mut out = lhs;
            for (labels, value) in rhs {
                if !lhs_sigs.contains(&match_signature(&labels, matching)) {
                    out.push((labels, value));
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

fn one_to_one(
    op: TokenType,
    matching: Option<&LabelModifier>,
    return_bool: bool,
    lhs: &[(LabelSet, f64)],
    rhs: &[(LabelSet, f64)],
) -> Result<Vec<(LabelSet, f64)>> {
    let mut rhs_by: HashMap<LabelSet, f64> = HashMap::new();
    for (labels, value) in rhs {
        rhs_by
            .entry(match_signature(labels, matching))
            .or_insert(*value);
    }
    let mut out = Vec::new();
    for (labels, lhs_value) in lhs {
        let sig = match_signature(labels, matching);
        let Some(rhs_value) = rhs_by.get(&sig).copied() else {
            continue;
        };
        let output = apply_binary_output(op, *lhs_value, rhs_value)?;
        if let Some(value) = sample_binary_value(output, return_bool, *lhs_value) {
            // 1:1 结果 label 集 = 匹配签名。
            out.push((sig, value));
        }
    }
    Ok(out)
}

/// `group_left` (group_left=true) / `group_right` 的多对一匹配：
/// "one" 侧按签名建表，"many" 侧逐项匹配，结果保留 many 侧 label 并从 one 侧拷 `extra`。
fn group_match(
    op: TokenType,
    matching: Option<&LabelModifier>,
    return_bool: bool,
    lhs: &[(LabelSet, f64)],
    rhs: &[(LabelSet, f64)],
    extra: &[String],
    group_left: bool,
) -> Result<Vec<(LabelSet, f64)>> {
    let (many, one) = if group_left { (lhs, rhs) } else { (rhs, lhs) };
    let mut one_by: HashMap<LabelSet, (LabelSet, f64)> = HashMap::new();
    for (labels, value) in one {
        one_by
            .entry(match_signature(labels, matching))
            .or_insert_with(|| (labels.clone(), *value));
    }
    let mut out = Vec::new();
    for (many_labels, many_value) in many {
        let sig = match_signature(many_labels, matching);
        let Some((one_labels, one_value)) = one_by.get(&sig) else {
            continue;
        };
        // 运算符始终 lhs OP rhs，与分组方向无关。
        let (lhs_value, rhs_value) = if group_left {
            (*many_value, *one_value)
        } else {
            (*one_value, *many_value)
        };
        let output = apply_binary_output(op, lhs_value, rhs_value)?;
        let Some(value) = sample_binary_value(output, return_bool, lhs_value) else {
            continue;
        };
        let mut labels = many_labels.clone();
        for key in extra {
            match one_labels.get(key) {
                Some(v) => {
                    labels.insert(key.clone(), v.clone());
                }
                None => {
                    labels.remove(key);
                }
            }
        }
        out.push((labels, value));
    }
    Ok(out)
}

pub(super) fn apply_binary_output(op: TokenType, lhs: f64, rhs: f64) -> Result<BinaryOutput> {
    let output = match op.id() {
        T_ADD => BinaryOutput::Value(lhs + rhs),
        T_SUB => BinaryOutput::Value(lhs - rhs),
        T_MUL => BinaryOutput::Value(lhs * rhs),
        T_DIV => BinaryOutput::Value(lhs / rhs),
        T_MOD => BinaryOutput::Value(lhs % rhs),
        T_POW => BinaryOutput::Value(lhs.powf(rhs)),
        T_EQLC => BinaryOutput::Comparison(lhs == rhs),
        T_NEQ => BinaryOutput::Comparison(lhs != rhs),
        T_GTR => BinaryOutput::Comparison(lhs > rhs),
        T_GTE => BinaryOutput::Comparison(lhs >= rhs),
        T_LSS => BinaryOutput::Comparison(lhs < rhs),
        T_LTE => BinaryOutput::Comparison(lhs <= rhs),
        _ => {
            return Err(Error::invalid(format!(
                "promql binary operator not yet supported: {op}"
            )));
        }
    };
    Ok(output)
}

pub(super) fn binary_return_bool(binary: &BinaryExpr) -> bool {
    binary.modifier.as_ref().is_some_and(|m| m.return_bool)
}

pub(super) fn sample_binary_value(
    output: BinaryOutput,
    return_bool: bool,
    comparison_true_value: f64,
) -> Option<f64> {
    match output {
        BinaryOutput::Value(value) => Some(value),
        BinaryOutput::Comparison(true) if return_bool => Some(1.0),
        BinaryOutput::Comparison(false) if return_bool => Some(0.0),
        BinaryOutput::Comparison(true) => Some(comparison_true_value),
        BinaryOutput::Comparison(false) => None,
    }
}

pub(super) fn scalar_binary_value(output: BinaryOutput) -> f64 {
    match output {
        BinaryOutput::Value(value) => value,
        BinaryOutput::Comparison(true) => 1.0,
        BinaryOutput::Comparison(false) => 0.0,
    }
}

pub(super) fn apply_binary_instant(
    binary: &BinaryExpr,
    lhs: InstantVector,
    rhs: InstantVector,
    lhs_is_scalar: bool,
    rhs_is_scalar: bool,
) -> Result<InstantVector> {
    let return_bool = binary_return_bool(binary);
    let items = match (lhs_is_scalar, rhs_is_scalar) {
        (true, true) => {
            let lhs = scalar_value(lhs, "binary lhs")?;
            let rhs = scalar_value(rhs, "binary rhs")?;
            vec![(
                LabelSet::new(),
                scalar_binary_value(apply_binary_output(binary.op, lhs, rhs)?),
            )]
        }
        (true, false) => {
            let lhs = scalar_value(lhs, "binary lhs")?;
            let mut items = Vec::with_capacity(rhs.items.len());
            for (labels, rhs) in rhs.items {
                let output = apply_binary_output(binary.op, lhs, rhs)?;
                if let Some(value) = sample_binary_value(output, return_bool, rhs) {
                    items.push((labels, value));
                }
            }
            items
        }
        (false, true) => {
            let rhs = scalar_value(rhs, "binary rhs")?;
            let mut items = Vec::with_capacity(lhs.items.len());
            for (labels, lhs) in lhs.items {
                let output = apply_binary_output(binary.op, lhs, rhs)?;
                if let Some(value) = sample_binary_value(output, return_bool, lhs) {
                    items.push((labels, value));
                }
            }
            items
        }
        (false, false) => match_vectors(binary, lhs.items, rhs.items)?,
    };
    Ok(InstantVector { items })
}

pub(super) fn apply_binary_range_scalar(
    binary: &BinaryExpr,
    scalar: f64,
    range: RangeVector,
    scalar_on_lhs: bool,
) -> Result<RangeVector> {
    let return_bool = binary_return_bool(binary);
    let mut points = Vec::with_capacity(range.points.len());
    for point in range.points {
        let (lhs, rhs) = if scalar_on_lhs {
            (scalar, point.value)
        } else {
            (point.value, scalar)
        };
        let output = apply_binary_output(binary.op, lhs, rhs)?;
        if let Some(value) = sample_binary_value(output, return_bool, point.value) {
            points.push(RangePoint { value, ..point });
        }
    }
    Ok(RangeVector { points })
}

pub(super) fn apply_binary_range(
    binary: &BinaryExpr,
    lhs: RangeVector,
    rhs: RangeVector,
) -> Result<RangeVector> {
    // 逐时间戳套用与 instant 相同的匹配/集合语义：把两侧点按 ts 分桶，每个 ts
    // 跑一次 match_vectors，再把结果重新挂回该 ts。
    let mut lhs_by_ts: BTreeMap<i64, Vec<(LabelSet, f64)>> = BTreeMap::new();
    for point in lhs.points {
        lhs_by_ts
            .entry(point.ts_us)
            .or_default()
            .push((point.labels, point.value));
    }
    let mut rhs_by_ts: HashMap<i64, Vec<(LabelSet, f64)>> = HashMap::new();
    for point in rhs.points {
        rhs_by_ts
            .entry(point.ts_us)
            .or_default()
            .push((point.labels, point.value));
    }
    // `or` 可能引入只存在于 rhs 的时间戳，故取并集。
    let mut timestamps: Vec<i64> = lhs_by_ts.keys().copied().collect();
    for ts in rhs_by_ts.keys() {
        if !lhs_by_ts.contains_key(ts) {
            timestamps.push(*ts);
        }
    }
    timestamps.sort_unstable();

    let mut points = Vec::new();
    for ts in timestamps {
        let lhs_items = lhs_by_ts.remove(&ts).unwrap_or_default();
        let rhs_items = rhs_by_ts.remove(&ts).unwrap_or_default();
        for (labels, value) in match_vectors(binary, lhs_items, rhs_items)? {
            points.push(RangePoint {
                ts_us: ts,
                labels,
                value,
            });
        }
    }
    Ok(RangeVector { points })
}
