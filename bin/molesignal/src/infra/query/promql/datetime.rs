// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 日期/时间函数（UTC）：`minute` / `hour` / `day_of_week` / `day_of_month` /
//! `day_of_year` / `days_in_month` / `month` / `year`，以及 `timestamp`。
//!
//! 与 Prometheus 一致：日期类函数把输入向量的**值**当作 Unix 秒解读，无参时默认
//! 作用于 `vector(time())`（即求值时刻）。`timestamp(v)` 返回每个样本自身的时间戳。

use chrono::{DateTime, Datelike, Timelike, Utc};

use super::*;

pub(super) fn is_datetime_function(name: &str) -> bool {
    super::capabilities::is_function_category(name, super::capabilities::FunctionCategory::DateTime)
}

pub(super) fn apply_datetime(func: &str, input: InstantVector) -> InstantVector {
    let items = input
        .items
        .into_iter()
        .map(|(labels, value)| (labels, datetime_field(func, value)))
        .collect();
    InstantVector { items }
}

pub(super) fn apply_datetime_range(func: &str, mut input: RangeVector) -> RangeVector {
    for point in &mut input.points {
        point.value = datetime_field(func, point.value);
    }
    input
}

/// `timestamp(v)`：instant 路径下样本取自求值时刻，故每条返回 `at_us` 对应的秒。
pub(super) fn apply_timestamp(input: InstantVector, at_us: i64) -> InstantVector {
    let seconds = at_us as f64 / 1_000_000.0;
    let items = input
        .items
        .into_iter()
        .map(|(labels, _)| (labels, seconds))
        .collect();
    InstantVector { items }
}

/// range 路径下每个点带有自己的时间戳，`timestamp` 可精确返回该点的秒值。
pub(super) fn apply_timestamp_range(mut input: RangeVector) -> RangeVector {
    for point in &mut input.points {
        point.value = point.ts_us as f64 / 1_000_000.0;
    }
    input
}

fn datetime_field(func: &str, value: f64) -> f64 {
    let Some(dt) = unix_seconds_to_utc(value) else {
        return f64::NAN;
    };
    match func {
        "minute" => dt.minute() as f64,
        "hour" => dt.hour() as f64,
        // Prometheus: 0 = Sunday .. 6 = Saturday。
        "day_of_week" => dt.weekday().num_days_from_sunday() as f64,
        "day_of_month" => dt.day() as f64,
        "day_of_year" => dt.ordinal() as f64,
        "days_in_month" => days_in_month(dt.year(), dt.month()) as f64,
        "month" => dt.month() as f64,
        "year" => dt.year() as f64,
        _ => f64::NAN,
    }
}

fn unix_seconds_to_utc(value: f64) -> Option<DateTime<Utc>> {
    if !value.is_finite() {
        return None;
    }
    DateTime::from_timestamp(value as i64, 0)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first_this = chrono::NaiveDate::from_ymd_opt(year, month, 1);
    let first_next = chrono::NaiveDate::from_ymd_opt(next_year, next_month, 1);
    match (first_this, first_next) {
        (Some(a), Some(b)) => (b - a).num_days() as u32,
        _ => 30,
    }
}
