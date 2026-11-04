// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 事件的重复规则（RFC 5545）写成轮值：一周里生效的时段，加上一人一班多长。
//!
//! - 第一班的起止加 `RRULE` 给出每一班。按本地时间重复，所以交接时刻全年不变。
//! - `shifts_per_member` 数的是重复的次数，从 `start_time` 数起，`BYDAY` 的每一天各算一次：OpenAPI 要求
//!   `FREQ=WEEKLY` 时它能被 `BYDAY` 的天数整除，否则网页编辑器读不了。`BYDAY=MO,TU,WE,TH,FR` 配 5 是每人一周；
//!   配 1 是每人一天，周五的下一班隔着周末，交接不在固定的周期上，只有一个人时才无所谓。
//! - 只写得出落在一周固定时段上的规则，以及一班接一班、一直有人值班的规则。`EXDATE`、`RDATE` 增减某一次，轮值
//!   表达不了，文档也没说它们算不算进 `shifts_per_member`。
//!
//! 文档没说跨夏令时切换时一班有多长。官方样例的七天一班就跨了切换，实际经过的时间正好七天，本地时间差一小时，
//! 所以两种算法有一种等于间隔就算一班接一班；如果 PagerDuty 的交接不在本地时间，预演的对照会发现。

use chrono::{Datelike, NaiveDateTime, Timelike};
use chrono_tz::Tz;

use super::super::super::weekly::WeekMinutes;
use crate::{
    app::alerting::oncall_transfer::Finding, domain::alerting::time_window::MINUTES_PER_DAY,
    shared::time::TimestampMicros,
};

const HOUR_SECS: i64 = 3_600;
const DAY_SECS: i64 = 24 * HOUR_SECS;
const WEEK_SECS: i64 = 7 * DAY_SECS;
const DAY_MINUTES: usize = MINUTES_PER_DAY as usize;

/// 事件写成轮值后的样子。
pub(super) struct Cadence {
    /// 一周里生效的分钟。
    pub minutes: WeekMinutes,
    /// 一人一班多长（秒）。
    pub turn_secs: u64,
}

/// 第一班的起止，和算本地时间用的时区。
pub(super) struct FirstShift {
    pub start: i64,
    pub end: i64,
    pub tz: Tz,
}

impl FirstShift {
    fn local(&self, at: i64) -> NaiveDateTime {
        TimestampMicros(at)
            .to_datetime()
            .with_timezone(&self.tz)
            .naive_local()
    }

    /// 按本地时间有多长（秒）。
    fn wall_secs(&self) -> i64 {
        (self.local(self.end) - self.local(self.start)).num_seconds()
    }

    /// 实际经过多长（秒）。
    fn elapsed_secs(&self) -> i64 {
        (self.end - self.start) / 1_000_000
    }

    /// 从周日数起第几天。
    fn weekday(&self) -> usize {
        self.local(self.start).weekday().num_days_from_sunday() as usize
    }

    /// 当天第几分钟开始、有多少分钟；开始和长度不是整分时写不成时段。
    fn whole_minutes(&self) -> Option<(usize, usize)> {
        let start = self.local(self.start);
        let wall = self.wall_secs();

        (start.second() == 0 && start.nanosecond() == 0 && wall > 0 && wall % 60 == 0).then(|| {
            (
                (start.hour() * 60 + start.minute()) as usize,
                (wall / 60) as usize,
            )
        })
    }
}

/// `shifts` 是每人连值几班，`several` 是事件不止一个成员：只有一个人时交接没有意义。写不成轮值时说明原因。
pub(super) fn cadence_of(
    recurrence: &[String],
    first: &FirstShift,
    shifts: u64,
    several: bool,
) -> Result<Cadence, Finding> {
    let unsupported = || Finding::new("recurrence_unsupported").with("value", recurrence.join(" "));

    match repeat_of(recurrence).ok_or_else(unsupported)? {
        Repeat::Every(spacing) => evenly(spacing, first, shifts, several).ok_or_else(unsupported),
        // 一周一天就是每周一次。
        Repeat::OnDays(days) if days == [first.weekday()] => {
            evenly(WEEK_SECS, first, shifts, several).ok_or_else(unsupported)
        }
        Repeat::OnDays(days) => {
            let minutes = on_days(&days, first, several).ok_or_else(unsupported)?;
            let per_week = days.len() as u64;

            if several && !shifts.is_multiple_of(per_week) {
                return Err(Finding::new("shifts_per_member_unsupported")
                    .with("value", shifts.to_string())
                    .with("per_week", per_week.to_string()));
            }

            let weeks = if several { shifts / per_week } else { 1 };
            let turn_secs = turn_of(WEEK_SECS, weeks).ok_or_else(unsupported)?;

            Ok(Cadence { minutes, turn_secs })
        }
    }
}

enum Repeat {
    /// 等间隔重复，间隔多少秒。
    Every(i64),
    /// 每周这几天各一次，从周日数起。
    OnDays(Vec<usize>),
}

/// 读那一条 `RRULE`。带 `EXDATE`、`RDATE`，或者有这里不认识的部分（`COUNT`、`UNTIL`、`BYMONTHDAY`……）时为空。
fn repeat_of(recurrence: &[String]) -> Option<Repeat> {
    let [line] = recurrence else {
        return None;
    };
    let line = line.trim().to_ascii_uppercase();
    let rule = line.strip_prefix("RRULE:")?;

    let mut unit = None;
    let mut interval = 1;
    let mut days = Vec::new();

    for part in rule.split(';') {
        match part.split_once('=')? {
            ("FREQ", "HOURLY") => unit = Some(HOUR_SECS),
            ("FREQ", "DAILY") => unit = Some(DAY_SECS),
            ("FREQ", "WEEKLY") => unit = Some(WEEK_SECS),
            ("INTERVAL", value) => interval = value.parse::<u32>().ok().filter(|n| *n > 0)?,
            ("BYDAY", value) => {
                days = value.split(',').map(weekday).collect::<Option<Vec<_>>>()?;
            }
            // 一周从哪天算只影响隔周的 `BYDAY`，而那种规则写不成轮值。
            ("WKST", _) => {}
            _ => return None,
        }
    }

    let unit = unit?;

    days.sort_unstable();
    days.dedup();

    match (days.len(), unit, interval) {
        (0, ..) => Some(Repeat::Every(unit * i64::from(interval))),
        (7, DAY_SECS | WEEK_SECS, 1) => Some(Repeat::Every(DAY_SECS)),
        (_, DAY_SECS | WEEK_SECS, 1) => Some(Repeat::OnDays(days)),
        _ => None,
    }
}

/// 周日是 0；`1MO` 这种第几个周几的写法写不成每周时段。
fn weekday(code: &str) -> Option<usize> {
    ["SU", "MO", "TU", "WE", "TH", "FR", "SA"]
        .iter()
        .position(|day| *day == code)
}

fn turn_of(spacing: i64, count: u64) -> Option<u64> {
    u64::try_from(spacing).ok()?.checked_mul(count)
}

fn evenly(spacing: i64, first: &FirstShift, shifts: u64, several: bool) -> Option<Cadence> {
    let wall = first.wall_secs();
    let turn_secs = turn_of(spacing, shifts)?;

    // 一班接一班，一直有人值班。只有一个人时，比间隔还长也一样。
    if wall == spacing || first.elapsed_secs() == spacing || (wall > spacing && !several) {
        return Some(Cadence {
            minutes: WeekMinutes::full(),
            turn_secs,
        });
    }

    // 比间隔还长时前后两个人同时值班；间隔不是一天或一周时，有人值班的时段不在一周的固定位置上。
    if wall > spacing || (spacing != DAY_SECS && spacing != WEEK_SECS) {
        return None;
    }

    let (start, length) = first.whole_minutes()?;
    let mut minutes = WeekMinutes::empty();

    if spacing == DAY_SECS {
        minutes.add_daily(start, length);
    } else {
        minutes.add(first.weekday() * DAY_MINUTES + start, length);
    }

    Some(Cadence { minutes, turn_secs })
}

fn on_days(days: &[usize], first: &FirstShift, several: bool) -> Option<WeekMinutes> {
    // 第一班不在 `BYDAY` 的那几天里时，RFC 5545 也没说规则怎么算。
    if !days.contains(&first.weekday()) {
        return None;
    }

    let (start, length) = first.whole_minutes()?;
    let mut minutes = WeekMinutes::empty();

    for day in days {
        let mut shift = WeekMinutes::empty();

        shift.add(day * DAY_MINUTES + start, length);

        // 前一班没结束下一班就开始了：交接时两个人同时值班。
        if several && minutes.intersects(&shift) {
            return None;
        }

        minutes.add(day * DAY_MINUTES + start, length);
    }

    Some(minutes)
}
