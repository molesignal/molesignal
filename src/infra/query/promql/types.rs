// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

/// `LabelSet`：稳定排序的 label 集合，作为 series 分组键 + hash 友好。
pub type LabelSet = BTreeMap<String, String>;

/// 单个时间序列：标签集 + (ts_us, value) 列表。
#[derive(Debug, Clone)]
pub struct Series {
    pub labels: LabelSet,
    pub samples: Vec<(i64, f64)>,
}

/// instant 求值结果：每条返回一个 label 集 + 单个 value。
#[derive(Debug, Clone, Default)]
pub struct InstantVector {
    pub items: Vec<(LabelSet, f64)>,
}

/// range 求值结果：每条返回一个时间点、label 集和 value。
#[derive(Debug, Clone)]
pub(super) struct RangePoint {
    pub(super) ts_us: i64,
    pub(super) labels: LabelSet,
    pub(super) value: f64,
}

#[derive(Debug, Clone, Default)]
pub(super) struct RangeVector {
    pub(super) points: Vec<RangePoint>,
}
