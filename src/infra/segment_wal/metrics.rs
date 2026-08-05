// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Segment WAL 自身的可观测指标。
//!
//! - `wal_fsync_errors_total{kind}`：`sync_file` 失败计数。
//!   `kind ∈ {batch_flush, every_write, segment_rotate}` 反映触发该 sync 的路径。
//!   计数器仅记录、不改变错误抛出行为；运维通过本指标快速识别"是磁盘 / fs 异常"而非业务错误。

use std::sync::OnceLock;

use prometheus::IntCounterVec;

use crate::shared::metrics::register_int_counter_vec;

static FSYNC_ERRORS: OnceLock<IntCounterVec> = OnceLock::new();

fn fsync_errors_vec() -> &'static IntCounterVec {
    FSYNC_ERRORS.get_or_init(|| {
        register_int_counter_vec(
            "wal_fsync_errors_total",
            "WAL sync_data / sync_all failures by trigger path",
            &["kind"],
        )
    })
}

/// 触发 sync 的路径标签。
pub mod fsync_kind {
    pub const BATCH_FLUSH: &str = "batch_flush";
    pub const EVERY_WRITE: &str = "every_write";
    pub const SEGMENT_ROTATE: &str = "segment_rotate";
}

pub(crate) fn inc_fsync_error(kind: &str) {
    fsync_errors_vec().with_label_values(&[kind]).inc();
}

#[cfg(test)]
pub(crate) fn fsync_error_count(kind: &str) -> u64 {
    fsync_errors_vec().with_label_values(&[kind]).get()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 替代方案：跨平台稳定地触发 `fdatasync` 失败困难（`/dev/full` 在 macOS 不可用，
    /// 各 FS 行为不一），改为白盒断言"counter 接线正确 + label 分桶生效"。
    /// 真实 fsync 错误的 metric 路径靠 `write_record` / `flush_and_fsync` / `rotate` 三处的
    /// `inc_fsync_error(...)` 在 `if let Err` 分支里被调用 —— 走读即可确认。
    #[test]
    fn fsync_error_counter_increments_per_label() {
        const LABEL: &str = "test_kind_unit_isolated";
        let before = fsync_error_count(LABEL);
        inc_fsync_error(LABEL);
        inc_fsync_error(LABEL);
        let after = fsync_error_count(LABEL);
        assert_eq!(after - before, 2);
        // 与生产标签独立计数。
        assert_eq!(fsync_error_count("non_existent_kind_99"), 0);
    }
}
