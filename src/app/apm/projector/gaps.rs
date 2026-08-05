// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashMap;

use parking_lot::Mutex;

use crate::{
    domain::apm::{ProjectionGap, ProjectionGapReason},
    shared::{
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

const MINUTE_MICROS: i64 = 60_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GapKey {
    org_id: Id,
    minute_at: TimestampMicros,
    reason: ProjectionGapReason,
}

pub(super) struct GapLedger {
    capacity: usize,
    values: Mutex<HashMap<GapKey, u64>>,
}

impl GapLedger {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            values: Mutex::new(HashMap::new()),
        }
    }

    pub(super) fn record(
        &self,
        org_id: &Id,
        event_time: TimestampMicros,
        reason: ProjectionGapReason,
        count: u64,
    ) {
        let minute_at = TimestampMicros(event_time.0.div_euclid(MINUTE_MICROS) * MINUTE_MICROS);
        let key = GapKey {
            org_id: org_id.clone(),
            minute_at,
            reason,
        };
        let mut values = self.values.lock();
        if values.len() >= self.capacity && !values.contains_key(&key) {
            if let Some((_, current)) = values.iter_mut().next() {
                *current = current.saturating_add(count);
            }
            return;
        }
        values
            .entry(key)
            .and_modify(|current| *current = current.saturating_add(count))
            .or_insert(count);
    }

    pub(super) fn record_now(&self, org_id: Option<&Id>, reason: ProjectionGapReason, count: u64) {
        if let Some(org_id) = org_id {
            self.record(org_id, TimestampMicros::now(), reason, count);
        }
    }

    pub(super) fn take(&self) -> Vec<ProjectionGap> {
        let values = std::mem::take(&mut *self.values.lock());
        let now = TimestampMicros::now();
        values
            .into_iter()
            .map(|(key, dropped_facts)| ProjectionGap {
                org_id: key.org_id,
                range: TimeRange::new(
                    key.minute_at,
                    TimestampMicros(key.minute_at.0.saturating_add(MINUTE_MICROS - 1)),
                ),
                reason: key.reason,
                dropped_facts,
                recorded_at: now,
            })
            .collect()
    }

    pub(super) fn restore(&self, gaps: &[ProjectionGap]) {
        for gap in gaps {
            self.record(&gap.org_id, gap.range.start, gap.reason, gap.dropped_facts);
        }
    }
}
