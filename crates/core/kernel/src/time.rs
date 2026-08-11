// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 微秒级时间戳。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TimestampMicros(pub i64);

impl TimestampMicros {
    pub fn now() -> Self {
        Self(Utc::now().timestamp_micros())
    }

    pub fn from_datetime(dt: DateTime<Utc>) -> Self {
        Self(dt.timestamp_micros())
    }

    pub fn from_secs(secs: i64) -> Self {
        Self(secs.saturating_mul(1_000_000))
    }

    pub fn from_millis(millis: i64) -> Self {
        Self(millis.saturating_mul(1_000))
    }

    pub fn to_datetime(self) -> DateTime<Utc> {
        DateTime::from_timestamp_micros(self.0).unwrap_or_default()
    }
}

/// 时间区间，闭区间 `[start, end]`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: TimestampMicros,
    pub end: TimestampMicros,
}

impl TimeRange {
    pub fn new(start: TimestampMicros, end: TimestampMicros) -> Self {
        Self { start, end }
    }

    /// 单点构造：把单个时间戳包成 [t, t]。
    pub fn at(t: TimestampMicros) -> Self {
        Self { start: t, end: t }
    }

    pub fn duration_micros(&self) -> i64 {
        self.end.0.saturating_sub(self.start.0)
    }

    /// 闭区间包含。
    pub fn contains(&self, t: TimestampMicros) -> bool {
        self.start.0 <= t.0 && t.0 <= self.end.0
    }

    /// 闭区间相交（共享至少一个点即为 true）。
    pub fn overlaps(&self, other: TimeRange) -> bool {
        self.start.0 <= other.end.0 && other.start.0 <= self.end.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(us: i64) -> TimestampMicros {
        TimestampMicros(us)
    }

    #[test]
    fn now_is_monotonic_and_nonzero() {
        let a = TimestampMicros::now();
        let b = TimestampMicros::now();
        assert!(a.0 > 0);
        assert!(b.0 >= a.0);
    }

    #[test]
    fn from_secs_and_millis() {
        assert_eq!(TimestampMicros::from_secs(1).0, 1_000_000);
        assert_eq!(TimestampMicros::from_millis(1).0, 1_000);
    }

    #[test]
    fn contains_inclusive_bounds() {
        let r = TimeRange::new(t(10), t(20));
        assert!(r.contains(t(10)));
        assert!(r.contains(t(15)));
        assert!(r.contains(t(20)));
        assert!(!r.contains(t(9)));
        assert!(!r.contains(t(21)));
    }

    #[test]
    fn overlaps_inclusive_and_disjoint() {
        let a = TimeRange::new(t(10), t(20));
        // identical → overlap
        assert!(a.overlaps(TimeRange::new(t(10), t(20))));
        // nested → overlap
        assert!(a.overlaps(TimeRange::new(t(12), t(15))));
        // touching at single point → overlap (closed interval)
        assert!(a.overlaps(TimeRange::new(t(20), t(30))));
        assert!(a.overlaps(TimeRange::new(t(0), t(10))));
        // strictly before / after → no
        assert!(!a.overlaps(TimeRange::new(t(0), t(9))));
        assert!(!a.overlaps(TimeRange::new(t(21), t(30))));
    }

    #[test]
    fn datetime_roundtrip() {
        let now = TimestampMicros::now();
        let dt = now.to_datetime();
        assert_eq!(TimestampMicros::from_datetime(dt).0, now.0);
    }
}
