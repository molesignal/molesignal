// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 按时区重复的时间段。排班的生效时段、静默的循环窗口、路由规则和告警流水线的时间条件、升级策略的
//! 支持时段都用它。
//!
//! - [`ClockTime`]：一天里的时刻，精确到分钟，写作 `HH:MM`；`24:00` 只能作结束。
//! - [`WeeklyRange`]：每周哪几天、从几点到几点。结束不晚于开始表示跨午夜，这一段算在开始的那一天：
//!   「周五 22:00–06:00」是周五夜里到周六早上。
//! - [`WeeklyWindows`]：一个时区加若干段，可以按一份节假日日历算（[`CalendarUse`]）。
//! - [`MonthlyWindow`]：每月第几天或第几个星期几，从某个时刻起持续一段时间。
//!
//! 本地时间换成时刻时按时区和夏令时算（[`local_instant`]）：落在夏令时跳过的那一段里取跳过之后的
//! 第一刻，落在重复的那一段里取前一个。

mod calendar;
mod monthly;

use std::fmt;

pub use calendar::{CalendarUse, HolidayTreatment, windows_contain, windows_transitions};
use chrono::{
    DateTime, Datelike, Duration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone,
    Timelike, Utc,
};
pub use monthly::{MAX_MONTHLY_DURATION_MINUTES, MonthDay, MonthlyWindow};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::shared::{Error, Result, time::TimestampMicros};

/// 一天有多少分钟。
pub const MINUTES_PER_DAY: u16 = 24 * 60;

/// 一组时间段最多几段。
pub const MAX_WEEKLY_RANGES: usize = 16;

/// 每天都算的星期掩码。
pub const EVERY_DAY: u8 = 0b111_1111;

/// 每天全天。
pub const ALL_DAY: WeeklyRange = WeeklyRange {
    weekday_mask: EVERY_DAY,
    start: ClockTime::MIDNIGHT,
    end: ClockTime::END_OF_DAY,
};

/// 一天里的时刻，精确到分钟：从零点起的分钟数，`0..=1440`。写作 `HH:MM`，`24:00` 只能作结束。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClockTime(u16);

impl ClockTime {
    pub const MIDNIGHT: Self = Self(0);
    pub const END_OF_DAY: Self = Self(MINUTES_PER_DAY);

    /// `hour` 0–24、`minute` 0–59；24 点只有 `24:00`。
    pub fn new(hour: u8, minute: u8) -> Option<Self> {
        let minutes = u16::from(hour) * 60 + u16::from(minute);
        (minute < 60 && minutes <= MINUTES_PER_DAY).then_some(Self(minutes))
    }

    pub fn minutes(self) -> u16 {
        self.0
    }

    /// 读 `HH:MM`。
    pub fn parse(text: &str) -> Result<Self> {
        let invalid = || Error::invalid(format!("`{text}` is not a time of day: use HH:MM"));
        let (hour, minute) = text.trim().split_once(':').ok_or_else(invalid)?;

        if hour.len() != 2 || minute.len() != 2 {
            return Err(invalid());
        }

        let hour: u8 = hour.parse().map_err(|_| invalid())?;
        let minute: u8 = minute.parse().map_err(|_| invalid())?;

        Self::new(hour, minute).ok_or_else(invalid)
    }

    /// 这一天里的这个时刻；`24:00` 是第二天零点。
    fn on(self, date: NaiveDate) -> NaiveDateTime {
        date.and_time(NaiveTime::MIN) + Duration::minutes(i64::from(self.0))
    }
}

impl fmt::Display for ClockTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.0 / 60, self.0 % 60)
    }
}

impl Serialize for ClockTime {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ClockTime {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// 每周哪几天、从几点到几点（本地时间）。结束不晚于开始表示跨午夜，算在开始的那一天。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyRange {
    /// bit0 = 周日 … bit6 = 周六。
    pub weekday_mask: u8,
    pub start: ClockTime,
    pub end: ClockTime,
}

impl WeeklyRange {
    /// 至少一天；`24:00` 只能作结束；开始和结束不能相同（整天写 `00:00`–`24:00`）。
    pub fn validate(&self) -> Result<()> {
        if self.weekday_mask & EVERY_DAY == 0 || self.weekday_mask & !EVERY_DAY != 0 {
            return Err(Error::invalid(
                "weekday_mask names the days with bits 0 (Sunday) to 6 (Saturday), at least one",
            ));
        }

        if self.start == ClockTime::END_OF_DAY {
            return Err(Error::invalid("24:00 can only end a time range"));
        }

        if self.start == self.end {
            return Err(Error::invalid(
                "a time range cannot start and end at the same time; a whole day is 00:00 to 24:00",
            ));
        }

        Ok(())
    }

    fn crosses_midnight(&self) -> bool {
        self.end <= self.start
    }

    fn on_day(&self, weekday: u8) -> bool {
        self.weekday_mask & (1 << (weekday % 7)) != 0
    }

    /// 本地的星期几（0 = 周日）和分钟落在这一段里。
    pub fn contains_local(&self, weekday: u8, minute: u16) -> bool {
        if !self.crosses_midnight() {
            return self.on_day(weekday) && self.start.0 <= minute && minute < self.end.0;
        }

        (self.on_day(weekday) && minute >= self.start.0)
            || (self.on_day(weekday + 6) && minute < self.end.0)
    }
}

/// 一个时区里的若干段每周重复的时间。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeeklyWindows {
    /// IANA 时区名。
    pub timezone: String,
    pub ranges: Vec<WeeklyRange>,
    /// 按这份节假日日历算：节假日不算在这些时段里，或只在节假日才算。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calendar: Option<CalendarUse>,
}

impl WeeklyWindows {
    /// 时区认得出，至少一段、最多 [`MAX_WEEKLY_RANGES`] 段，每段都说得通。
    pub fn validate(&self) -> Result<()> {
        parse_timezone(&self.timezone)?;

        if self.ranges.is_empty() {
            return Err(Error::invalid("a time window needs at least one range"));
        }

        if let Some(calendar) = &self.calendar {
            calendar.validate()?;
        }

        validate_ranges(&self.ranges)
    }

    /// 能在 `org_id` 里用：说得通，引用的日历是这个组织的。
    pub fn check(&self, org_id: &crate::shared::ids::Id) -> Result<()> {
        self.validate()?;

        match &self.calendar {
            Some(calendar) => calendar.check(org_id),
            None => Ok(()),
        }
    }

    pub fn contains(&self, at: TimestampMicros) -> bool {
        windows_contain(
            &self.ranges,
            timezone_or_utc(&self.timezone),
            self.calendar.as_ref(),
            at,
        )
    }

    /// `(from, to)` 里各段开始、结束的时刻，升序、不重复。
    pub fn transitions(&self, from: TimestampMicros, to: TimestampMicros) -> Vec<i64> {
        windows_transitions(
            &self.ranges,
            timezone_or_utc(&self.timezone),
            self.calendar.as_ref(),
            from.0,
            to.0,
        )
    }
}

/// 时间条件：在这些时段里，或不在这些时段里。路由规则和告警流水线按它挑事件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeCondition {
    pub mode: TimeConditionMode,
    #[serde(flatten)]
    pub windows: WeeklyWindows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeConditionMode {
    Inside,
    Outside,
}

impl TimeCondition {
    pub fn validate(&self) -> Result<()> {
        self.windows.validate()
    }

    /// 能在 `org_id` 里用：说得通，引用的节假日日历是这个组织的。
    pub fn check(&self, org_id: &crate::shared::ids::Id) -> Result<()> {
        self.windows.check(org_id)
    }

    /// `at` 这一刻满足条件。
    pub fn holds(&self, at: TimestampMicros) -> bool {
        self.windows.contains(at) == (self.mode == TimeConditionMode::Inside)
    }
}

/// 最多 [`MAX_WEEKLY_RANGES`] 段，每段都说得通；可以一段都没有。
pub fn validate_ranges(ranges: &[WeeklyRange]) -> Result<()> {
    if ranges.len() > MAX_WEEKLY_RANGES {
        return Err(Error::invalid(format!(
            "at most {MAX_WEEKLY_RANGES} time ranges"
        )));
    }

    ranges.iter().try_for_each(WeeklyRange::validate)
}

/// 读 IANA 时区名。
pub fn parse_timezone(name: &str) -> Result<chrono_tz::Tz> {
    name.trim()
        .parse::<chrono_tz::Tz>()
        .map_err(|_| Error::invalid(format!("`{name}` is not an IANA timezone")))
}

/// 读 IANA 时区名，认不出（或空着）时按 UTC。
pub fn timezone_or_utc(name: &str) -> chrono_tz::Tz {
    parse_timezone(name).unwrap_or(chrono_tz::UTC)
}

/// `at` 在 `tz` 的本地时间落在任何一段里。
pub fn ranges_contain(ranges: &[WeeklyRange], tz: chrono_tz::Tz, at: TimestampMicros) -> bool {
    let local = at.to_datetime().with_timezone(&tz);
    let weekday = local.weekday().num_days_from_sunday() as u8;
    let minute = (local.hour() * 60 + local.minute()) as u16;

    ranges
        .iter()
        .any(|range| range.contains_local(weekday, minute))
}

/// `(from, to)` 里各段在 `tz` 本地开始、结束的时刻（微秒），升序、不重复。
pub fn range_transitions(
    ranges: &[WeeklyRange],
    tz: chrono_tz::Tz,
    from: i64,
    to: i64,
) -> Vec<i64> {
    if ranges.is_empty() || to <= from {
        return Vec::new();
    }

    let first = local_date(tz, from) - Duration::days(1);
    let last = local_date(tz, to) + Duration::days(1);
    let mut points = Vec::new();

    for date in first.iter_days().take_while(|date| *date <= last) {
        let weekday = date.weekday().num_days_from_sunday() as u8;

        for range in ranges.iter().filter(|range| range.on_day(weekday)) {
            let end_date = if range.crosses_midnight() {
                date + Duration::days(1)
            } else {
                date
            };

            for local in [range.start.on(date), range.end.on(end_date)] {
                let at = local_instant(tz, local).timestamp_micros();

                if at > from && at < to {
                    points.push(at);
                }
            }
        }
    }

    points.sort_unstable();
    points.dedup();
    points
}

/// 本地时间换成时刻：夏令时跳过的那一段里取跳过之后的第一刻，重复的那一段里取前一个。
pub fn local_instant(tz: chrono_tz::Tz, local: NaiveDateTime) -> DateTime<Utc> {
    match tz.from_local_datetime(&local) {
        LocalResult::Single(at) => at.with_timezone(&Utc),
        LocalResult::Ambiguous(earliest, _) => earliest.with_timezone(&Utc),
        LocalResult::None => {
            let mut probe = local;

            for _ in 0..MINUTES_PER_DAY {
                probe += Duration::minutes(1);

                if let Some(at) = tz.from_local_datetime(&probe).earliest() {
                    return at.with_timezone(&Utc);
                }
            }

            Utc.from_utc_datetime(&local)
        }
    }
}

/// `at`（微秒）在 `tz` 里是哪一天。
fn local_date(tz: chrono_tz::Tz, at: i64) -> NaiveDate {
    TimestampMicros(at)
        .to_datetime()
        .with_timezone(&tz)
        .date_naive()
}

#[cfg(test)]
mod tests;
