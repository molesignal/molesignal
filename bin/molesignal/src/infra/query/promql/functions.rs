// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;

pub(super) fn sort_instant_vector(mut input: InstantVector, descending: bool) -> InstantVector {
    input.items.sort_by(|a, b| {
        let value_order = if descending {
            b.1.partial_cmp(&a.1)
        } else {
            a.1.partial_cmp(&b.1)
        }
        .unwrap_or(std::cmp::Ordering::Equal);
        value_order.then_with(|| a.0.cmp(&b.0))
    });
    input
}

/// `sort_by_label(v, l, ...)`：按给定 label 值字典序稳定排序（`desc` 取反）。
pub(super) fn sort_instant_vector_by_label(
    mut input: InstantVector,
    labels: &[String],
    descending: bool,
) -> InstantVector {
    input.items.sort_by(|a, b| {
        let mut order = std::cmp::Ordering::Equal;
        for label in labels {
            let av = a.0.get(label).map(String::as_str).unwrap_or("");
            let bv = b.0.get(label).map(String::as_str).unwrap_or("");
            order = av.cmp(bv);
            if order != std::cmp::Ordering::Equal {
                break;
            }
        }
        if descending { order.reverse() } else { order }
    });
    input
}

/// `absent(v)`：`v` 为空 → 返回单条 value=1（labels 取选择器的等值匹配）；否则空。
pub(super) fn absent_vector(expr: &Expr, inner: InstantVector) -> InstantVector {
    if !inner.items.is_empty() {
        return InstantVector::default();
    }
    let labels = match expr {
        Expr::VectorSelector(vs) => absent_labels(&vs.matchers),
        _ => LabelSet::new(),
    };
    InstantVector {
        items: vec![(labels, 1.0)],
    }
}

/// `absent_over_time(v[range])`：窗口内无样本 → value=1；否则空。
pub(super) fn absent_over_time_vector(ms: &MatrixSelector, present: bool) -> InstantVector {
    if present {
        return InstantVector::default();
    }
    InstantVector {
        items: vec![(absent_labels(&ms.vs.matchers), 1.0)],
    }
}

/// 从匹配器里抽出等值（`=`）标签，供 absent 系列填充结果 label 集。
pub(super) fn absent_labels(matchers: &Matchers) -> LabelSet {
    let mut out = LabelSet::new();
    for m in &matchers.matchers {
        if m.name != "__name__" && matches!(m.op, MatchOp::Equal) {
            out.insert(m.name.clone(), m.value.clone());
        }
    }
    out
}

pub(super) fn time_instant_vector(at_us: i64) -> InstantVector {
    vector_from_scalar(at_us as f64 / 1_000_000.0)
}

pub(super) fn vector_from_scalar(value: f64) -> InstantVector {
    InstantVector {
        items: vec![(LabelSet::new(), value)],
    }
}

pub(super) fn scalar_from_vector(input: InstantVector) -> InstantVector {
    let mut items = input.items.into_iter();
    let value = match (items.next(), items.next()) {
        (Some((_, value)), None) => value,
        _ => f64::NAN,
    };
    vector_from_scalar(value)
}

pub fn apply_rate_like(func: &str, series: Vec<Series>, range: Duration) -> InstantVector {
    let range_secs = range.as_secs_f64().max(1.0);
    let items: Vec<(LabelSet, f64)> = series
        .into_iter()
        .filter_map(|s| {
            if s.samples.len() < 2 {
                return None;
            }
            let samples = match func {
                "irate" => &s.samples[s.samples.len() - 2..],
                "rate" | "increase" => s.samples.as_slice(),
                _ => return None,
            };
            let (first_ts, _) = *samples.first()?;
            let (last_ts, _) = *samples.last()?;
            if last_ts <= first_ts {
                return None;
            }
            let delta = counter_delta(samples)?;
            let value = match func {
                "rate" => delta / range_secs,
                "irate" => {
                    let elapsed_secs = (last_ts - first_ts) as f64 / 1_000_000.0;
                    delta / elapsed_secs.max(1e-9)
                }
                "increase" => delta,
                _ => return None,
            };
            Some((s.labels, value))
        })
        .collect();
    InstantVector { items }
}

pub(super) fn apply_rate_like_range(
    func: &str,
    series: Vec<Series>,
    range: Duration,
    start_us: i64,
    end_us: i64,
    step_us: i64,
) -> RangeVector {
    let range_us = (range.as_micros() as i64).max(1);
    let range_secs = range.as_secs_f64().max(1.0);
    let mut points = Vec::new();

    for s in series {
        each_step_window(&s.samples, start_us, end_us, step_us, range_us, |t, win| {
            if let Some(value) = rate_window_value(func, win, range_secs) {
                points.push(RangePoint {
                    ts_us: t,
                    labels: s.labels.clone(),
                    value,
                });
            }
        });
    }

    RangeVector { points }
}

/// rate 家族（`rate`/`irate`/`increase`）的单窗口求值；`range_secs` 是 selector 的 `[range]`
/// 秒数（≥1）。窗口样本不足 2 个 / 时间无前进时返 `None`。增量缓存与 range 路径共用此函数，
/// 保证同一窗口取值一致。
pub(super) fn rate_window_value(func: &str, win: &[(i64, f64)], range_secs: f64) -> Option<f64> {
    if win.len() < 2 {
        return None;
    }
    let samples = match func {
        "irate" => &win[win.len() - 2..],
        "rate" | "increase" => win,
        _ => return None,
    };
    let (first_ts, _) = *samples.first()?;
    let (last_ts, _) = *samples.last()?;
    if last_ts <= first_ts {
        return None;
    }
    let delta = counter_delta(samples)?;
    match func {
        "rate" => Some(delta / range_secs),
        "irate" => {
            let elapsed_secs = (last_ts - first_ts) as f64 / 1_000_000.0;
            Some(delta / elapsed_secs.max(1e-9))
        }
        "increase" => Some(delta),
        _ => None,
    }
}

/// Counter-aware delta used by `rate`, `irate` and `increase`.
///
/// A lower value after a higher value is a counter reset, not a negative
/// increment. Prometheus treats the new value as the amount accumulated since
/// the reset, so `[100, 110, 5, 10]` increases by `10 + 5 + 5 = 20`.
/// Non-finite samples and non-increasing timestamps are ignored defensively;
/// the normal materialization path already sorts and de-duplicates them.
fn counter_delta(samples: &[(i64, f64)]) -> Option<f64> {
    let mut iter = samples
        .iter()
        .copied()
        .filter(|(_, value)| value.is_finite());
    let (mut previous_ts, mut previous_value) = iter.next()?;
    let mut delta = 0.0;
    let mut transitions = 0usize;

    for (timestamp, value) in iter {
        if timestamp <= previous_ts {
            continue;
        }
        delta += if value >= previous_value {
            value - previous_value
        } else {
            // Counter reset: the post-reset value is the increase from zero.
            value.max(0.0)
        };
        previous_ts = timestamp;
        previous_value = value;
        transitions += 1;
    }

    (transitions > 0).then_some(delta)
}

pub(super) fn apply_over_time(
    func: &str,
    quantile: Option<f64>,
    series: Vec<Series>,
) -> InstantVector {
    let items = series
        .into_iter()
        .filter_map(|s| {
            apply_over_time_value(func, quantile, &s.samples).map(|value| (s.labels, value))
        })
        .collect();
    InstantVector { items }
}

pub(super) fn apply_over_time_range(
    func: &str,
    quantile: Option<f64>,
    series: Vec<Series>,
    range: Duration,
    start_us: i64,
    end_us: i64,
    step_us: i64,
) -> RangeVector {
    let range_us = (range.as_micros() as i64).max(1);
    let mut points = Vec::new();
    for s in series {
        each_step_window(&s.samples, start_us, end_us, step_us, range_us, |t, win| {
            let Some(value) = apply_over_time_value(func, quantile, win) else {
                return;
            };
            points.push(RangePoint {
                ts_us: t,
                labels: s.labels.clone(),
                value,
            });
        });
    }
    RangeVector { points }
}

pub(super) fn apply_over_time_value(
    func: &str,
    quantile: Option<f64>,
    samples: &[(i64, f64)],
) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }
    let values: Vec<f64> = samples.iter().map(|(_, value)| *value).collect();
    match func {
        "avg_over_time" => Some(values.iter().sum::<f64>() / values.len() as f64),
        "min_over_time" => Some(values.iter().copied().fold(f64::INFINITY, f64::min)),
        "max_over_time" => Some(values.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
        "sum_over_time" => Some(values.iter().sum()),
        "count_over_time" => Some(values.len() as f64),
        "quantile_over_time" => Some(quantile_value(values, quantile?)),
        "stddev_over_time" => Some(stdvar_value(&values).sqrt()),
        "stdvar_over_time" => Some(stdvar_value(&values)),
        "last_over_time" => samples.last().map(|(_, value)| *value),
        "present_over_time" => Some(1.0),
        "mad_over_time" => Some(mad_value(values)),
        _ => None,
    }
}

pub(super) fn quantile_value(mut values: Vec<f64>, q: f64) -> f64 {
    if q.is_nan() {
        return f64::NAN;
    }
    if q < 0.0 {
        return f64::NEG_INFINITY;
    }
    if q > 1.0 {
        return f64::INFINITY;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if values.len() == 1 {
        return values[0];
    }
    let rank = q * (values.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    if lower == upper {
        return values[lower];
    }
    let weight = rank - lower as f64;
    values[lower] + (values[upper] - values[lower]) * weight
}

pub(super) fn stdvar_value(values: &[f64]) -> f64 {
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    values
        .iter()
        .map(|value| {
            let delta = value - mean;
            delta * delta
        })
        .sum::<f64>()
        / values.len() as f64
}

pub(super) fn mad_value(values: Vec<f64>) -> f64 {
    let median = quantile_value(values.clone(), 0.5);
    let deviations = values
        .into_iter()
        .map(|value| (value - median).abs())
        .collect();
    quantile_value(deviations, 0.5)
}

/// 裸 instant-vector selector 的 range 求值：每个 step 取 `(t - 5min, t]` 内最新
/// 样本（Prometheus staleness lookback 语义），而不是原样吐出窗口内全部原始样本。
pub(super) fn samples_to_range_vector(
    series: Vec<Series>,
    start_us: i64,
    end_us: i64,
    step_us: i64,
) -> RangeVector {
    let mut points = Vec::new();
    for s in series {
        each_step_window(
            &s.samples,
            start_us,
            end_us,
            step_us,
            STALENESS_LOOKBACK_US,
            |t, win| {
                let Some(&(_, value)) = win.last() else {
                    return;
                };
                points.push(RangePoint {
                    ts_us: t,
                    labels: s.labels.clone(),
                    value,
                });
            },
        );
    }
    RangeVector { points }
}

/// `histogram_quantile(q, sum by(le)(rate(metric_bucket[5m])))`：
/// 输入要求是按 `le` 标签分桶的 cumulative bucket。算法：
/// 1. 按除 `le` 外的标签集分组；
/// 2. 每组内按 le 升序，找到第一个 `cumulative_count >= q * total` 的桶；
/// 3. 在该桶与前一桶之间线性插值。
pub fn apply_histogram_quantile(q: f64, inner: InstantVector) -> InstantVector {
    // group key = labels without "le"
    let mut grouped: HashMap<LabelSet, Vec<(f64, f64)>> = HashMap::new();
    for (mut labels, value) in inner.items {
        let le = match labels.remove("le") {
            Some(s) => match s.parse::<f64>() {
                Ok(v) => v,
                Err(_) if s == "+Inf" => f64::INFINITY,
                Err(_) => continue,
            },
            None => continue,
        };
        grouped.entry(labels).or_default().push((le, value));
    }
    let items: Vec<(LabelSet, f64)> = grouped
        .into_iter()
        .filter_map(|(labels, mut buckets)| {
            buckets.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let total = buckets.last()?.1;
            if total <= 0.0 {
                return Some((labels, f64::NAN));
            }
            let target = q * total;
            let mut prev = (0.0f64, 0.0f64);
            for &(le, cum) in &buckets {
                if cum >= target {
                    // 线性插值（与 Prometheus 默认行为接近）
                    let span = cum - prev.1;
                    if span <= 0.0 {
                        return Some((labels, le));
                    }
                    let frac = (target - prev.1) / span;
                    return Some((labels, prev.0 + frac * (le - prev.0)));
                }
                prev = (le, cum);
            }
            buckets.last().map(|(le, _)| (labels.clone(), *le))
        })
        .collect();
    InstantVector { items }
}

/// `histogram_fraction(lower, upper, v)`：classic bucket 直方图中，估算落在
/// `[lower, upper]` 区间内观测值的占比（每个分组返回一个标量，正常区间 ∈ `0..=1`）。
///
/// 与 [`apply_histogram_quantile`] 共用同一套 classic bucket 模型：
/// 1. 按除 `le` 外的标签集分组、组内按 le 升序；
/// 2. 以 `(0, 0)` 为起点、各 `(le, cumulative_count)` 为折点构成分段线性 CDF；
/// 3. `fraction = (rank(upper) - rank(lower)) / total`，`rank(x)` 见 [`histogram_bucket_rank`]。
pub(super) fn apply_histogram_fraction(
    lower: f64,
    upper: f64,
    inner: InstantVector,
) -> InstantVector {
    // group key = labels without "le"（与 apply_histogram_quantile 一致）
    let mut grouped: HashMap<LabelSet, Vec<(f64, f64)>> = HashMap::new();
    for (mut labels, value) in inner.items {
        let le = match labels.remove("le") {
            Some(s) => match s.parse::<f64>() {
                Ok(v) => v,
                Err(_) if s == "+Inf" => f64::INFINITY,
                Err(_) => continue,
            },
            None => continue,
        };
        grouped.entry(labels).or_default().push((le, value));
    }
    let items: Vec<(LabelSet, f64)> = grouped
        .into_iter()
        .filter_map(|(labels, mut buckets)| {
            buckets.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let total = buckets.last()?.1;
            if total <= 0.0 {
                return Some((labels, f64::NAN));
            }
            let frac = (histogram_bucket_rank(&buckets, upper)
                - histogram_bucket_rank(&buckets, lower))
                / total;
            Some((labels, frac))
        })
        .collect();
    InstantVector { items }
}

/// classic bucket CDF 在值 `x` 处的累计计数（rank）。`buckets` 已按 le 升序、值为累计计数；
/// 折线起点固定为 `(0, 0)`（与 [`apply_histogram_quantile`] 的插值约定一致）。
///
/// - `x <= 0` → `0`；
/// - 落在某有限桶 `(prev_le, le]` 内 → 在 `(prev_le, prev_cum)`..`(le, cum)` 间线性插值；
/// - `+Inf` 桶不可插值：`x = +Inf` 取全量 `cum`，超出最大有限 le 的有限 `x` 取上一个有限 le 处的累计值。
fn histogram_bucket_rank(buckets: &[(f64, f64)], x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut prev = (0.0f64, 0.0f64);
    for &(le, cum) in buckets {
        if le.is_infinite() {
            return if x.is_infinite() { cum } else { prev.1 };
        }
        if x <= le {
            let span = le - prev.0;
            if span <= 0.0 {
                return cum;
            }
            let frac = (x - prev.0) / span;
            return prev.1 + frac * (cum - prev.1);
        }
        prev = (le, cum);
    }
    prev.1
}

pub(super) fn group_by(
    input: &InstantVector,
    modifier: Option<&LabelModifier>,
) -> Vec<(LabelSet, Vec<f64>)> {
    let mut groups: HashMap<LabelSet, Vec<f64>> = HashMap::new();
    for (ls, v) in &input.items {
        let key = project_labels(ls, modifier);
        groups.entry(key).or_default().push(*v);
    }
    groups.into_iter().collect()
}

pub(super) fn project_labels(ls: &LabelSet, modifier: Option<&LabelModifier>) -> LabelSet {
    match modifier {
        None => LabelSet::new(), // aggregate without modifier → drop all labels
        Some(LabelModifier::Include(labels)) => {
            let mut out = LabelSet::new();
            for k in labels.labels.iter() {
                if let Some(v) = ls.get(k) {
                    out.insert(k.clone(), v.clone());
                }
            }
            out
        }
        Some(LabelModifier::Exclude(labels)) => {
            let mut out = ls.clone();
            for k in labels.labels.iter() {
                out.remove(k);
            }
            out
        }
    }
}
