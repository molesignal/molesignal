// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::str::FromStr;

use chrono::Utc;

use crate::{
    domain::synthetics::MonitorSchedule,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn next_due_at(
    monitor_id: &Id,
    schedule: &MonitorSchedule,
    after: TimestampMicros,
) -> Result<Option<TimestampMicros>> {
    match schedule {
        MonitorSchedule::Interval { every_seconds } => {
            let base = i64::from(*every_seconds).saturating_mul(1_000_000);
            let jitter_window = (base / 10).max(1);
            let hash = blake3::hash(monitor_id.as_str().as_bytes());
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&hash.as_bytes()[..8]);
            let jitter = (u64::from_le_bytes(bytes) % jitter_window as u64) as i64;
            Ok(Some(TimestampMicros(
                after.0.saturating_add(base).saturating_add(jitter),
            )))
        }
        MonitorSchedule::Cron {
            expression,
            timezone,
        } => {
            let timezone = chrono_tz::Tz::from_str(timezone)
                .map_err(|_| Error::invalid("invalid IANA timezone"))?;
            let schedule = cron::Schedule::from_str(expression)
                .map_err(|error| Error::invalid(format!("invalid Cron expression: {error}")))?;
            let after = after.to_datetime().with_timezone(&timezone);
            Ok(schedule
                .after(&after)
                .next()
                .map(|value| TimestampMicros::from_datetime(value.with_timezone(&Utc))))
        }
        MonitorSchedule::Heartbeat { .. } => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_jitter_is_stable_and_bounded() {
        let start = TimestampMicros(1_000_000);
        let schedule = MonitorSchedule::Interval { every_seconds: 60 };
        let first = next_due_at(&Id::from_string("monitor-a"), &schedule, start)
            .unwrap()
            .unwrap();
        let second = next_due_at(&Id::from_string("monitor-a"), &schedule, start)
            .unwrap()
            .unwrap();
        assert_eq!(first, second);
        assert!((61_000_000..67_000_000).contains(&first.0));
    }
}
