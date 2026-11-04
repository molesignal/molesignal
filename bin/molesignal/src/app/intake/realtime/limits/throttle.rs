// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The throttles of this node, by step and group.
//!
//! Each group is a generic cell rate algorithm, the one `governor` runs for the router's limits:
//! a group lets `limit` events through at once, then one every `window / limit`. It is kept here
//! with an explicit clock and a cap on groups, which `governor`'s keyed limiter has no way to say.
use std::collections::HashMap;

use parking_lot::Mutex;

use crate::{
    domain::pipeline::limit::{MAX_THROTTLE_GROUPS, OTHER_BUCKET, ThrottleSpec},
    shared::ids::Id,
};

/// One throttle step's groups, for the settings they were made with.
struct Throttle {
    limit: u32,
    window: i64,
    /// When each group is back to letting a whole `limit` through (its theoretical arrival time).
    groups: HashMap<String, i64>,
    last_used: i64,
}

#[derive(Default)]
pub(super) struct ThrottleBook {
    throttles: Mutex<HashMap<(Id, String), Throttle>>,
}

impl ThrottleBook {
    /// Whether the step lets one more event of `group` through at `now` (microseconds).
    pub(super) fn admit(
        &self,
        pipeline: &Id,
        step: &str,
        spec: &ThrottleSpec,
        group: String,
        now: i64,
    ) -> bool {
        let window = i64::from(spec.window_secs) * 1_000_000;
        let mut throttles = self.throttles.lock();

        let throttle = throttles
            .entry((pipeline.clone(), step.to_owned()))
            .or_insert_with(|| Throttle::new(spec.limit, window));

        // Changed settings start over.
        if throttle.limit != spec.limit || throttle.window != window {
            *throttle = Throttle::new(spec.limit, window);
        }

        throttle.last_used = now;
        throttle.admit(group, now)
    }

    /// Forgets the groups that are back to full, and throttles unused for `idle`.
    pub(super) fn forget(&self, now: i64, idle: i64) {
        let mut throttles = self.throttles.lock();

        throttles.retain(|_, throttle| now - throttle.last_used < idle);

        for throttle in throttles.values_mut() {
            throttle.groups.retain(|_, full_at| *full_at > now);
        }
    }
}

impl Throttle {
    fn new(limit: u32, window: i64) -> Self {
        Self {
            limit,
            window,
            groups: HashMap::new(),
            last_used: 0,
        }
    }

    fn admit(&mut self, group: String, now: i64) -> bool {
        let interval = (self.window / i64::from(self.limit)).max(1);
        let tolerance = self.window - interval;

        let group = if self.groups.contains_key(&group) || self.groups.len() < MAX_THROTTLE_GROUPS {
            group
        } else {
            OTHER_BUCKET.to_owned()
        };

        let arrival = self.groups.get(&group).copied().unwrap_or(now).max(now);

        if arrival - now > tolerance {
            return false;
        }

        self.groups.insert(group, arrival + interval);
        true
    }
}
