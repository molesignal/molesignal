// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use chrono::{NaiveTime, Timelike};
use serde::Deserialize;
use serde_json::Value;

use crate::shared::{Error, Result, time::TimestampMicros};

#[derive(Debug, Deserialize)]
struct QuietHoursConfig {
    #[serde(default)]
    enabled: bool,
    timezone: String,
    start: String,
    end: String,
}

fn parse(value: &Value) -> Result<(QuietHoursConfig, chrono_tz::Tz, NaiveTime, NaiveTime)> {
    let config: QuietHoursConfig = serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("invalid quiet_hours: {error}")))?;
    let timezone = config
        .timezone
        .parse::<chrono_tz::Tz>()
        .map_err(|_| Error::invalid("quiet_hours timezone must be a valid IANA timezone"))?;
    let start = NaiveTime::parse_from_str(&config.start, "%H:%M")
        .map_err(|_| Error::invalid("quiet_hours start must use HH:MM"))?;
    let end = NaiveTime::parse_from_str(&config.end, "%H:%M")
        .map_err(|_| Error::invalid("quiet_hours end must use HH:MM"))?;
    Ok((config, timezone, start, end))
}

pub fn validate_quiet_hours(value: &Value) -> Result<()> {
    parse(value).map(|_| ())
}

pub fn quiet_hours_active(value: &Value, at: TimestampMicros) -> bool {
    let Ok((config, timezone, start, end)) = parse(value) else {
        return false;
    };
    if !config.enabled {
        return false;
    }
    let local = at.to_datetime().with_timezone(&timezone);
    let Some(now) = NaiveTime::from_hms_opt(local.hour(), local.minute(), 0) else {
        return false;
    };
    if start == end {
        return true;
    }
    if start < end {
        start <= now && now < end
    } else {
        now >= start || now < end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(value: &str) -> TimestampMicros {
        TimestampMicros(
            chrono::DateTime::parse_from_rfc3339(value)
                .unwrap()
                .timestamp_micros(),
        )
    }

    #[test]
    fn overnight_window_uses_configured_timezone() {
        let config = serde_json::json!({
            "enabled": true,
            "timezone": "Asia/Shanghai",
            "start": "22:00",
            "end": "08:00"
        });
        assert!(quiet_hours_active(&config, ts("2026-07-29T15:00:00Z")));
        assert!(!quiet_hours_active(&config, ts("2026-07-29T04:00:00Z")));
    }

    #[test]
    fn rejects_unknown_timezone() {
        assert!(
            validate_quiet_hours(&serde_json::json!({
                "enabled": true,
                "timezone": "Mars/Olympus",
                "start": "22:00",
                "end": "08:00"
            }))
            .is_err()
        );
    }
}
