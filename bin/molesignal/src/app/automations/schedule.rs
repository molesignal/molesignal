// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cron 触发：保存时的校验，以及下一次在什么时候运行。
//!
//! 写法是 crontab 里常见的五段：分、时、日、月、星期，按规则的时区算。星期照 crontab 的习惯，
//! 0 和 7 都是周日；`cron` crate 数星期从 1（周日）开始，这里换算过去。日和星期只能限定一个：
//! crontab 对两者取「或」，`cron` crate 取「且」，同时限定时两边的意思不一样。相邻两次运行至少
//! 隔 15 分钟。

use std::str::FromStr;

use chrono::Utc;
use chrono_tz::Tz;

use crate::shared::{Error, Result, time::TimestampMicros};

/// 相邻两次运行至少隔这么久。
pub const MIN_GAP_MINUTES: i64 = 15;
/// 校验间隔时往后看多少次运行。
const LOOKAHEAD: usize = 400;

fn invalid(message: impl Into<String>) -> Error {
    Error::invalid(message.into())
}

/// IANA 时区名，例如 `Asia/Shanghai`。
pub fn parse_timezone(name: &str) -> Result<Tz> {
    Tz::from_str(name.trim()).map_err(|_| invalid("timezone must be an IANA time zone name"))
}

/// crontab 的一个星期数（0–7，0 和 7 是周日）；名字（MON、TUE……）为空。
fn standard_day(token: &str) -> Result<Option<u8>> {
    let token = token.trim();

    if token.chars().all(|c| c.is_ascii_alphabetic()) {
        return Ok(None);
    }

    match token.parse::<u8>() {
        Ok(day) if day <= 7 => Ok(Some(day)),
        _ => Err(invalid(format!(
            "`{token}` is not a day of the week; days run from 0 to 7, where 0 and 7 are Sunday"
        ))),
    }
}

/// crontab 的星期数在 `cron` crate 里的数（1–7，1 是周日）。
fn crate_day(day: u8) -> u8 {
    if day == 0 || day == 7 { 1 } else { day + 1 }
}

/// 星期这一段换成 `cron` crate 的写法。名字两边一样，原样留下。
fn weekdays(field: &str) -> Result<String> {
    let mut items = Vec::new();

    for item in field.split(',') {
        let (base, step) = match item.split_once('/') {
            Some((base, step)) => (base, format!("/{step}")),
            None => (item, String::new()),
        };

        if base == "*" {
            items.push(format!("*{step}"));
            continue;
        }

        let Some((from, to)) = base.split_once('-') else {
            match standard_day(base)? {
                Some(day) => items.push(format!("{}{step}", crate_day(day))),
                None => items.push(format!("{base}{step}")),
            }
            continue;
        };

        match (standard_day(from)?, standard_day(to)?) {
            (None, None) => items.push(format!("{base}{step}")),
            (Some(0), Some(7)) if step.is_empty() => items.push("1-7".into()),
            // 「几到 7」：到周六为止，再加周日。
            (Some(start), Some(7)) if start > 0 && step.is_empty() => {
                items.push(format!("{}-7", start + 1));
                items.push("1".into());
            }
            (Some(start), Some(end)) if start <= end && end < 7 => {
                items.push(format!("{}-{}{step}", start + 1, end + 1));
            }
            _ => {
                return Err(invalid(format!(
                    "the day-of-week range `{base}` is not supported; list the days instead"
                )));
            }
        }
    }

    Ok(items.join(","))
}

/// 五段的 crontab 写法换成 `cron` crate 的七段（秒、分、时、日、月、星期、年）。
fn parse(expression: &str) -> Result<cron::Schedule> {
    let fields = expression.split_whitespace().collect::<Vec<_>>();

    let [minute, hour, day, month, week] = fields[..] else {
        return Err(invalid(
            "a cron expression has five fields: minute, hour, day of month, month and day of week",
        ));
    };

    let restricted = |field: &str| field != "*" && field != "?";

    if restricted(day) && restricted(week) {
        return Err(invalid(
            "limit either the day of the month or the day of the week, not both",
        ));
    }

    let expression = format!("0 {minute} {hour} {day} {month} {} *", weekdays(week)?);

    cron::Schedule::from_str(&expression)
        .map_err(|error| invalid(format!("invalid cron expression: {error}")))
}

/// 表达式与时区说得通，会运行，相邻两次至少隔 15 分钟。
pub fn validate(expression: &str, timezone: &str) -> Result<()> {
    let schedule = parse(expression)?;
    let timezone = parse_timezone(timezone)?;
    let mut upcoming = schedule.upcoming(timezone).take(LOOKAHEAD);

    let Some(mut previous) = upcoming.next() else {
        return Err(invalid("this cron expression never runs"));
    };

    for at in upcoming {
        if (at - previous).num_minutes() < MIN_GAP_MINUTES {
            return Err(invalid(format!(
                "a scheduled automation runs at most once every {MIN_GAP_MINUTES} minutes"
            )));
        }

        previous = at;
    }

    Ok(())
}

/// `after` 之后的下一次运行；表达式不再运行时为空。
pub fn next_after(
    expression: &str,
    timezone: &str,
    after: TimestampMicros,
) -> Result<Option<TimestampMicros>> {
    let schedule = parse(expression)?;
    let timezone: Tz = parse_timezone(timezone)?;
    let after = after.to_datetime().with_timezone(&timezone);

    Ok(schedule
        .after(&after)
        .next()
        .map(|at| TimestampMicros::from_datetime(at.with_timezone(&Utc))))
}

#[cfg(test)]
mod tests {
    use chrono::{Datelike, TimeZone, Timelike, Weekday};

    use super::*;

    fn micros(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> TimestampMicros {
        TimestampMicros::from_datetime(
            Utc.with_ymd_and_hms(year, month, day, hour, minute, 0)
                .unwrap(),
        )
    }

    #[test]
    fn weekdays_are_read_the_crontab_way() {
        assert_eq!(weekdays("1-5").unwrap(), "2-6");
        assert_eq!(weekdays("0,6").unwrap(), "1,7");
        assert_eq!(weekdays("7").unwrap(), "1");
        assert_eq!(weekdays("1-7").unwrap(), "2-7,1");
        assert_eq!(weekdays("0-6").unwrap(), "1-7");
        assert_eq!(weekdays("0-7").unwrap(), "1-7");
        assert_eq!(weekdays("1-5/2").unwrap(), "2-6/2");
        assert_eq!(weekdays("MON-FRI").unwrap(), "MON-FRI");
        assert_eq!(weekdays("*/2").unwrap(), "*/2");
        assert!(weekdays("8").is_err());
        assert!(weekdays("MON-5").is_err());
    }

    #[test]
    fn weekday_mornings_land_on_weekdays_in_the_rules_time_zone() {
        // 2026-10-17 is a Saturday. 09:00 in Shanghai is 01:00 UTC.
        let next = next_after("0 9 * * 1-5", "Asia/Shanghai", micros(2026, 10, 17, 12, 0))
            .unwrap()
            .unwrap()
            .to_datetime();

        assert_eq!(next.weekday(), Weekday::Mon);
        assert_eq!((next.hour(), next.minute()), (1, 0));
    }

    #[test]
    fn a_schedule_runs_at_most_every_fifteen_minutes() {
        assert!(validate("*/15 * * * *", "UTC").is_ok());
        assert!(validate("0 9 * * 1-5", "Europe/Berlin").is_ok());
        assert!(validate("*/5 * * * *", "UTC").is_err());
        assert!(validate("0,10 * * * *", "UTC").is_err());
        assert!(validate("0 9 * * *", "Mars/Olympus").is_err());
        assert!(validate("0 9 * *", "UTC").is_err());
        assert!(validate("0 9 1 * 1", "UTC").is_err());
    }
}
