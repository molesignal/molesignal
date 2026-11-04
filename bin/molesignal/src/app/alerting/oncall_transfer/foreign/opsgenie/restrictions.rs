// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Opsgenie 的时段限制（`timeRestriction`）铺成一周的分钟。

use serde::Deserialize;

use super::super::{
    MAX_RESTRICTIONS,
    weekly::{WEEK, WeekMinutes},
};
use crate::{
    app::alerting::oncall_transfer::Finding, domain::alerting::time_window::MINUTES_PER_DAY,
};

#[derive(Deserialize)]
pub(super) struct TimeRestriction {
    #[serde(rename = "type")]
    kind: String,
    // 创建时的文档写单个 `restriction`，响应的样例写 `restrictions`。
    #[serde(default)]
    restriction: Option<Frame>,
    #[serde(default)]
    restrictions: Vec<Frame>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Frame {
    #[serde(default)]
    start_day: Option<String>,
    start_hour: u32,
    start_min: u32,
    #[serde(default)]
    end_day: Option<String>,
    end_hour: u32,
    end_min: u32,
}

/// 时段限制铺成分钟。结束不晚于开始表示跨到第二天（或下一周）；起止相同说不清。
pub(super) fn restricted_minutes(restriction: &TimeRestriction) -> Result<WeekMinutes, Finding> {
    let unsupported =
        || Finding::new("restriction_unsupported").with("value", restriction.kind.clone());
    let frames: Vec<&Frame> = restriction
        .restrictions
        .iter()
        .chain(restriction.restriction.iter())
        .collect();

    if frames.is_empty() || frames.len() > MAX_RESTRICTIONS {
        return Err(unsupported());
    }

    let day_minutes = usize::from(MINUTES_PER_DAY);
    let mut minutes = WeekMinutes::empty();

    for frame in frames {
        let start = minute_of_day(frame.start_hour, frame.start_min).ok_or_else(unsupported)?;
        let end = minute_of_day(frame.end_hour, frame.end_min).ok_or_else(unsupported)?;

        match restriction.kind.as_str() {
            "time-of-day" => {
                let length = span(start, end, day_minutes).ok_or_else(unsupported)?;

                minutes.add_daily(start, length);
            }
            "weekday-and-time-of-day" => {
                let day = |name: &Option<String>| name.as_deref().and_then(weekday);
                let start_day = day(&frame.start_day).ok_or_else(unsupported)?;
                let end_day = day(&frame.end_day).ok_or_else(unsupported)?;
                let start = start_day * day_minutes + start;
                let end = end_day * day_minutes + end;

                minutes.add(start, span(start, end, WEEK).ok_or_else(unsupported)?);
            }
            _ => return Err(unsupported()),
        }
    }

    Ok(minutes)
}

fn minute_of_day(hour: u32, minute: u32) -> Option<usize> {
    let total = hour.checked_mul(60)?.checked_add(minute)?;

    (minute < 60 && total <= u32::from(MINUTES_PER_DAY)).then_some(total as usize)
}

/// `start` 到 `end` 有多长，结束不晚于开始时绕一圈（`cycle`）。
fn span(start: usize, end: usize, cycle: usize) -> Option<usize> {
    match end.cmp(&start) {
        std::cmp::Ordering::Greater => Some(end - start),
        std::cmp::Ordering::Less => Some(end + cycle - start),
        std::cmp::Ordering::Equal => None,
    }
}

/// 周日是 0。
fn weekday(name: &str) -> Option<usize> {
    [
        "sunday",
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
    ]
    .iter()
    .position(|day| name.eq_ignore_ascii_case(day))
}
