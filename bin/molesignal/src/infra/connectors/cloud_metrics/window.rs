// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Which time range a metric pull asks a cloud for.
//!
//! A cloud's aggregate for a period is complete only some minutes after the period ends, and the
//! store keeps every row it is given, so a pull asks for whole periods that ended at least
//! `delay` ago, starting where the last written pull ended. Ranges never overlap: a value the
//! cloud corrects after it was read stays as read.
//!
//! The cursor is kept in the connector's read state as `{"end": <unix seconds>}`. A new connector
//! starts one period back. One pull covers at most [`MAX_PERIODS_PER_PULL`] periods and says there
//! is more; a connector that was off longer than [`MAX_CATCH_UP_SECS`] skips what is older.

use kernel::{Error, Result};
use serde_json::{Value, json};

use super::super::config::text;

/// How many periods one pull asks for at most.
pub const MAX_PERIODS_PER_PULL: i64 = 12;

/// How far back a pull catches up after a connector was off.
pub const MAX_CATCH_UP_SECS: i64 = 6 * 3600;

/// The periods a connector reads and how long it waits for them to complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    pub period_secs: i64,
    pub delay_secs: i64,
}

impl Schedule {
    /// `period` and `delay` of `config`, in seconds, with the defaults of the kind.
    pub fn parse(config: &Value, default_period: i64, default_delay: i64) -> Result<Self> {
        let seconds = |key: &str, default: i64| -> Result<i64> {
            match config.get(key) {
                None | Some(Value::Null) => Ok(default),
                Some(Value::Number(number)) => number
                    .as_i64()
                    .ok_or_else(|| Error::invalid(format!("`{key}` must be whole seconds"))),
                Some(Value::String(_)) if text(config, key).is_empty() => Ok(default),
                Some(Value::String(_)) => text(config, key)
                    .parse::<i64>()
                    .map_err(|_| Error::invalid(format!("`{key}` must be whole seconds"))),
                Some(_) => Err(Error::invalid(format!("`{key}` must be whole seconds"))),
            }
        };

        let schedule = Self {
            period_secs: seconds("period", default_period)?,
            delay_secs: seconds("delay", default_delay)?,
        };

        if schedule.period_secs < 60 || schedule.period_secs % 60 != 0 {
            return Err(Error::invalid(
                "`period` must be a whole number of minutes, at least 60 seconds",
            ));
        }

        if !(0..=86_400).contains(&schedule.delay_secs) {
            return Err(Error::invalid(
                "`delay` must be between 0 and 86400 seconds",
            ));
        }

        Ok(schedule)
    }
}

/// A range of whole periods, `[start, end)` in unix seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub start: i64,
    pub end: i64,
    /// Complete periods are waiting after this one.
    pub more: bool,
    /// Periods older than [`MAX_CATCH_UP_SECS`] were skipped to reach this one.
    pub skipped: bool,
}

impl Window {
    /// The next range to read after `position` at `now_secs`, or `None` while no new period has
    /// completed.
    pub fn next(schedule: Schedule, position: &Value, now_secs: i64) -> Option<Self> {
        let period = schedule.period_secs;
        let ready = (now_secs - schedule.delay_secs).div_euclid(period) * period;
        let saved = position.get("end").and_then(Value::as_i64);
        let mut start = saved.unwrap_or(ready - period);
        let oldest = ready - MAX_CATCH_UP_SECS.max(period);
        let skipped = start < oldest;

        if skipped {
            start = oldest.div_euclid(period) * period;
        }

        if start >= ready {
            return None;
        }

        let end = ready.min(start + MAX_PERIODS_PER_PULL * period);

        Some(Self {
            start,
            end,
            more: end < ready,
            skipped,
        })
    }

    /// The read state to keep once this range is written.
    pub fn position(&self) -> Value {
        json!({ "end": self.end })
    }

    /// The periods this range holds, each `[start, start + period)`.
    pub fn periods(&self, schedule: Schedule) -> impl Iterator<Item = i64> {
        (self.start..self.end).step_by(schedule.period_secs as usize)
    }

    /// What a pull reports when it skipped old periods.
    pub fn warning(&self) -> Option<String> {
        self.skipped.then(|| {
            format!(
                "the connector was behind by more than {} hours: only the last {} hours were read",
                MAX_CATCH_UP_SECS / 3600,
                MAX_CATCH_UP_SECS / 3600
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIVE_MINUTES: Schedule = Schedule {
        period_secs: 300,
        delay_secs: 600,
    };

    #[test]
    fn a_new_connector_reads_the_last_complete_period() {
        // 10:20:30 - 10 min = 10:10:30 → the last complete period ends at 10:10.
        let now = 1_700_000_000 - 1_700_000_000 % 300 + 30;
        let window = Window::next(FIVE_MINUTES, &Value::Null, now + 600).unwrap();

        assert_eq!(window.end, now - 30);
        assert_eq!(window.start, window.end - 300);
        assert!(!window.more);
        assert_eq!(window.position(), json!({ "end": window.end }));
    }

    #[test]
    fn the_next_pull_starts_where_the_last_ended_and_waits_for_a_whole_period() {
        let end = 1_700_000_100;
        let position = json!({ "end": end });

        assert_eq!(Window::next(FIVE_MINUTES, &position, end + 600 + 299), None);

        let window = Window::next(FIVE_MINUTES, &position, end + 600 + 300).unwrap();

        assert_eq!((window.start, window.end), (end, end + 300));
    }

    #[test]
    fn a_long_gap_is_read_a_few_periods_at_a_time_then_cut_short() {
        let end = 1_700_000_100;
        let now = end + 600 + 3600;
        let window = Window::next(FIVE_MINUTES, &json!({ "end": end }), now).unwrap();

        assert_eq!(window.end - window.start, MAX_PERIODS_PER_PULL * 300);
        assert!(!window.more);

        let behind = Window::next(FIVE_MINUTES, &json!({ "end": end }), now + 86_400).unwrap();

        assert!(behind.skipped && behind.more);
        assert!(behind.warning().is_some());
    }

    #[test]
    fn the_schedule_takes_whole_minutes() {
        assert_eq!(Schedule::parse(&json!({}), 300, 600).unwrap(), FIVE_MINUTES);
        assert_eq!(
            Schedule::parse(&json!({"period": "60", "delay": 180}), 300, 600).unwrap(),
            Schedule {
                period_secs: 60,
                delay_secs: 180
            }
        );
        assert!(Schedule::parse(&json!({"period": 90}), 300, 600).is_err());
        assert!(Schedule::parse(&json!({"period": "soon"}), 300, 600).is_err());
        assert!(Schedule::parse(&json!({"delay": -1}), 300, 600).is_err());
    }

    #[test]
    fn a_range_lists_its_periods() {
        let window = Window {
            start: 0,
            end: 900,
            more: false,
            skipped: false,
        };

        assert_eq!(
            window.periods(FIVE_MINUTES).collect::<Vec<_>>(),
            [0, 300, 600]
        );
    }
}
