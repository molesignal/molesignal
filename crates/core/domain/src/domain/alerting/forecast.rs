// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 预测规则：拿规则查询过去一段时间的序列往后推，预计在提前量之内越过阈值就触发。
//!
//! - 线性：Theil–Sen（两两斜率的中位数作斜率，各点残差的中位数作截距），个别尖刺带不偏趋势。
//! - 按天 / 按周：取一个（或几个）周期之前同一时刻的值（相邻两点线性插值），再加上这段时间里的
//!   趋势（同一个 Theil–Sen 斜率乘以相隔的时间）。
//!
//! 序列记在规则的去抖状态里：每轮评估把这一轮的值记成一个点，相邻的点至少隔一个步长；规则刚建好或
//! 查询改过时，先在回看时间里均匀补一批点。

use serde::{Deserialize, Serialize};

use super::rule::ComparisonOp;
use crate::shared::{Error, Result};

pub const DEFAULT_FORECAST_LOOKBACK_DAYS: u32 = 7;
pub const MAX_FORECAST_LOOKBACK_DAYS: u32 = 30;
pub const DEFAULT_FORECAST_LEAD_SECS: u32 = 86_400;
pub const MIN_FORECAST_LEAD_SECS: u32 = 900;
pub const MAX_FORECAST_LEAD_SECS: u32 = 7 * 86_400;

/// 序列最多留多少个点。
pub const MAX_FORECAST_POINTS: usize = 168;

/// 刚开始时补多少个历史点：每个点跑一次规则查询。
pub const FORECAST_BACKFILL_POINTS: usize = 48;

/// 少于这么多点不预测。
pub const MIN_FORECAST_POINTS: usize = 6;

/// 往后看最多看多少个时刻。
const MAX_FORECAST_CHECKS: i64 = 240;

const HOUR_MICROS: f64 = 3_600_000_000.0;
const DAY_MICROS: i64 = 86_400_000_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForecastModel {
    #[default]
    Linear,
    Daily,
    Weekly,
}

impl ForecastModel {
    fn season_micros(self) -> Option<i64> {
        match self {
            Self::Linear => None,
            Self::Daily => Some(DAY_MICROS),
            Self::Weekly => Some(7 * DAY_MICROS),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Daily => "daily",
            Self::Weekly => "weekly",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForecastParams {
    /// 拿过去几天的序列来推。
    #[serde(default = "default_lookback_days")]
    pub lookback_days: u32,
    /// 提前量：预计在这么多秒之内越线就触发。
    #[serde(default = "default_lead_secs")]
    pub lead_secs: u32,
    #[serde(default)]
    pub model: ForecastModel,
}

fn default_lookback_days() -> u32 {
    DEFAULT_FORECAST_LOOKBACK_DAYS
}

fn default_lead_secs() -> u32 {
    DEFAULT_FORECAST_LEAD_SECS
}

impl ForecastParams {
    pub fn validate(&self) -> Result<()> {
        if !(1..=MAX_FORECAST_LOOKBACK_DAYS).contains(&self.lookback_days) {
            return Err(Error::invalid(format!(
                "forecast.lookback_days is 1 to {MAX_FORECAST_LOOKBACK_DAYS}"
            )));
        }

        if !(MIN_FORECAST_LEAD_SECS..=MAX_FORECAST_LEAD_SECS).contains(&self.lead_secs) {
            return Err(Error::invalid(format!(
                "forecast.lead_secs is {MIN_FORECAST_LEAD_SECS} to {MAX_FORECAST_LEAD_SECS}"
            )));
        }

        if self.model == ForecastModel::Weekly && self.lookback_days < 7 {
            return Err(Error::invalid(
                "a weekly forecast looks back at least 7 days",
            ));
        }

        Ok(())
    }

    pub fn lookback_micros(&self) -> i64 {
        i64::from(self.lookback_days) * DAY_MICROS
    }

    pub fn lead_micros(&self) -> i64 {
        i64::from(self.lead_secs) * 1_000_000
    }

    /// 相邻两点至少隔多久：评估窗口，和回看时间按点数上限分下来，取大的。
    pub fn step_micros(&self, period_secs: u32) -> i64 {
        let period = i64::from(period_secs.max(1)) * 1_000_000;

        period.max(self.lookback_micros() / MAX_FORECAST_POINTS as i64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SeriesPoint {
    pub at: i64,
    pub value: f64,
}

/// 记在去抖状态里的序列。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForecastSeries {
    /// 规则查询、窗口与回看的摘要：规则改过以后对不上，序列重新补。
    #[serde(default)]
    pub signature: String,
    /// 上次补历史点的时间。
    #[serde(default)]
    pub filled_at: Option<i64>,
    #[serde(default)]
    pub points: Vec<SeriesPoint>,
}

impl ForecastSeries {
    /// 序列要不要重新补：没补过，或规则改过。
    pub fn needs_fill(&self, signature: &str) -> bool {
        self.filled_at.is_none() || self.signature != signature
    }

    /// 记下一个点。最后一个点离它前一个点还不到一个步长时，它只是这一步里较早的读数，换成新的；
    /// 否则追加。再去掉回看以外和超出上限的点。
    pub fn record(&mut self, point: SeriesPoint, step: i64, lookback: i64) {
        if self.points.last().is_some_and(|last| point.at <= last.at) {
            return;
        }

        let replace = match self.points.as_slice() {
            [.., before, _] => point.at - before.at < step,
            _ => false,
        };

        if replace {
            self.points.pop();
        }

        self.points.push(point);

        let oldest = point.at - lookback;
        self.points.retain(|kept| kept.at >= oldest);

        if self.points.len() > MAX_FORECAST_POINTS + 1 {
            let extra = self.points.len() - MAX_FORECAST_POINTS - 1;
            self.points.drain(..extra);
        }
    }
}

/// 趋势：从 `origin` 起，每小时变化 `slope`，`origin` 时的值为 `intercept`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trend {
    pub origin: i64,
    pub slope: f64,
    pub intercept: f64,
}

impl Trend {
    pub fn at(&self, at: i64) -> f64 {
        self.intercept + self.slope * hours(at - self.origin)
    }
}

fn hours(micros: i64) -> f64 {
    micros as f64 / HOUR_MICROS
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }

    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;

    Some(if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    })
}

/// Theil–Sen 趋势；点少于两个、或时间都一样时没有。
pub fn theil_sen(points: &[SeriesPoint]) -> Option<Trend> {
    let origin = points.first()?.at;
    let mut slopes = Vec::with_capacity(points.len() * points.len() / 2);

    for (index, left) in points.iter().enumerate() {
        for right in &points[index + 1..] {
            if right.at != left.at {
                slopes.push((right.value - left.value) / hours(right.at - left.at));
            }
        }
    }

    let slope = median(&mut slopes)?;
    let mut residuals = points
        .iter()
        .map(|point| point.value - slope * hours(point.at - origin))
        .collect::<Vec<_>>();
    let intercept = median(&mut residuals)?;

    Some(Trend {
        origin,
        slope,
        intercept,
    })
}

/// `at` 时刻的值，按相邻两点线性插值；在序列之外没有。点要按时间排好。
fn interpolate(points: &[SeriesPoint], at: i64) -> Option<f64> {
    let after = points.partition_point(|point| point.at < at);
    let right = points.get(after)?;

    if right.at == at {
        return Some(right.value);
    }

    let left = points.get(after.checked_sub(1)?)?;
    let share = (at - left.at) as f64 / (right.at - left.at) as f64;

    Some(left.value + (right.value - left.value) * share)
}

/// 预计的越线：最早的那一刻，和那时预计的值。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossing {
    pub at: i64,
    pub value: f64,
}

/// 从 `now` 起到提前量为止，第一次预计满足 `operator threshold` 的时刻。序列不够推（点太少、按天按周
/// 却不满一个周期）时返回 `None`，这一轮不下结论；够了但不会越线时返回 `Some(None)`。最后一个点
/// 已经越线时，越线就在它那一刻。
pub fn forecast_crossing(
    params: &ForecastParams,
    points: &[SeriesPoint],
    now: i64,
    operator: ComparisonOp,
    threshold: f64,
) -> Option<Option<Crossing>> {
    if points.len() < MIN_FORECAST_POINTS {
        return None;
    }

    let first = points.first()?;
    let last = points.last()?;

    if let Some(season) = params.model.season_micros()
        && last.at - first.at < season
    {
        return None;
    }

    let trend = theil_sen(points)?;

    if operator.holds(last.value, threshold) {
        return Some(Some(Crossing {
            at: last.at,
            value: last.value,
        }));
    }

    let lead = params.lead_micros();
    let step = (lead / MAX_FORECAST_CHECKS).max(60_000_000);
    let start = now.max(last.at);
    let mut at = start + step;

    while at <= start + lead {
        let predicted = match params.model.season_micros() {
            None => Some(trend.at(at)),
            Some(season) => {
                let gap = at - last.at;
                let seasons = (gap + season - 1).div_euclid(season).max(1);
                let back = seasons * season;

                interpolate(points, at - back).map(|base| base + trend.slope * hours(back))
            }
        };

        if let Some(value) = predicted
            && value.is_finite()
            && operator.holds(value, threshold)
        {
            return Some(Some(Crossing { at, value }));
        }

        at += step;
    }

    Some(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 3_600_000_000;

    fn params(model: ForecastModel) -> ForecastParams {
        ForecastParams {
            lookback_days: 7,
            lead_secs: 86_400,
            model,
        }
    }

    fn series(hours: i64, value: impl Fn(i64) -> f64) -> Vec<SeriesPoint> {
        (0..=hours)
            .map(|hour| SeriesPoint {
                at: hour * HOUR,
                value: value(hour),
            })
            .collect()
    }

    #[test]
    fn theil_sen_ignores_a_spike() {
        let mut points = series(24, |hour| 10.0 + 2.0 * hour as f64);
        points[12].value = 500.0;

        let trend = theil_sen(&points).unwrap();
        assert!((trend.slope - 2.0).abs() < 1e-9);
        assert!((trend.at(30 * HOUR) - 70.0).abs() < 1e-6);
    }

    #[test]
    fn a_rising_line_crosses_within_the_lead() {
        // 每小时涨 1%，现在 60%，大约 30 小时后到 90%：提前量 1 天看不到，2 天看得到。
        let points = series(48, |hour| 12.0 + hour as f64);
        let now = 48 * HOUR;

        let day = forecast_crossing(
            &params(ForecastModel::Linear),
            &points,
            now,
            ComparisonOp::Gte,
            90.0,
        );
        assert_eq!(day, Some(None));

        let two_days = ForecastParams {
            lead_secs: 2 * 86_400,
            ..params(ForecastModel::Linear)
        };
        let crossing = forecast_crossing(&two_days, &points, now, ComparisonOp::Gte, 90.0)
            .unwrap()
            .unwrap();
        assert!((crossing.at - (now + 30 * HOUR)).abs() <= 15 * 60_000_000);
    }

    #[test]
    fn a_daily_pattern_is_carried_forward_with_the_trend() {
        // 每天 12 点冲到 100，平时 20；趋势平。明天中午会越过 90，线性看不出来。
        let points = series(72, |hour| if hour % 24 == 12 { 100.0 } else { 20.0 });
        let now = 72 * HOUR;

        let daily = forecast_crossing(
            &params(ForecastModel::Daily),
            &points,
            now,
            ComparisonOp::Gt,
            90.0,
        )
        .unwrap()
        .unwrap();
        assert!(daily.at > now && daily.at <= now + 13 * HOUR);

        assert_eq!(
            forecast_crossing(
                &params(ForecastModel::Linear),
                &points,
                now,
                ComparisonOp::Gt,
                90.0
            ),
            Some(None)
        );
    }

    #[test]
    fn too_little_history_gives_no_verdict() {
        let points = series(3, |hour| hour as f64);
        assert_eq!(
            forecast_crossing(
                &params(ForecastModel::Linear),
                &points,
                3 * HOUR,
                ComparisonOp::Gt,
                9.0
            ),
            None
        );

        let half_day = series(12, |hour| hour as f64);
        assert_eq!(
            forecast_crossing(
                &params(ForecastModel::Daily),
                &half_day,
                12 * HOUR,
                ComparisonOp::Gt,
                99.0
            ),
            None
        );
    }

    #[test]
    fn the_series_keeps_one_point_a_step_and_only_the_lookback() {
        let mut kept = ForecastSeries::default();
        let step = HOUR;
        let lookback = 10 * HOUR;

        for minute in 0..=(12 * 60) {
            kept.record(
                SeriesPoint {
                    at: minute * 60_000_000,
                    value: minute as f64,
                },
                step,
                lookback,
            );
        }

        assert!(kept.points.len() <= 12);
        assert!(kept.points.first().unwrap().at >= 2 * HOUR);
        assert_eq!(kept.points.last().unwrap().value, 720.0);
        assert!(
            kept.points
                .windows(3)
                .all(|three| three[2].at - three[0].at >= step)
        );
    }

    #[test]
    fn weekly_needs_a_week_and_leads_have_bounds() {
        assert!(params(ForecastModel::Weekly).validate().is_ok());
        assert!(
            ForecastParams {
                lookback_days: 3,
                ..params(ForecastModel::Weekly)
            }
            .validate()
            .is_err()
        );
        assert!(
            ForecastParams {
                lead_secs: 60,
                ..params(ForecastModel::Linear)
            }
            .validate()
            .is_err()
        );
        assert!(
            ForecastParams {
                lookback_days: 31,
                ..params(ForecastModel::Linear)
            }
            .validate()
            .is_err()
        );
    }
}
