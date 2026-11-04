// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 时间段引用节假日日历时怎么算（[`CalendarUse`]）。
//!
//! 每一段算在它开始的那一天（跨午夜的一段算前一天），所以节假日看的是那一天：周五是节假日时，
//! 「周五 22:00–06:00」到周六早上都不算。补班日按周一的时段算。

use chrono::{Datelike, Duration, NaiveDate, Timelike};
use serde::{Deserialize, Serialize};

use super::{WeeklyRange, local_date, local_instant, range_transitions, ranges_contain};
use crate::{
    domain::alerting::holiday::{self, DayKind},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 补班日按星期几的时段算：周一。
const WORKDAY_COUNTS_AS: u8 = 1;

/// 时间段怎么看一份节假日日历。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarUse {
    pub calendar_id: Id,
    pub holidays: HolidayTreatment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HolidayTreatment {
    /// 节假日不算在这些时段里；补班日按周一的时段算。
    Exclude,
    /// 只有节假日才算，时段照样限定：每天 00:00–24:00 就是节假日全天。
    Only,
}

impl CalendarUse {
    pub fn validate(&self) -> Result<()> {
        if self.calendar_id.0.trim().is_empty() {
            return Err(Error::invalid("calendar.calendar_id cannot be empty"));
        }

        Ok(())
    }

    /// 在 `org_id` 里能用：有这份日历，并且是这个组织的。
    pub fn check(&self, org_id: &Id) -> Result<()> {
        self.validate()?;

        if !holiday::belongs_to(&self.calendar_id, org_id) {
            return Err(Error::invalid(format!(
                "no holiday calendar {} in this organization",
                self.calendar_id
            )));
        }

        Ok(())
    }

    /// 这一天的时段按星期几（0 = 周日）算；`None` 是这一天的时段都不算。
    fn counts_as(&self, date: NaiveDate) -> Option<u8> {
        let weekday = date.weekday().num_days_from_sunday() as u8;

        match (self.holidays, holiday::day_kind(&self.calendar_id, date)) {
            (HolidayTreatment::Exclude, Some(DayKind::Holiday)) => None,
            (HolidayTreatment::Exclude, Some(DayKind::Workday)) => Some(WORKDAY_COUNTS_AS),
            (HolidayTreatment::Exclude, None) => Some(weekday),
            (HolidayTreatment::Only, Some(DayKind::Holiday)) => Some(weekday),
            (HolidayTreatment::Only, _) => None,
        }
    }
}

/// 一组段在 `tz` 里包含 `at`：有日历时按日历换算每一天。
pub fn windows_contain(
    ranges: &[WeeklyRange],
    tz: chrono_tz::Tz,
    calendar: Option<&CalendarUse>,
    at: TimestampMicros,
) -> bool {
    let Some(calendar) = calendar else {
        return ranges_contain(ranges, tz, at);
    };

    let local = at.to_datetime().with_timezone(&tz);
    let today = local.date_naive();
    let minute = (local.hour() * 60 + local.minute()) as u16;
    let today_as = calendar.counts_as(today);
    let yesterday_as = today
        .pred_opt()
        .and_then(|yesterday| calendar.counts_as(yesterday));

    ranges
        .iter()
        .any(|range| range.contains_anchored(today_as, yesterday_as, minute))
}

/// `(from, to)` 里一组段开始、结束的时刻（微秒），升序、不重复；有日历时只留真正变了的那些刻。
pub fn windows_transitions(
    ranges: &[WeeklyRange],
    tz: chrono_tz::Tz,
    calendar: Option<&CalendarUse>,
    from: i64,
    to: i64,
) -> Vec<i64> {
    let Some(calendar) = calendar else {
        return range_transitions(ranges, tz, from, to);
    };

    if ranges.is_empty() || to <= from {
        return Vec::new();
    }

    // 每一天里每一段的开始、结束（不看星期），加上每天零点：状态只可能在这些刻变。
    let first = local_date(tz, from) - Duration::days(1);
    let last = local_date(tz, to) + Duration::days(1);
    let mut candidates = Vec::new();

    for date in first.iter_days().take_while(|date| *date <= last) {
        candidates.push(local_instant(tz, super::ClockTime::MIDNIGHT.on(date)));

        for range in ranges {
            let end_date = if range.crosses_midnight() {
                date + Duration::days(1)
            } else {
                date
            };

            candidates.push(local_instant(tz, range.start.on(date)));
            candidates.push(local_instant(tz, range.end.on(end_date)));
        }
    }

    let mut points: Vec<i64> = candidates
        .into_iter()
        .map(|at| at.timestamp_micros())
        .filter(|at| *at > from && *at < to)
        .filter(|at| {
            windows_contain(ranges, tz, Some(calendar), TimestampMicros(*at - 1))
                != windows_contain(ranges, tz, Some(calendar), TimestampMicros(*at))
        })
        .collect();

    points.sort_unstable();
    points.dedup();
    points
}

impl WeeklyRange {
    /// 今天按星期 `today` 算、昨天按星期 `yesterday` 算（`None` 是那天不算）时，这一段包含今天的
    /// 第 `minute` 分钟。
    pub(super) fn contains_anchored(
        &self,
        today: Option<u8>,
        yesterday: Option<u8>,
        minute: u16,
    ) -> bool {
        let started_today = today.is_some_and(|day| self.on_day(day))
            && self.start.0 <= minute
            && (self.crosses_midnight() || minute < self.end.0);
        let from_yesterday = self.crosses_midnight()
            && yesterday.is_some_and(|day| self.on_day(day))
            && minute < self.end.0;

        started_today || from_yesterday
    }
}
