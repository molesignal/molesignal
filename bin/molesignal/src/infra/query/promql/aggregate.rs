// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;

pub(super) fn apply_topk_bottomk(
    op: &str,
    k: f64,
    input: InstantVector,
    modifier: Option<&LabelModifier>,
) -> Result<InstantVector> {
    let k = topk_limit(k)?;
    if k == 0 {
        return Ok(InstantVector::default());
    }

    let mut grouped: BTreeMap<LabelSet, Vec<(LabelSet, f64)>> = BTreeMap::new();
    for (labels, value) in input.items {
        grouped
            .entry(project_labels(&labels, modifier))
            .or_default()
            .push((labels, value));
    }

    let mut items = Vec::new();
    for (_, mut samples) in grouped {
        sort_topk_samples(op, &mut samples)?;
        items.extend(samples.into_iter().take(k));
    }
    Ok(InstantVector { items })
}

pub(super) fn apply_topk_bottomk_range(
    op: &str,
    k: f64,
    input: RangeVector,
    modifier: Option<&LabelModifier>,
) -> Result<RangeVector> {
    let k = topk_limit(k)?;
    if k == 0 {
        return Ok(RangeVector::default());
    }

    let mut grouped: BTreeMap<(i64, LabelSet), Vec<RangePoint>> = BTreeMap::new();
    for point in input.points {
        let key = project_labels(&point.labels, modifier);
        grouped.entry((point.ts_us, key)).or_default().push(point);
    }

    let mut points = Vec::new();
    for (_, mut samples) in grouped {
        sort_topk_points(op, &mut samples)?;
        points.extend(samples.into_iter().take(k));
    }
    Ok(RangeVector { points })
}

/// `limitk(k, v)`：每组取确定性的 k 条（按 label 集排序后取前 k）。
pub(super) fn apply_limitk(
    k: f64,
    input: InstantVector,
    modifier: Option<&LabelModifier>,
) -> Result<InstantVector> {
    let k = topk_limit(k)?;
    if k == 0 {
        return Ok(InstantVector::default());
    }
    let mut grouped: BTreeMap<LabelSet, Vec<(LabelSet, f64)>> = BTreeMap::new();
    for (labels, value) in input.items {
        grouped
            .entry(project_labels(&labels, modifier))
            .or_default()
            .push((labels, value));
    }
    let mut items = Vec::new();
    for (_, mut samples) in grouped {
        samples.sort_by(|a, b| a.0.cmp(&b.0));
        items.extend(samples.into_iter().take(k));
    }
    Ok(InstantVector { items })
}

pub(super) fn apply_limitk_range(
    k: f64,
    input: RangeVector,
    modifier: Option<&LabelModifier>,
) -> Result<RangeVector> {
    let k = topk_limit(k)?;
    if k == 0 {
        return Ok(RangeVector::default());
    }
    let mut grouped: BTreeMap<(i64, LabelSet), Vec<RangePoint>> = BTreeMap::new();
    for point in input.points {
        let key = project_labels(&point.labels, modifier);
        grouped.entry((point.ts_us, key)).or_default().push(point);
    }
    let mut points = Vec::new();
    for (_, mut samples) in grouped {
        samples.sort_by(|a, b| a.labels.cmp(&b.labels));
        points.extend(samples.into_iter().take(k));
    }
    Ok(RangeVector { points })
}

/// `limit_ratio(r, v)`：按 series label 的稳定哈希落点取 `r` 比例的子集；
/// `r<0` 取互补子集（与 `1+r` 对称），故 `r` 与 `-(1-r)` 划分出不相交的两半。
pub(super) fn apply_limit_ratio(ratio: f64, input: InstantVector) -> Result<InstantVector> {
    let ratio = validate_ratio(ratio)?;
    let items = input
        .items
        .into_iter()
        .filter(|(labels, _)| in_ratio(labelset_ratio(labels), ratio))
        .collect();
    Ok(InstantVector { items })
}

pub(super) fn apply_limit_ratio_range(ratio: f64, input: RangeVector) -> Result<RangeVector> {
    let ratio = validate_ratio(ratio)?;
    let points = input
        .points
        .into_iter()
        .filter(|point| in_ratio(labelset_ratio(&point.labels), ratio))
        .collect();
    Ok(RangeVector { points })
}

fn validate_ratio(ratio: f64) -> Result<f64> {
    if !(-1.0..=1.0).contains(&ratio) {
        return Err(Error::invalid("limit_ratio parameter must be in [-1, 1]"));
    }
    Ok(ratio)
}

/// series label 集 → `[0,1)` 的稳定落点（blake3，跨进程一致）。
fn labelset_ratio(labels: &LabelSet) -> f64 {
    let mut buf = String::new();
    for (key, value) in labels {
        buf.push_str(key);
        buf.push('=');
        buf.push_str(value);
        buf.push('\n');
    }
    let hash = blake3::hash(buf.as_bytes());
    let n = u64::from_le_bytes(hash.as_bytes()[..8].try_into().unwrap());
    n as f64 / (u64::MAX as f64 + 1.0)
}

fn in_ratio(point: f64, ratio: f64) -> bool {
    if ratio >= 0.0 {
        point < ratio
    } else {
        point >= 1.0 + ratio
    }
}

#[derive(Debug, Clone)]
pub(super) enum AggregateParam {
    Scalar(f64),
    LabelName(String),
}

pub(super) fn apply_regular_aggregate(
    op: &str,
    labels: LabelSet,
    values: Vec<f64>,
    param: Option<&AggregateParam>,
) -> Result<Vec<(LabelSet, f64)>> {
    let value = match op {
        "sum" => values.iter().copied().sum::<f64>(),
        "avg" => values.iter().copied().sum::<f64>() / values.len() as f64,
        "min" => values.iter().copied().fold(f64::INFINITY, f64::min),
        "max" => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        "count" => values.len() as f64,
        "stddev" => stdvar_value(&values).sqrt(),
        "stdvar" => stdvar_value(&values),
        "quantile" => {
            let Some(AggregateParam::Scalar(q)) = param else {
                return Err(Error::invalid(
                    "quantile aggregate requires scalar parameter",
                ));
            };
            quantile_value(values, *q)
        }
        "group" => 1.0,
        "count_values" => {
            let Some(AggregateParam::LabelName(label_name)) = param else {
                return Err(Error::invalid(
                    "count_values aggregate requires label name parameter",
                ));
            };
            return Ok(count_values_aggregate(labels, values, label_name));
        }
        other => {
            return Err(Error::invalid(format!(
                "promql aggregate not yet supported: {other}"
            )));
        }
    };
    Ok(vec![(labels, value)])
}

pub(super) fn count_values_aggregate(
    labels: LabelSet,
    values: Vec<f64>,
    label_name: &str,
) -> Vec<(LabelSet, f64)> {
    let mut counts: BTreeMap<String, f64> = BTreeMap::new();
    for value in values {
        *counts.entry(format_sample_value_label(value)).or_default() += 1.0;
    }
    counts
        .into_iter()
        .map(|(sample_value, count)| {
            let mut labels = labels.clone();
            labels.insert(label_name.to_string(), sample_value);
            (labels, count)
        })
        .collect()
}

pub(super) fn format_sample_value_label(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value == f64::INFINITY {
        "+Inf".to_string()
    } else if value == f64::NEG_INFINITY {
        "-Inf".to_string()
    } else {
        value.to_string()
    }
}

pub(super) fn topk_limit(k: f64) -> Result<usize> {
    if !k.is_finite() {
        return Err(Error::invalid("topk/bottomk parameter must be finite"));
    }
    Ok(k.max(0.0).floor() as usize)
}

pub(super) fn sort_topk_samples(op: &str, samples: &mut [(LabelSet, f64)]) -> Result<()> {
    match op {
        "topk" => samples.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        }),
        "bottomk" => samples.sort_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        }),
        other => {
            return Err(Error::invalid(format!(
                "promql aggregate not yet supported: {other}"
            )));
        }
    }
    Ok(())
}

pub(super) fn sort_topk_points(op: &str, samples: &mut [RangePoint]) -> Result<()> {
    match op {
        "topk" => samples.sort_by(|a, b| {
            b.value
                .partial_cmp(&a.value)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.labels.cmp(&b.labels))
        }),
        "bottomk" => samples.sort_by(|a, b| {
            a.value
                .partial_cmp(&b.value)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.labels.cmp(&b.labels))
        }),
        other => {
            return Err(Error::invalid(format!(
                "promql aggregate not yet supported: {other}"
            )));
        }
    }
    Ok(())
}
