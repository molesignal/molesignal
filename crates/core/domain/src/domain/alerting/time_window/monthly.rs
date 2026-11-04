// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 每月重复一次的时间：每月第几天，或第几个 / 最后一个星期几，从某个本地时刻起持续一段时间（最长
//! 7 天）。用在静默的维护窗口上，例如「每月第一个周日 02:00 起 4 小时」。

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};

use super::{ClockTime, local_instant, parse_timezone, timezone_or_utc};
use crate::shared::{Error, Result, time::TimestampMicros};

/// 一次最长持续多久：7 天。
pub const MAX_MONTHLY_DURATION_MINUTES: u32 = 7 * 24 * 60;

/// 每月的哪一天。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MonthDay {
    /// 第几天（1–31）；这个月没有这一天时取最后一天。
    Date { day: u8 },
    /// 第几个星期几：`ordinal` 1–5，-1 是最后一个；`weekday` 0 = 周日 … 6 = 周六。没有第 5 个
    /// 这个星期几的月份这一次不算。
    Weekday { ordinal: i8, weekday: u8 },
}

impl MonthDay {
    fn validate(&self) -> Result<()> {
        match *self {
            Self::Date { day } if !(1..=31).contains(&day) => {
                Err(Error::invalid("a day of the month is 1 to 31"))
            }
            Self::Weekday { ordinal, .. } if ordinal != -1 && !(1..=5).contains(&ordinal) => Err(
                Error::invalid("the ordinal of a weekday is 1 to 5, or -1 for the last one"),
            ),
            Self::Weekday { weekday, .. } if weekday > 6 => {
                Err(Error::invalid("a weekday is 0 (Sunday) to 6 (Saturday)"))
            }
            _ => Ok(()),
        }
    }

    /// 这个月的这一天；没有就是空。
    pub(super) fn in_month(&self, year: i32, month: u32) -> Option<NaiveDate> {
        let first = NaiveDate::from_ymd_opt(year, month, 1)?;
        let last = last_day(first);

        match *self {
            Self::Date { day } => first.with_day(u32::from(day).min(last.day())),
            Self::Weekday { ordinal, weekday } => {
                let weekday = Weekday::try_from(weekday_from_sunday(weekday)).ok()?;

                if ordinal < 0 {
                    let back = (last.weekday().num_days_from_monday() + 7
                        - weekday.num_days_from_monday())
                        % 7;
                    return Some(last - Duration::days(i64::from(back)));
                }

                let ahead = (weekday.num_days_from_monday() + 7
                    - first.weekday().num_days_from_monday())
                    % 7;
                let date = first + Duration::days(i64::from(ahead) + 7 * i64::from(ordinal - 1));

                (date.month() == month).then_some(date)
            }
        }
    }
}

/// 每月一次、持续一段时间的窗口。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthlyWindow {
    /// IANA 时区名。
    pub timezone: String,
    pub day: MonthDay,
    pub start: ClockTime,
    /// 从开始起持续多少分钟，1 到 [`MAX_MONTHLY_DURATION_MINUTES`]。
    pub duration_minutes: u32,
}

impl MonthlyWindow {
    pub fn validate(&self) -> Result<()> {
        parse_timezone(&self.timezone)?;
        self.day.validate()?;

        if self.start == ClockTime::END_OF_DAY {
            return Err(Error::invalid("24:00 can only end a time range"));
        }

        if !(1..=MAX_MONTHLY_DURATION_MINUTES).contains(&self.duration_minutes) {
            return Err(Error::invalid(format!(
                "duration_minutes must be between 1 and {MAX_MONTHLY_DURATION_MINUTES}"
            )));
        }

        Ok(())
    }

    pub fn contains(&self, at: TimestampMicros) -> bool {
        self.occurrences_around(at.0, at.0)
            .any(|(start, end)| start <= at.0 && at.0 < end)
    }

    /// `(from, to)` 里每次开始、结束的时刻，升序、不重复。
    pub fn transitions(&self, from: TimestampMicros, to: TimestampMicros) -> Vec<i64> {
        let mut points: Vec<i64> = self
            .occurrences_around(from.0, to.0)
            .flat_map(|(start, end)| [start, end])
            .filter(|at| *at > from.0 && *at < to.0)
            .collect();

        points.sort_unstable();
        points.dedup();
        points
    }

    /// 和 `[from, to]` 沾边的各次（开始、结束，微秒）：从 `from` 前一个月数到 `to` 那个月。
    fn occurrences_around(&self, from: i64, to: i64) -> impl Iterator<Item = (i64, i64)> + '_ {
        let tz = timezone_or_utc(&self.timezone);
        let first = TimestampMicros(from).to_datetime().with_timezone(&tz);
        let last = TimestampMicros(to).to_datetime().with_timezone(&tz);
        let months =
            month_index(last.year(), last.month()) - month_index(first.year(), first.month());
        let duration = i64::from(self.duration_minutes) * 60_000_000;
        let start_month = month_index(first.year(), first.month()) - 1;

        (0..=months + 1).filter_map(move |offset| {
            let index = start_month + offset;
            let (year, month) = (index.div_euclid(12) as i32, index.rem_euclid(12) as u32 + 1);
            let date = self.day.in_month(year, month)?;
            let start = local_instant(tz, self.start.on(date)).timestamp_micros();

            Some((start, start.saturating_add(duration)))
        })
    }
}

fn month_index(year: i32, month: u32) -> i64 {
    i64::from(year) * 12 + i64::from(month) - 1
}

fn last_day(first: NaiveDate) -> NaiveDate {
    let (year, month) = if first.month() == 12 {
        (first.year() + 1, 1)
    } else {
        (first.year(), first.month() + 1)
    };

    NaiveDate::from_ymd_opt(year, month, 1).map_or(first, |next| next - Duration::days(1))
}

/// 0 = 周日 … 6 = 周六，换成 chrono 从周一数起的序号。
fn weekday_from_sunday(weekday: u8) -> u8 {
    (weekday + 6) % 7
}
