// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 定时巡检的周期：保存时的校验，以及下一次什么时候执行。

use std::str::FromStr;

use chrono::{DateTime, Datelike, Days, LocalResult, NaiveDate, TimeDelta, TimeZone, Utc};
use chrono_tz::Tz;

use crate::{
    domain::log_inspection::InspectionSchedule,
    shared::{Error, Result, time::TimestampMicros},
};

/// 间隔周期最长一周；再长的用每周的固定时刻。
const MAX_INTERVAL_MINUTES: u32 = 7 * 24 * 60;

pub fn parse_timezone(name: &str) -> Result<Tz> {
    Tz::from_str(name.trim()).map_err(|_| Error::invalid("timezone must be an IANA time zone name"))
}

/// 周期本身说得通，并且不比实例允许的最短间隔更密。
pub fn validate(schedule: &InspectionSchedule, min_interval_secs: u64) -> Result<()> {
    match schedule {
        InspectionSchedule::Interval { every_minutes } => {
            if *every_minutes == 0 || *every_minutes > MAX_INTERVAL_MINUTES {
                return Err(Error::invalid(format!(
                    "schedule.every_minutes must be between 1 and {MAX_INTERVAL_MINUTES}"
                )));
            }
        }
        InspectionSchedule::Daily { hour, minute } => validate_time(*hour, *minute)?,
        InspectionSchedule::Weekly {
            weekday,
            hour,
            minute,
        } => {
            if !(1..=7).contains(weekday) {
                return Err(Error::invalid(
                    "schedule.weekday must be between 1 (Monday) and 7 (Sunday)",
                ));
            }

            validate_time(*hour, *minute)?;
        }
    }

    if schedule.period_secs() < min_interval_secs {
        return Err(Error::invalid(format!(
            "this instance runs a log inspection at most once every {min_interval_secs} seconds"
        )));
    }

    Ok(())
}

fn validate_time(hour: u8, minute: u8) -> Result<()> {
    if hour > 23 || minute > 59 {
        return Err(Error::invalid(
            "schedule.hour must be 0-23 and schedule.minute 0-59",
        ));
    }

    Ok(())
}

/// `after` 之后的下一次执行时刻。带时刻的周期按 `timezone` 解释。
pub fn next_run_after(
    schedule: &InspectionSchedule,
    timezone: Tz,
    after: TimestampMicros,
) -> TimestampMicros {
    match schedule {
        InspectionSchedule::Interval { every_minutes } => TimestampMicros(
            after
                .0
                .saturating_add(i64::from(*every_minutes) * 60 * 1_000_000),
        ),
        InspectionSchedule::Daily { hour, minute } => {
            next_local_time(timezone, after, *hour, *minute, |_| true)
        }
        InspectionSchedule::Weekly {
            weekday,
            hour,
            minute,
        } => next_local_time(timezone, after, *hour, *minute, |date| {
            date.weekday().number_from_monday() == u32::from(*weekday)
        }),
    }
}

/// 从 `after` 当地的这一天起逐天找：第一个 `on_day` 认可、且那一刻晚于 `after` 的日子。
fn next_local_time(
    timezone: Tz,
    after: TimestampMicros,
    hour: u8,
    minute: u8,
    on_day: impl Fn(NaiveDate) -> bool,
) -> TimestampMicros {
    let after = after.to_datetime();
    let today = after.with_timezone(&timezone).date_naive();

    // 每周的周期最远在七天后；多看两天，夏令时把那一刻推到次日也找得到。
    for offset in 0..=9 {
        let Some(date) = today.checked_add_days(Days::new(offset)) else {
            break;
        };

        if !on_day(date) {
            continue;
        }

        if let Some(instant) = local_instant(timezone, date, hour, minute)
            && instant > after
        {
            return TimestampMicros::from_datetime(instant);
        }
    }

    // 日历走到尽头才会到这里；一天后再算一次，不让巡检就此停摆。
    TimestampMicros::from_datetime(after + TimeDelta::days(1))
}

/// 当地这一天的这个时刻是哪一刻。拨慢时钟时出现两次的时刻取第一次；拨快时钟时被跳过的时刻
/// 顺延一小时，落在跳过之后。
fn local_instant(timezone: Tz, date: NaiveDate, hour: u8, minute: u8) -> Option<DateTime<Utc>> {
    let naive = date.and_hms_opt(u32::from(hour), u32::from(minute), 0)?;

    let local = match timezone.from_local_datetime(&naive) {
        LocalResult::Single(instant) => instant,
        LocalResult::Ambiguous(first, _) => first,
        LocalResult::None => timezone
            .from_local_datetime(&(naive + TimeDelta::hours(1)))
            .earliest()?,
    };

    Some(local.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDateTime;

    use super::*;

    fn at(utc: &str) -> TimestampMicros {
        let naive = NaiveDateTime::parse_from_str(utc, "%Y-%m-%d %H:%M").unwrap();
        TimestampMicros::from_datetime(naive.and_utc())
    }

    fn shanghai() -> Tz {
        parse_timezone("Asia/Shanghai").unwrap()
    }

    #[test]
    fn an_interval_runs_that_long_after_the_last_run() {
        let schedule = InspectionSchedule::Interval { every_minutes: 90 };

        assert_eq!(
            next_run_after(&schedule, shanghai(), at("2026-10-13 08:00")),
            at("2026-10-13 09:30")
        );
    }

    #[test]
    fn a_daily_time_is_read_in_the_time_zone_of_the_inspection() {
        // 上海 09:30 是 UTC 01:30。
        let schedule = InspectionSchedule::Daily {
            hour: 9,
            minute: 30,
        };

        assert_eq!(
            next_run_after(&schedule, shanghai(), at("2026-10-13 00:00")),
            at("2026-10-13 01:30")
        );

        // 今天的那一刻已经过了，就是明天。
        assert_eq!(
            next_run_after(&schedule, shanghai(), at("2026-10-13 01:30")),
            at("2026-10-14 01:30")
        );
    }

    #[test]
    fn a_weekly_time_waits_for_its_weekday() {
        // 2026-10-13 是周二；周一 09:00（上海）是 UTC 周一 01:00。
        let schedule = InspectionSchedule::Weekly {
            weekday: 1,
            hour: 9,
            minute: 0,
        };

        assert_eq!(
            next_run_after(&schedule, shanghai(), at("2026-10-13 03:00")),
            at("2026-10-19 01:00")
        );

        // 刚好在那一刻执行完，下一次是整整一周后。
        assert_eq!(
            next_run_after(&schedule, shanghai(), at("2026-10-19 01:00")),
            at("2026-10-26 01:00")
        );
    }

    #[test]
    fn a_time_skipped_by_daylight_saving_runs_after_the_gap() {
        // 纽约 2026-03-08 02:00 拨到 03:00，02:30 这一刻不存在：顺延到 03:30（UTC 07:30）。
        let new_york = parse_timezone("America/New_York").unwrap();

        let schedule = InspectionSchedule::Daily {
            hour: 2,
            minute: 30,
        };

        assert_eq!(
            next_run_after(&schedule, new_york, at("2026-03-08 05:00")),
            at("2026-03-08 07:30")
        );
    }

    #[test]
    fn a_schedule_shorter_than_the_instance_allows_is_refused() {
        let hourly = InspectionSchedule::Interval { every_minutes: 60 };
        let half_hourly = InspectionSchedule::Interval { every_minutes: 30 };

        assert!(validate(&hourly, 3600).is_ok());
        assert!(validate(&half_hourly, 3600).is_err());
        assert!(validate(&half_hourly, 900).is_ok());

        // 一天一次的周期，在只许一周一次的实例上同样过不了。
        let daily = InspectionSchedule::Daily { hour: 9, minute: 0 };
        assert!(validate(&daily, 3600).is_ok());
        assert!(validate(&daily, 7 * 24 * 3600).is_err());
    }

    #[test]
    fn times_and_weekdays_out_of_range_are_refused() {
        for schedule in [
            InspectionSchedule::Interval { every_minutes: 0 },
            InspectionSchedule::Interval {
                every_minutes: MAX_INTERVAL_MINUTES + 1,
            },
            InspectionSchedule::Daily {
                hour: 24,
                minute: 0,
            },
            InspectionSchedule::Daily {
                hour: 9,
                minute: 60,
            },
            InspectionSchedule::Weekly {
                weekday: 0,
                hour: 9,
                minute: 0,
            },
            InspectionSchedule::Weekly {
                weekday: 8,
                hour: 9,
                minute: 0,
            },
        ] {
            assert!(validate(&schedule, 60).is_err(), "{schedule:?}");
        }

        assert!(parse_timezone("Mars/Olympus").is_err());
    }
}
