// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The stretch a window is compared with: the same length just before it, or the same stretch a
//! week earlier.

use kernel::time::{TimeRange, TimestampMicros};
use serde::Deserialize;

const WEEK_MICROS: i64 = 7 * 24 * 60 * 60 * 1_000_000;

/// What a window is compared with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Baseline {
    /// The stretch of the same length just before the window.
    #[default]
    Previous,
    /// The same stretch a week earlier.
    WeekBefore,
    /// No comparison.
    #[serde(rename = "none")]
    Skip,
}

impl Baseline {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Previous => "previous",
            Self::WeekBefore => "week_before",
            Self::Skip => "none",
        }
    }

    /// The stretch to compare `window` with, both ends inside it as they are in a window; `None`
    /// when there is no comparison.
    pub(super) fn range(self, window: TimeRange) -> Option<TimeRange> {
        let start = window.start.0;
        let end = window.end.0;

        match self {
            Self::Previous => {
                let last = start.saturating_sub(1);
                let first = last.saturating_sub(end.saturating_sub(start));

                Some(TimeRange::new(
                    TimestampMicros(first),
                    TimestampMicros(last),
                ))
            }
            Self::WeekBefore => Some(TimeRange::new(
                TimestampMicros(start.saturating_sub(WEEK_MICROS)),
                TimestampMicros(end.saturating_sub(WEEK_MICROS)),
            )),
            Self::Skip => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> TimeRange {
        TimeRange::new(
            TimestampMicros(10_000_000_000),
            TimestampMicros(10_003_600_000),
        )
    }

    #[test]
    fn the_previous_stretch_is_as_long_and_ends_just_before_the_window() {
        let previous = Baseline::Previous.range(window()).unwrap();

        assert_eq!(previous.end.0, window().start.0 - 1);
        assert_eq!(
            previous.end.0 - previous.start.0,
            window().end.0 - window().start.0
        );
    }

    #[test]
    fn the_week_before_is_the_same_stretch_seven_days_earlier() {
        let week = Baseline::WeekBefore.range(window()).unwrap();

        assert_eq!(window().start.0 - week.start.0, WEEK_MICROS);
        assert_eq!(window().end.0 - week.end.0, WEEK_MICROS);
    }

    #[test]
    fn none_compares_with_nothing_and_previous_is_the_default() {
        assert_eq!(Baseline::Skip.range(window()), None);
        assert_eq!(Baseline::default(), Baseline::Previous);

        let parsed: Baseline = serde_json::from_value(serde_json::json!("none")).unwrap();
        assert_eq!(parsed, Baseline::Skip);
        assert_eq!(parsed.as_str(), "none");
    }
}
