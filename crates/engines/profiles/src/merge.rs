// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Flame graph merger (task 4.2 / 4.4).
//!
//! Aggregates multiple [`NormalizedProfile`] instances in a window into one stack tree by frame
//! path, then flattens it into a **flamebearer** (`names[]` + `levels[]`, compatible with
//! ecosystem flame graph renderers). A diff accumulates the baseline / comparison sides, then
//! computes the signed delta per frame path.
//!
//! In a flamebearer, `levels[d]` is the flat integer array of all bars at that depth, with 4
//! integers per bar: `[offset, total, self, name_index]`, where `offset` is the gap from the
//! right edge of the previous bar on the same level.
//! A diff bar has 5 integers: `[offset, total, self, name_index, delta]`, where `total` = the
//! sum of both sides (it determines the width and guarantees that frames present on only one
//! side are also visible), and `delta` = comparison - baseline.

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use super::NormalizedProfile;

/// Aggregated flamebearer for a single window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flamebearer {
    /// Frame name string table; `names[0]` is always `"total"` (the root).
    pub names: Vec<String>,
    /// Flat array of bars per level, 4 integers per bar: `[offset, total, self, name_index]`.
    pub levels: Vec<Vec<i64>>,
    /// Total value at the root (= sum of the primary values of the merged samples).
    pub num_ticks: i64,
    /// Maximum self value of a single frame (for color normalization).
    pub max_self: i64,
    /// Unit of the primary sample value (`nanoseconds` / `bytes` / `count` ...).
    pub units: String,
}

/// Differential flamebearer. 5 integers per bar: `[offset, total, self, name_index, delta]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffFlamebearer {
    pub names: Vec<String>,
    pub levels: Vec<Vec<i64>>,
    /// Sum of the totals of both sides.
    pub num_ticks: i64,
    pub max_self: i64,
    /// Maximum absolute value of the signed delta of any single frame (for color normalization).
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

// ===== Single-window merge =====

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

            // Intern first (ending the mutable borrow of self), then walk the tree.
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
    // Widest-first stable sort: total descending, then name_idx ascending.
    kids.sort_by(|a, b| b.total.cmp(&a.total).then(a.name_idx.cmp(&b.name_idx)));
    let mut cx = x_start;

    for kid in kids {
        walk(kid, depth + 1, cx, levels, prev_end, max_self);
        cx += kid.total;
    }
}

/// Merges multiple profiles into a single-window flamebearer.
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

// ===== Differential merge =====

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

/// Merges the baseline / comparison profile groups into a differential flamebearer.
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

// ===== Even sampling within a window (task 4.2 limit) =====

/// When `items` exceeds `max`, samples evenly by index down to `max` items and returns
/// `truncated = true`; otherwise returns `items` unchanged with `truncated = false`.
/// `max == 0` means no limit.
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
