// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PagerDuty 的限制（`restrictions`）铺成一周的分钟。时刻按请求的 `time_zone` 渲染，所以先认一下响应是不是按
//! 排班自己的时区渲染的。

use chrono_tz::Tz;
use serde::Deserialize;

use super::{
    super::{
        Draft, MAX_RESTRICTIONS, rendered,
        weekly::{WEEK, WeekMinutes},
    },
    Layer,
};
use crate::{
    app::alerting::oncall_transfer::Finding, domain::alerting::time_window::MINUTES_PER_DAY,
};

#[derive(Deserialize)]
pub(super) struct Restriction {
    #[serde(rename = "type")]
    kind: String,
    start_time_of_day: String,
    duration_seconds: i64,
    #[serde(default)]
    start_day_of_week: Option<u32>,
}

/// 这一层的限制铺成分钟；没有限制时为空。说不清的限制挡住写入。
pub(super) fn restricted_minutes(
    draft: &mut Draft<'_>,
    layer: &Layer,
    name: &str,
    tz: Tz,
) -> Option<WeekMinutes> {
    if layer.restrictions.is_empty() {
        return None;
    }

    if rendered::offset_is_foreign(&layer.rotation_virtual_start, tz) {
        draft.block(
            Finding::new("rendered_in_other_zone")
                .with("rotation", name)
                .with("value", layer.rotation_virtual_start.clone()),
        );
    }

    let mut minutes = WeekMinutes::empty();

    if layer.restrictions.len() > MAX_RESTRICTIONS {
        draft.block(
            Finding::new("restriction_unsupported")
                .with("rotation", name)
                .with(
                    "value",
                    format!("{} restrictions", layer.restrictions.len()),
                ),
        );

        return Some(minutes);
    }

    for restriction in &layer.restrictions {
        if let Err(finding) = add_restriction(&mut minutes, restriction) {
            draft.block(finding.with("rotation", name));
        }
    }

    Some(minutes)
}

fn add_restriction(minutes: &mut WeekMinutes, restriction: &Restriction) -> Result<(), Finding> {
    let unsupported =
        || Finding::new("restriction_unsupported").with("value", describe(restriction));
    let day_minutes = usize::from(MINUTES_PER_DAY);

    let start = start_minute(&restriction.start_time_of_day).ok_or_else(unsupported)?;
    let seconds = usize::try_from(restriction.duration_seconds).map_err(|_| unsupported())?;

    if seconds % 60 != 0 {
        return Err(unsupported());
    }

    let length = seconds / 60;

    match (restriction.kind.as_str(), restriction.start_day_of_week) {
        ("daily_restriction", _) => minutes.add_daily(start, length),
        // ISO 8601：1 是周一，7 是周日；这里从周日数起。
        ("weekly_restriction", Some(day @ 1..=7)) => {
            let day = day as usize % 7;

            minutes.add(day * day_minutes + start, length.min(WEEK));
        }
        _ => return Err(unsupported()),
    }

    Ok(())
}

fn describe(restriction: &Restriction) -> String {
    format!(
        "{} {} {}s",
        restriction.kind, restriction.start_time_of_day, restriction.duration_seconds
    )
}

/// `HH:MM:SS` 换成当天第几分钟；不是整分的说不清。
fn start_minute(text: &str) -> Option<usize> {
    let mut parts = text.trim().split(':');
    let hour: usize = parts.next()?.parse().ok()?;
    let minute: usize = parts.next()?.parse().ok()?;
    let second: usize = parts.next().unwrap_or("0").parse().ok()?;

    (hour < 24 && minute < 60 && second == 0).then_some(hour * 60 + minute)
}
