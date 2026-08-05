// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 物理数据集与时间分区约定。
//!
//! [`StreamType`](crate::domain::stream::StreamType) 描述用户可见的信号类型；
//! [`PhysicalDatasetKind`] 描述同一逻辑 stream 在对象存储里的物理用途。二者分开后，
//! Trace 摘要、RUM 摘要、指标 rollup 不再和原始事件混写，也不会被通用查询误扫。

use std::{fmt, str::FromStr};

use chrono::{Datelike, Timelike};
use serde::{Deserialize, Serialize};

use crate::{
    domain::stream::StreamType,
    shared::{Error, time::TimestampMicros},
};

pub const HOUR_MICROS: i64 = 60 * 60 * 1_000_000;

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PhysicalDatasetKind {
    #[default]
    Raw,
    TraceSummary,
    RumSessionSummary,
    RumErrorSummary,
    MetricCatalog,
    MetricRollup,
}

impl PhysicalDatasetKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::TraceSummary => "trace_summary",
            Self::RumSessionSummary => "rum_session_summary",
            Self::RumErrorSummary => "rum_error_summary",
            Self::MetricCatalog => "metric_catalog",
            Self::MetricRollup => "metric_rollup",
        }
    }
}

/// 重建一个用户可见逻辑 stream 时应透明读取的物理数据集。
///
/// Metrics 的老数据会由 compactor 从 `raw` 原子替换为
/// `metric_rollup`；两者合起来才是完整时间序列。其他派生数据集
/// 是专用读模型，不得混入通用原始事件查询。
pub const fn logical_query_datasets(stream_type: StreamType) -> &'static [PhysicalDatasetKind] {
    match stream_type {
        StreamType::Metrics => &[PhysicalDatasetKind::Raw, PhysicalDatasetKind::MetricRollup],
        _ => &[PhysicalDatasetKind::Raw],
    }
}

impl fmt::Display for PhysicalDatasetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PhysicalDatasetKind {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "raw" => Ok(Self::Raw),
            "trace_summary" => Ok(Self::TraceSummary),
            "rum_session_summary" => Ok(Self::RumSessionSummary),
            "rum_error_summary" => Ok(Self::RumErrorSummary),
            "metric_catalog" => Ok(Self::MetricCatalog),
            "metric_rollup" => Ok(Self::MetricRollup),
            other => Err(Error::invalid(format!(
                "unknown physical dataset kind `{other}`"
            ))),
        }
    }
}

/// 返回时间戳所属 UTC 小时的起点；对 Unix epoch 之前的值也使用向下取整。
pub const fn hour_start_micros(timestamp: TimestampMicros) -> TimestampMicros {
    TimestampMicros(timestamp.0.div_euclid(HOUR_MICROS) * HOUR_MICROS)
}

/// 对象存储路径使用的 UTC 小时分区段：`YYYY/MM/DD/HH`。
pub fn hour_partition_path(timestamp: TimestampMicros) -> String {
    let datetime = hour_start_micros(timestamp).to_datetime();
    format!(
        "{:04}/{:02}/{:02}/{:02}",
        datetime.year(),
        datetime.month(),
        datetime.day(),
        datetime.hour()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_dataset_roundtrips() {
        for kind in [
            PhysicalDatasetKind::Raw,
            PhysicalDatasetKind::TraceSummary,
            PhysicalDatasetKind::RumSessionSummary,
            PhysicalDatasetKind::RumErrorSummary,
            PhysicalDatasetKind::MetricCatalog,
            PhysicalDatasetKind::MetricRollup,
        ] {
            assert_eq!(kind.as_str().parse::<PhysicalDatasetKind>().unwrap(), kind);
        }
    }

    #[test]
    fn metrics_logical_query_includes_raw_and_rollup_only() {
        assert_eq!(
            logical_query_datasets(StreamType::Metrics),
            &[PhysicalDatasetKind::Raw, PhysicalDatasetKind::MetricRollup,]
        );
        assert_eq!(
            logical_query_datasets(StreamType::Logs),
            &[PhysicalDatasetKind::Raw]
        );
    }

    #[test]
    fn hour_partition_uses_floor_and_utc_path() {
        assert_eq!(
            hour_start_micros(TimestampMicros(3_600_000_001)).0,
            3_600_000_000
        );
        assert_eq!(hour_start_micros(TimestampMicros(-1)).0, -3_600_000_000);
        assert_eq!(
            hour_partition_path(TimestampMicros(1_767_225_600_000_000)),
            "2026/01/01/00"
        );
    }
}
