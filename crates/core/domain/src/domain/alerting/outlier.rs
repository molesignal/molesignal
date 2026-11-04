// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 离群规则：按列拆分的查询每轮把各组相互比较，偏离其余组的组各开各的事件。
//!
//! 偏离程度 = (值 − 各组中位数) / 尺度。尺度是 MAD（各组与中位数之差的中位数）乘 1.4826，正态分布下
//! 与标准差相当；大多数组一样时 MAD 为 0，尺度至少取中位数的 5%，免得一点点差别也算离群。有值的组
//! 少于 3 个时不判断。

use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result};

pub const DEFAULT_OUTLIER_TOLERANCE: f64 = 3.0;
pub const MIN_OUTLIER_TOLERANCE: f64 = 0.5;
pub const MAX_OUTLIER_TOLERANCE: f64 = 10.0;

/// 有值的组少于这么多不判断。
pub const MIN_OUTLIER_GROUPS: usize = 3;

const MAD_TO_SIGMA: f64 = 1.4826;

/// MAD 为 0 时，尺度至少是中位数的这么多。
const MIN_SCALE_SHARE: f64 = 0.05;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutlierDirection {
    #[default]
    Both,
    /// 只看比其余组高的。
    Above,
    /// 只看比其余组低的。
    Below,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlierParams {
    /// 偏离多少个尺度算离群；越小越敏感。
    #[serde(default = "default_tolerance")]
    pub tolerance: f64,
    #[serde(default)]
    pub direction: OutlierDirection,
}

fn default_tolerance() -> f64 {
    DEFAULT_OUTLIER_TOLERANCE
}

impl OutlierParams {
    pub fn validate(&self) -> Result<()> {
        if !self.tolerance.is_finite()
            || !(MIN_OUTLIER_TOLERANCE..=MAX_OUTLIER_TOLERANCE).contains(&self.tolerance)
        {
            return Err(Error::invalid(format!(
                "outlier.tolerance is {MIN_OUTLIER_TOLERANCE} to {MAX_OUTLIER_TOLERANCE}"
            )));
        }

        Ok(())
    }

    /// 偏离程度 `score` 算不算离群。
    pub fn is_outlier(&self, score: f64) -> bool {
        match self.direction {
            OutlierDirection::Both => score.abs() > self.tolerance,
            OutlierDirection::Above => score > self.tolerance,
            OutlierDirection::Below => -score > self.tolerance,
        }
    }
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;

    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

/// 每组的偏离程度（带符号，高于中位数为正）；没有值的组为 `None`。有值的组不够时整个为 `None`。
pub fn outlier_scores(values: &[Option<f64>]) -> Option<Vec<Option<f64>>> {
    let mut present = values
        .iter()
        .flatten()
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();

    if present.len() < MIN_OUTLIER_GROUPS {
        return None;
    }

    let center = median(&mut present);
    let mut deviations = present
        .iter()
        .map(|value| (value - center).abs())
        .collect::<Vec<_>>();
    let mad = median(&mut deviations);
    let scale = (MAD_TO_SIGMA * mad)
        .max(MIN_SCALE_SHARE * center.abs())
        .max(f64::EPSILON);

    Some(
        values
            .iter()
            .map(|value| {
                value
                    .filter(|value| value.is_finite())
                    .map(|value| (value - center) / scale)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(direction: OutlierDirection) -> OutlierParams {
        OutlierParams {
            tolerance: DEFAULT_OUTLIER_TOLERANCE,
            direction,
        }
    }

    #[test]
    fn one_host_far_above_the_rest_stands_out() {
        let scores = outlier_scores(&[
            Some(100.0),
            Some(101.0),
            Some(99.0),
            Some(100.0),
            Some(300.0),
        ])
        .unwrap();
        let flagged = scores
            .iter()
            .map(|score| {
                score.is_some_and(|score| params(OutlierDirection::Both).is_outlier(score))
            })
            .collect::<Vec<_>>();

        assert_eq!(flagged, vec![false, false, false, false, true]);
        assert!(!params(OutlierDirection::Below).is_outlier(scores[4].unwrap()));
    }

    #[test]
    fn identical_groups_need_a_real_gap() {
        let scores = outlier_scores(&[Some(10.0), Some(10.0), Some(10.0), Some(10.2)]).unwrap();
        assert!(!params(OutlierDirection::Both).is_outlier(scores[3].unwrap()));

        let scores = outlier_scores(&[Some(0.0), Some(0.0), Some(0.0), Some(5.0)]).unwrap();
        assert!(params(OutlierDirection::Above).is_outlier(scores[3].unwrap()));
    }

    #[test]
    fn fewer_than_three_values_give_no_verdict() {
        assert!(outlier_scores(&[Some(1.0), None, Some(50.0)]).is_none());

        let scores = outlier_scores(&[Some(1.0), None, Some(1.0), Some(1.1)]).unwrap();
        assert!(scores[1].is_none());
    }

    #[test]
    fn tolerance_has_bounds() {
        assert!(
            OutlierParams {
                tolerance: 0.1,
                direction: OutlierDirection::Both
            }
            .validate()
            .is_err()
        );
        assert!(
            OutlierParams {
                tolerance: f64::NAN,
                direction: OutlierDirection::Both
            }
            .validate()
            .is_err()
        );
        assert!(params(OutlierDirection::Both).validate().is_ok());
    }
}
