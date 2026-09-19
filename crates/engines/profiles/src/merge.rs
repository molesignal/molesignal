// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 火焰图合并器（task 4.2 / 4.4）。
//!
//! 把窗口内多份 [`NormalizedProfile`] 按 frame 路径聚合成一棵栈树，再扁平化为
//! **flamebearer**（`names[]` + `levels[]`，与生态火焰图渲染兼容）。差分（diff）
//! 在 baseline / comparison 两侧累计后按 frame 路径求带符号增量。
//!
//! flamebearer `levels[d]` 是该深度所有 bar 的扁平整型数组，单 bar 4 个整数：
//! `[offset, total, self, name_index]`，`offset` 为同层相对上一 bar 右沿的间隙。
//! diff 单 bar 5 个整数：`[offset, total, self, name_index, delta]`，`total` =
//! 两侧之和（决定宽度，保证仅一侧出现的帧也可见），`delta` = comparison − baseline。

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use super::NormalizedProfile;

/// 单窗口聚合 flamebearer。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flamebearer {
    /// frame 名字符串表，`names[0]` 恒为 `"total"`（根）。
    pub names: Vec<String>,
    /// 每层 bar 的扁平数组，单 bar 4 整数 `[offset, total, self, name_index]`。
    pub levels: Vec<Vec<i64>>,
    /// 根总值（= 合并样本主值之和）。
    pub num_ticks: i64,
    /// 单帧 self 最大值（着色归一用）。
    pub max_self: i64,
    /// 主采样值单位（`nanoseconds` / `bytes` / `count` ...）。
    pub units: String,
}

/// 差分 flamebearer。单 bar 5 整数 `[offset, total, self, name_index, delta]`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffFlamebearer {
    pub names: Vec<String>,
    pub levels: Vec<Vec<i64>>,
    /// 两侧总值之和。
    pub num_ticks: i64,
    pub max_self: i64,
    /// 单帧带符号增量的最大绝对值（着色归一用）。
    pub max_abs_delta: i64,
    pub units: String,
}

fn units_of(profiles: &[NormalizedProfile]) -> String {
    profiles
        .first()
        .and_then(|p| p.sample_types.get(p.default_value_index))
        .map(|vt| vt.unit.clone())
        .unwrap_or_default()
}

// ===== 单窗口合并 =====

struct Node {
    name_idx: usize,
    total: i64,
    self_val: i64,
    children: HashMap<usize, Node>,
}

impl Node {
    fn new(name_idx: usize) -> Self {
        Self {
            name_idx,
            total: 0,
            self_val: 0,
            children: HashMap::new(),
        }
    }
}

struct Builder {
    names: Vec<String>,
    name_idx: HashMap<String, usize>,
    root: Node,
}

impl Builder {
    fn new() -> Self {
        let mut b = Builder {
            names: Vec::new(),
            name_idx: HashMap::new(),
            root: Node::new(0),
        };
        b.intern("total"); // index 0
        b
    }

    fn intern(&mut self, name: &str) -> usize {
        if let Some(&i) = self.name_idx.get(name) {
            return i;
        }
        let i = self.names.len();
        self.names.push(name.to_string());
        self.name_idx.insert(name.to_string(), i);
        i
    }

    fn add(&mut self, p: &NormalizedProfile) {
        let vi = p.default_value_index;
        for s in &p.samples {
            let v = s.values.get(vi).copied().unwrap_or(0);
            if v <= 0 {
                continue;
            }
            // 先 intern（结束对 self 的可变借用），再走树。
            let idxs: Vec<usize> = s
                .stack
                .iter()
                .map(|fr| self.intern(&fr.display_name()))
                .collect();
            self.root.total += v;
            let mut node = &mut self.root;
            for &idx in &idxs {
                node = node.children.entry(idx).or_insert_with(|| Node::new(idx));
                node.total += v;
            }
            node.self_val += v;
        }
    }

    fn flatten(&self) -> (Vec<Vec<i64>>, i64) {
        let mut levels: Vec<Vec<i64>> = Vec::new();
        let mut prev_end: Vec<i64> = Vec::new();
        let mut max_self = 0;
        walk(&self.root, 0, 0, &mut levels, &mut prev_end, &mut max_self);
        (levels, max_self)
    }
}

fn walk(
    node: &Node,
    depth: usize,
    x_start: i64,
    levels: &mut Vec<Vec<i64>>,
    prev_end: &mut Vec<i64>,
    max_self: &mut i64,
) {
    if levels.len() <= depth {
        levels.push(Vec::new());
        prev_end.push(0);
    }
    let offset = x_start - prev_end[depth];
    levels[depth].extend_from_slice(&[offset, node.total, node.self_val, node.name_idx as i64]);
    prev_end[depth] = x_start + node.total;
    if node.self_val > *max_self {
        *max_self = node.self_val;
    }
    let mut kids: Vec<&Node> = node.children.values().collect();
    // 宽优先稳定排序：total 降序，再 name_idx 升序。
    kids.sort_by(|a, b| b.total.cmp(&a.total).then(a.name_idx.cmp(&b.name_idx)));
    let mut cx = x_start;
    for kid in kids {
        walk(kid, depth + 1, cx, levels, prev_end, max_self);
        cx += kid.total;
    }
}

/// 合并多份 profile 为单窗口 flamebearer。
pub fn build_flamebearer(profiles: &[NormalizedProfile]) -> Flamebearer {
    let mut b = Builder::new();
    for p in profiles {
        b.add(p);
    }
    let num_ticks = b.root.total;
    let (levels, max_self) = b.flatten();
    Flamebearer {
        names: b.names,
        levels,
        num_ticks,
        max_self,
        units: units_of(profiles),
    }
}

// ===== 差分合并 =====

struct DiffNode {
    name_idx: usize,
    left: i64,
    right: i64,
    left_self: i64,
    right_self: i64,
    children: HashMap<usize, DiffNode>,
}

impl DiffNode {
    fn new(name_idx: usize) -> Self {
        Self {
            name_idx,
            left: 0,
            right: 0,
            left_self: 0,
            right_self: 0,
            children: HashMap::new(),
        }
    }
}

struct DiffBuilder {
    names: Vec<String>,
    name_idx: HashMap<String, usize>,
    root: DiffNode,
}

impl DiffBuilder {
    fn new() -> Self {
        let mut b = DiffBuilder {
            names: Vec::new(),
            name_idx: HashMap::new(),
            root: DiffNode::new(0),
        };
        b.intern("total");
        b
    }

    fn intern(&mut self, name: &str) -> usize {
        if let Some(&i) = self.name_idx.get(name) {
            return i;
        }
        let i = self.names.len();
        self.names.push(name.to_string());
        self.name_idx.insert(name.to_string(), i);
        i
    }

    fn add(&mut self, p: &NormalizedProfile, right: bool) {
        let vi = p.default_value_index;
        for s in &p.samples {
            let v = s.values.get(vi).copied().unwrap_or(0);
            if v <= 0 {
                continue;
            }
            let idxs: Vec<usize> = s
                .stack
                .iter()
                .map(|fr| self.intern(&fr.display_name()))
                .collect();
            if right {
                self.root.right += v;
            } else {
                self.root.left += v;
            }
            let mut node = &mut self.root;
            for &idx in &idxs {
                node = node
                    .children
                    .entry(idx)
                    .or_insert_with(|| DiffNode::new(idx));
                if right {
                    node.right += v;
                } else {
                    node.left += v;
                }
            }
            if right {
                node.right_self += v;
            } else {
                node.left_self += v;
            }
        }
    }

    fn flatten(&self) -> (Vec<Vec<i64>>, i64, i64) {
        let mut levels: Vec<Vec<i64>> = Vec::new();
        let mut prev_end: Vec<i64> = Vec::new();
        let mut max_self = 0;
        let mut max_abs_delta = 0;
        diff_walk(
            &self.root,
            0,
            0,
            &mut levels,
            &mut prev_end,
            &mut max_self,
            &mut max_abs_delta,
        );
        (levels, max_self, max_abs_delta)
    }
}

#[allow(clippy::too_many_arguments)]
fn diff_walk(
    node: &DiffNode,
    depth: usize,
    x_start: i64,
    levels: &mut Vec<Vec<i64>>,
    prev_end: &mut Vec<i64>,
    max_self: &mut i64,
    max_abs_delta: &mut i64,
) {
    if levels.len() <= depth {
        levels.push(Vec::new());
        prev_end.push(0);
    }
    let total = node.left + node.right;
    let self_val = node.left_self + node.right_self;
    let delta = node.right - node.left;
    let offset = x_start - prev_end[depth];
    levels[depth].extend_from_slice(&[offset, total, self_val, node.name_idx as i64, delta]);
    prev_end[depth] = x_start + total;
    if self_val > *max_self {
        *max_self = self_val;
    }
    if delta.abs() > *max_abs_delta {
        *max_abs_delta = delta.abs();
    }
    let mut kids: Vec<&DiffNode> = node.children.values().collect();
    kids.sort_by(|a, b| {
        (b.left + b.right)
            .cmp(&(a.left + a.right))
            .then(a.name_idx.cmp(&b.name_idx))
    });
    let mut cx = x_start;
    for kid in kids {
        diff_walk(
            kid,
            depth + 1,
            cx,
            levels,
            prev_end,
            max_self,
            max_abs_delta,
        );
        cx += kid.left + kid.right;
    }
}

/// 合并 baseline / comparison 两组 profile 为差分 flamebearer。
pub fn build_diff(
    baseline: &[NormalizedProfile],
    comparison: &[NormalizedProfile],
) -> DiffFlamebearer {
    let mut b = DiffBuilder::new();
    for p in baseline {
        b.add(p, false);
    }
    for p in comparison {
        b.add(p, true);
    }
    let num_ticks = b.root.left + b.root.right;
    let (levels, max_self, max_abs_delta) = b.flatten();
    let units = if !comparison.is_empty() {
        units_of(comparison)
    } else {
        units_of(baseline)
    };
    DiffFlamebearer {
        names: b.names,
        levels,
        num_ticks,
        max_self,
        max_abs_delta,
        units,
    }
}

// ===== 窗口内均匀采样（task 4.2 上限）=====

/// 当 `items` 超过 `max` 时，按下标均匀采样到 `max` 个并返回 `truncated = true`；
/// 否则原样返回、`truncated = false`。`max == 0` 视为不限制。
pub fn even_sample<T>(items: Vec<T>, max: usize) -> (Vec<T>, bool) {
    let n = items.len();
    if max == 0 || n <= max {
        return (items, false);
    }
    if max == 1 {
        let mut it = items.into_iter();
        return (it.next().into_iter().collect(), true);
    }
    let picked: BTreeSet<usize> = (0..max)
        .map(|i| (i as u128 * (n as u128 - 1) / (max as u128 - 1)) as usize)
        .collect();
    let out = items
        .into_iter()
        .enumerate()
        .filter_map(|(i, it)| picked.contains(&i).then_some(it))
        .collect();
    (out, true)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        super::{Frame, ProfileType, Sample, ValueType},
        *,
    };

    fn frame(name: &str) -> Frame {
        Frame {
            function: name.to_string(),
            file: None,
            line: None,
            address: None,
            build_id: None,
        }
    }

    fn profile(stacks: &[(&[&str], i64)]) -> NormalizedProfile {
        let samples = stacks
            .iter()
            .map(|(names, v)| Sample {
                stack: names.iter().map(|n| frame(n)).collect(),
                values: vec![*v],
                labels: BTreeMap::new(),
            })
            .collect();
        NormalizedProfile {
            service: "api".into(),
            profile_type: ProfileType::Cpu,
            sample_types: vec![ValueType::new("cpu", "nanoseconds")],
            default_value_index: 0,
            samples,
            period_type: None,
            period: 0,
            start_time_micros: 0,
            duration_nanos: 0,
            labels: BTreeMap::new(),
            trace_id: None,
            span_id: None,
        }
    }

    #[test]
    fn flamebearer_root_total_equals_sum() {
        let p = profile(&[(&["main", "a"], 10), (&["main", "b"], 30)]);
        let fb = build_flamebearer(&[p]);
        assert_eq!(fb.num_ticks, 40);
        assert_eq!(fb.names[0], "total");
        // root level: [offset=0, total=40, self=0, name=0]
        assert_eq!(fb.levels[0], vec![0, 40, 0, 0]);
        assert_eq!(fb.units, "nanoseconds");
    }

    #[test]
    fn flamebearer_merges_multiple_profiles_by_path() {
        let p1 = profile(&[(&["main", "a"], 10)]);
        let p2 = profile(&[(&["main", "a"], 5), (&["main", "b"], 7)]);
        let fb = build_flamebearer(&[p1, p2]);
        assert_eq!(fb.num_ticks, 22);
        // depth 1 is "main" spanning full width.
        assert_eq!(fb.levels[1][..2], [0, 22]);
        // depth 2 holds a(15) and b(7) — widest first.
        let level2 = &fb.levels[2];
        assert_eq!(level2.len(), 8); // two bars × 4 ints
        assert_eq!(level2[1], 15); // first bar total (a, widest)
        assert_eq!(level2[5], 7); // second bar total (b)
    }

    #[test]
    fn diff_marks_growth_positive_and_baseline_only_negative() {
        // baseline: hot=10, gone=4 ; comparison: hot=30 (grew), new=5
        let baseline = profile(&[(&["main", "hot"], 10), (&["main", "gone"], 4)]);
        let comparison = profile(&[(&["main", "hot"], 30), (&["main", "new"], 5)]);
        let diff = build_diff(&[baseline], &[comparison]);
        // find the "hot" and "gone" bars at depth 2 and check delta sign.
        let level2 = &diff.levels[2];
        let mut deltas: BTreeMap<String, i64> = BTreeMap::new();
        for bar in level2.chunks(5) {
            let name = diff.names[bar[3] as usize].clone();
            deltas.insert(name, bar[4]);
        }
        assert_eq!(deltas["hot"], 20); // 30 - 10
        assert_eq!(deltas["gone"], -4); // only in baseline
        assert_eq!(deltas["new"], 5); // only in comparison
        assert_eq!(diff.num_ticks, 49); // 14 + 35
    }

    #[test]
    fn even_sample_caps_and_flags_truncation() {
        let (out, trunc) = even_sample((0..10).collect(), 3);
        assert_eq!(out.len(), 3);
        assert!(trunc);
        assert_eq!(out[0], 0);
        assert_eq!(out[2], 9); // endpoints preserved

        let (out, trunc) = even_sample(vec![1, 2], 5);
        assert_eq!(out, vec![1, 2]);
        assert!(!trunc);

        let (out, trunc) = even_sample((0..100).collect::<Vec<_>>(), 0);
        assert_eq!(out.len(), 100);
        assert!(!trunc);
    }
}
