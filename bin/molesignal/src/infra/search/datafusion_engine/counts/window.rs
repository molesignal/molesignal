// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How a statement's time window meets the request and the files.

use domain::query::QueryRequest;
use kernel::time::{TimeRange, TimestampMicros};

/// 从索引数的柱子数上限。explorer 一次最多画 512 根；分组更多的语句交给 DataFusion。
const MAX_BARS: i64 = 4096;

/// 请求的时间窗，再按语句的时间条件收窄。
pub(super) fn narrowed(req: &QueryRequest, from: Option<i64>, to: Option<i64>) -> TimeRange {
    TimeRange::new(
        TimestampMicros(req.time_range.start.0.max(from.unwrap_or(i64::MIN))),
        TimestampMicros(req.time_range.end.0.min(to.unwrap_or(i64::MAX))),
    )
}

/// 窗内的柱子不超过 [`MAX_BARS`] 根。柱号随时间单调，首尾两根之间的就是全部。
pub(super) fn fits(window: &std::ops::Range<i64>, origin: i64, width: i64) -> bool {
    if window.is_empty() {
        return true;
    }

    let first = (window.start - origin) / width;
    let last = (window.end - 1 - origin) / width;

    last.checked_sub(first)
        .and_then(|span| span.checked_add(1))
        .is_some_and(|bars| bars <= MAX_BARS)
}

/// 文件的全部行都落在查询时间窗 `[start, end)` 内。
pub(super) fn within(window: TimeRange, file: TimeRange) -> bool {
    window.start.0 <= file.start.0 && file.end.0 < window.end.0
}

/// 文件可能有落在查询时间窗 `[start, end)` 内的行。文件的时间范围两端都含。
pub(super) fn overlaps(window: TimeRange, file: TimeRange) -> bool {
    file.start.0 < window.end.0 && window.start.0 <= file.end.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_fits_when_its_bars_can_be_counted() {
        assert!(fits(&(0..0), 0, 1));
        assert!(fits(&(0..4096), 0, 1));
        assert!(!fits(&(0..4097), 0, 1));
        assert!(fits(&(-5..5), 10, 3));
        assert!(!fits(&(i64::MIN / 2..i64::MAX / 2), 0, 1));
    }
}
