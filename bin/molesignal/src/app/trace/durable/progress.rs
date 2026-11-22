// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What one epoch's candidate log still needs: unsettled spans by sequence, the objects not
//! deleted yet, and the settled spans not recorded yet.

use std::collections::BTreeMap;

use object_store::path::Path;

/// Unsettled spans, stored objects and unrecorded settled spans of one epoch's log.
pub(super) struct Progress {
    pub(super) next_sequence: u64,
    /// Unsettled spans by sequence; a sequence leaves once all its spans are settled.
    pub(super) outstanding: BTreeMap<u64, u32>,
    /// Objects stored and not deleted yet, by sequence.
    pub(super) objects: BTreeMap<u64, Path>,
    /// Settled spans not recorded in the log yet.
    pub(super) settled: Vec<(u64, u32, u32)>,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            next_sequence: 1,
            outstanding: BTreeMap::new(),
            objects: BTreeMap::new(),
            settled: Vec::new(),
        }
    }
}

impl Progress {
    pub(super) fn allocate(&mut self) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        sequence
    }

    /// The lowest sequence a replay could still need.
    pub(super) fn low_watermark(&self) -> u64 {
        self.outstanding
            .keys()
            .next()
            .copied()
            .unwrap_or(self.next_sequence)
    }

    /// Objects below the low watermark: every candidate in them is settled, and every marker in
    /// them only names sequences below it.
    pub(super) fn take_deletable(&mut self) -> Vec<(u64, Path)> {
        let kept = self.objects.split_off(&self.low_watermark());
        std::mem::replace(&mut self.objects, kept)
            .into_iter()
            .collect()
    }

    pub(super) fn take_all(&mut self) -> Vec<(u64, Path)> {
        std::mem::take(&mut self.objects).into_iter().collect()
    }

    pub(super) fn restore(&mut self, objects: Vec<(u64, Path)>) {
        self.objects.extend(objects);
    }
}
