// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The step of a PromQL range query: given by the request, never derived from
//! its row limit.

use super::*;

/// range 查询每条 series 的步数上限，与 Prometheus 的 11,000 点一致。步长太小、
/// 步数超过它的请求被拒绝，提示调大 step。
pub(super) const MAX_RANGE_STEPS: i64 = 11_000;

/// range 查询的步长（micros）：请求给定的 `step`。只有 range 求值读它，那里 `step`
/// 一定存在；缺省时整段跨度算一步。
pub(super) fn range_step_us(req: &QueryRequest) -> i64 {
    req.step
        .unwrap_or_else(|| req.time_range.end.0.saturating_sub(req.time_range.start.0))
        .max(1)
}

/// range 查询的步点 `start, start + step, …`（不超过 `end`），与选择器在每个步点读的窗口的右端。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Steps {
    pub(super) start_us: i64,
    pub(super) end_us: i64,
    pub(super) step_us: i64,
    /// `@` 固定的时刻：每一步都读同一个窗口。
    pinned_us: Option<i64>,
    /// `offset` 把窗口往前移的微秒数，负数往后移。
    offset_us: i64,
}

impl Steps {
    /// 窗口右端就是步点本身。
    pub(super) fn new(start_us: i64, end_us: i64, step_us: i64) -> Self {
        Self {
            start_us,
            end_us,
            step_us,
            pinned_us: None,
            offset_us: 0,
        }
    }

    /// 请求的步点，窗口按选择器 `vs` 的 `@` 与 `offset` 平移，与 instant 求值的
    /// [`effective_eval_time`] 相同；点仍记在请求的步点上。
    pub(super) fn of_selector(vs: &VectorSelector, req: &QueryRequest) -> Self {
        Self {
            pinned_us: pinned_time(vs.at.as_ref(), req),
            offset_us: offset_micros(vs.offset.as_ref()),
            ..Self::new(
                req.time_range.start.0,
                req.time_range.end.0,
                range_step_us(req),
            )
        }
    }

    /// 步点 `t` 读的窗口的右端。
    pub(super) fn window_end(self, t: i64) -> i64 {
        self.pinned_us.unwrap_or(t) - self.offset_us
    }
}

/// 校验请求的 step：必须为正，且每条 series 的步数不超过 [`MAX_RANGE_STEPS`]。
pub(super) fn validate_range_step(req: &QueryRequest) -> Result<()> {
    let Some(step) = req.step else {
        return Ok(());
    };

    if step <= 0 {
        return Err(Error::invalid(
            "PromQL step must be a positive number of microseconds",
        ));
    }

    let span = req.time_range.end.0.saturating_sub(req.time_range.start.0);
    let steps = span / step + 1;

    if steps > MAX_RANGE_STEPS {
        return Err(Error::invalid(format!(
            "PromQL range query exceeds {MAX_RANGE_STEPS} steps per series ({steps}); use a larger step"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use kernel::{ids::Id, time::TimestampMicros};

    use super::*;

    fn request(span_us: i64, step: Option<i64>) -> QueryRequest {
        QueryRequest {
            org_id: Id::from_string("org"),
            language: domain::query::QueryLanguage::Promql,
            statement: "up".into(),
            time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(span_us)),
            stream: None,
            limit: Some(5),
            step,
            federation_clusters: Vec::new(),
        }
    }

    #[test]
    fn the_step_is_the_requested_one_whatever_the_limit() {
        let hour = 3_600_000_000;

        assert_eq!(range_step_us(&request(hour, Some(15_000_000))), 15_000_000);
        assert_eq!(range_step_us(&request(hour, None)), hour);
        assert_eq!(range_step_us(&request(0, None)), 1);
    }

    #[test]
    fn a_step_finer_than_the_steps_per_series_allow_is_refused() {
        let hour = 3_600_000_000;

        assert!(validate_range_step(&request(hour, None)).is_ok());
        assert!(validate_range_step(&request(hour, Some(1_000_000))).is_ok());
        assert!(validate_range_step(&request(hour, Some(0))).is_err());
        assert!(validate_range_step(&request(hour, Some(-5))).is_err());

        let error = validate_range_step(&request(hour, Some(100_000))).unwrap_err();
        assert!(error.to_string().contains("use a larger step"), "{error}");
    }
}
