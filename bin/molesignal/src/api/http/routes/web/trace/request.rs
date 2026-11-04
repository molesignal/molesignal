// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/api/v1/web/trace/{trace_id}` 请求结构（web-investigation-shell）。
//!
//! 路径里的 trace_id，和 `?from=&to=` 给出的查找窗口（不给时查最近 24h，没有再按摘要行在最长的
//! 查找窗口里找）。

use serde::Deserialize;

use crate::shared::{
    Error,
    time::{TimeRange, TimestampMicros},
};

/// 默认查询窗口：过去 24h（trace_id 是稀疏键，窗口仅做分区裁剪）。
const DEFAULT_WINDOW_SECS: i64 = 24 * 3600;

/// The longest window a trace is looked for in, when the reader says where it is.
const MAX_WINDOW_SECS: i64 = 31 * 24 * 3600;

/// Where a trace is looked for: the moments, in epoch microseconds, its Spans
/// began in. A link to a trace knows them; without them the last day is read.
#[derive(Debug, Default, Deserialize)]
pub struct TraceWindow {
    from: Option<i64>,
    to: Option<i64>,
}

/// 查找一条不知道时间的 Trace 时最远看到哪里：最长的查找窗口。
pub fn longest_range(now: TimestampMicros) -> TimeRange {
    TimeRange::new(TimestampMicros(now.0 - MAX_WINDOW_SECS * 1_000_000), now)
}

impl TraceWindow {
    /// 请求给出了查找窗口。
    pub fn given(&self) -> bool {
        self.from.is_some() || self.to.is_some()
    }

    /// The range to read: the window asked for, or the last day before `now`.
    pub fn range(&self, now: TimestampMicros) -> Result<TimeRange, Error> {
        match (self.from, self.to) {
            (Some(from), Some(to)) => {
                if to <= from {
                    return Err(Error::invalid("to must be greater than from"));
                }

                if to
                    .checked_sub(from)
                    .is_none_or(|span| span > MAX_WINDOW_SECS * 1_000_000)
                {
                    return Err(Error::invalid("a trace is looked for in 31 days at most"));
                }

                Ok(TimeRange::new(TimestampMicros(from), TimestampMicros(to)))
            }
            (None, None) => Ok(TimeRange::new(
                TimestampMicros(now.0 - DEFAULT_WINDOW_SECS * 1_000_000),
                now,
            )),
            _ => Err(Error::invalid("from and to are given together")),
        }
    }
}

/// trace_id 校验：长度 1..=128，仅 `[A-Za-z0-9_-]`。
pub fn validate_trace_id(trace_id: &str) -> Result<(), &'static str> {
    if trace_id.is_empty() || trace_id.len() > 128 {
        return Err("trace_id length must be 1..=128");
    }

    if !trace_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("trace_id contains invalid characters");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_quotes_and_semicolons() {
        assert!(validate_trace_id("abc' OR '1'='1").is_err());
        assert!(validate_trace_id("bad;id").is_err());
    }

    #[test]
    fn accepts_hex_and_base32_friendly() {
        assert!(validate_trace_id("abc-123_DEF").is_ok());
        assert!(validate_trace_id("0123456789abcdef0123456789abcdef").is_ok());
    }

    #[test]
    fn enforces_length_bounds() {
        assert!(validate_trace_id("").is_err());
        assert!(validate_trace_id(&"x".repeat(129)).is_err());
    }

    #[test]
    fn a_window_is_the_one_asked_for_or_the_last_day() {
        let now = TimestampMicros(100 * 24 * 3600 * 1_000_000);
        let asked = TraceWindow {
            from: Some(10),
            to: Some(20),
        };
        let range = asked.range(now).unwrap();
        assert_eq!((range.start.0, range.end.0), (10, 20));

        let last_day = TraceWindow::default().range(now).unwrap();
        assert_eq!(last_day.end, now);
        assert_eq!(now.0 - last_day.start.0, DEFAULT_WINDOW_SECS * 1_000_000);

        for refused in [
            (Some(20), Some(10)),
            (Some(10), None),
            (Some(0), Some(i64::MAX)),
        ] {
            let window = TraceWindow {
                from: refused.0,
                to: refused.1,
            };
            assert!(window.range(now).is_err(), "{refused:?}");
        }
    }
}
