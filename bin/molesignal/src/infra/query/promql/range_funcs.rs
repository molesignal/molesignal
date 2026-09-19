// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! range-vector 派生函数（输入 `metric[range]`，输出每条 series 一个值）：
//! `delta` / `idelta` / `deriv` / `predict_linear` / `resets` / `changes` /
//! `holt_winters`（别名 `double_exponential_smoothing`）。
//!
//! 与 rate 家族（functions.rs）分开：这些不做 per-second 归一，而是窗口内的
//! 差值 / 线性回归 / 计数 / 指数平滑。

use super::*;

/// `predict_linear` 的预测时长、`holt_winters` 的平滑因子，由 [`range_vector_args`] 填好。
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RangeFuncParams {
    pub(super) predict_t: f64,
    pub(super) hw_sf: f64,
    pub(super) hw_tf: f64,
}

pub(super) fn is_range_vector_function(name: &str) -> bool {
    super::capabilities::is_function_category(
        name,
        super::capabilities::FunctionCategory::RangeVector,
    )
}

/// instant 路径：`load_matrix` 已把样本裁到 `[at_us-range, at_us]`，整条 series 即窗口。
pub(super) fn apply_range_vector_func(
    func: &str,
    series: Vec<Series>,
    at_us: i64,
    params: RangeFuncParams,
) -> InstantVector {
    let items = series
        .into_iter()
        .filter_map(|s| {
            range_vector_value(func, &s.samples, at_us, params).map(|value| (s.labels, value))
        })
        .collect();
    InstantVector { items }
}

/// range 路径：按 step 步进，对每个步点取其前置 `range` 窗口计算。
pub(super) fn apply_range_vector_func_range(
    func: &str,
    series: Vec<Series>,
    range: Duration,
    start_us: i64,
    end_us: i64,
    step_us: i64,
    params: RangeFuncParams,
) -> RangeVector {
    let range_us = (range.as_micros() as i64).max(1);
    let mut points = Vec::new();
    for s in series {
        each_step_window(&s.samples, start_us, end_us, step_us, range_us, |t, win| {
            let Some(value) = range_vector_value(func, win, t, params) else {
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

pub(super) fn range_vector_value(
    func: &str,
    samples: &[(i64, f64)],
    at_us: i64,
    params: RangeFuncParams,
) -> Option<f64> {
    match func {
        "delta" => {
            let (_, first) = samples.first()?;
            let (_, last) = samples.last()?;
            (samples.len() >= 2).then_some(last - first)
        }
        "idelta" => {
            if samples.len() < 2 {
                return None;
            }
            let last = samples[samples.len() - 1].1;
            let prev = samples[samples.len() - 2].1;
            Some(last - prev)
        }
        "resets" => Some(count_resets(samples) as f64),
        "changes" => Some(count_changes(samples) as f64),
        "deriv" => linear_regression(samples, at_us).map(|(slope, _)| slope),
        "predict_linear" => linear_regression(samples, at_us)
            .map(|(slope, intercept)| intercept + slope * params.predict_t),
        "holt_winters" | "double_exponential_smoothing" => {
            holt_winters_value(samples, params.hw_sf, params.hw_tf)
        }
        _ => None,
    }
}

fn count_resets(samples: &[(i64, f64)]) -> usize {
    samples.windows(2).filter(|w| w[1].1 < w[0].1).count()
}

fn count_changes(samples: &[(i64, f64)]) -> usize {
    samples.windows(2).filter(|w| w[1].1 != w[0].1).count()
}

/// 以 `at_us` 为原点的最小二乘拟合，x 单位为秒。返回 `(slope_per_sec, intercept_at_origin)`；
/// 样本不足 2 个或时间无方差时返 `None`。
fn linear_regression(samples: &[(i64, f64)], at_us: i64) -> Option<(f64, f64)> {
    if samples.len() < 2 {
        return None;
    }
    let n = samples.len() as f64;
    let xs: Vec<f64> = samples
        .iter()
        .map(|(ts, _)| (ts - at_us) as f64 / 1_000_000.0)
        .collect();
    let mean_x = xs.iter().sum::<f64>() / n;
    let mean_y = samples.iter().map(|(_, v)| *v).sum::<f64>() / n;
    let mut cov = 0.0;
    let mut var = 0.0;
    for (x, (_, y)) in xs.iter().zip(samples.iter()) {
        let dx = x - mean_x;
        cov += dx * (y - mean_y);
        var += dx * dx;
    }
    if var == 0.0 {
        return None;
    }
    let slope = cov / var;
    let intercept = mean_y - slope * mean_x;
    Some((slope, intercept))
}

/// Prometheus `holt_winters` 双指数平滑，返回末端平滑值。`sf`/`tf` 须在 (0,1)。
fn holt_winters_value(samples: &[(i64, f64)], sf: f64, tf: f64) -> Option<f64> {
    let l = samples.len();
    if l < 2 {
        return None;
    }
    let mut s1 = samples[0].1;
    let mut b = samples[1].1 - samples[0].1;
    for sample in samples.iter().skip(1) {
        let x = sf * sample.1;
        let y = (1.0 - sf) * (s1 + b);
        let s0 = s1;
        s1 = x + y;
        b = tf * (s1 - s0) + (1.0 - tf) * b;
    }
    Some(s1)
}
