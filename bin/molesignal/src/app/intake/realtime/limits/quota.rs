// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The quotas of this node: what each bucket let through today, as the ledger last said for every
//! node, plus what this node counted since.
//!
//! The first event of a quota on a day waits for the ledger's counts of that day, so a restart goes
//! on from them. When the ledger cannot be read the quota counts from zero until the next sync reads
//! it: data goes on rather than being held back by a database that does not answer.
use std::collections::HashMap;

use parking_lot::Mutex;

use crate::{
    domain::pipeline::limit::{
        LimitKey, LimitUsage, LimitUsageLedger, LimitUsageRow, MAX_QUOTA_BUCKETS, OTHER_BUCKET,
        QuotaSpec,
    },
    shared::ids::Id,
};

/// A quota's buckets on one day.
struct QuotaDay {
    org: Id,
    loaded: bool,
    buckets: HashMap<String, Counts>,
    last_used: i64,
}

#[derive(Default)]
struct Counts {
    /// What the ledger held for every node when it was last read.
    stored: LimitUsage,
    /// What this node counted and is writing now.
    in_flight: LimitUsage,
    /// What this node counted since.
    pending: LimitUsage,
}

impl Counts {
    fn used(&self) -> LimitUsage {
        let mut used = self.stored;
        used.add(&self.in_flight);
        used.add(&self.pending);
        used
    }
}

#[derive(Default)]
pub(super) struct QuotaBook {
    days: Mutex<HashMap<LimitKey, QuotaDay>>,
}

impl QuotaBook {
    /// Reads a quota's counts of a day from the ledger, once a day per node. `now` keeps the day from
    /// being forgotten before its first event is counted.
    pub(super) async fn load(
        &self,
        ledger: &dyn LimitUsageLedger,
        org: &Id,
        key: &LimitKey,
        now: i64,
    ) {
        if self.days.lock().get(key).is_some_and(|day| day.loaded) {
            return;
        }

        let rows = match ledger.totals(std::slice::from_ref(key)).await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(
                    %error,
                    pipeline = %key.pipeline_id.0,
                    step = %key.step,
                    "quota counts unreadable; counting from zero until the next sync"
                );

                Vec::new()
            }
        };

        let mut days = self.days.lock();

        let day = days
            .entry(key.clone())
            .or_insert_with(|| QuotaDay::new(org));

        day.last_used = day.last_used.max(now);

        if !day.loaded {
            for row in rows {
                day.buckets.entry(row.bucket).or_default().stored = row.usage;
            }

            day.loaded = true;
        }
    }

    /// Counts an event of `bytes` into its bucket: `true` while the bucket is within its limit. An
    /// event is let through while what its bucket let through is below the limit, so the last one
    /// can take it a little past.
    pub(super) fn admit(
        &self,
        spec: &QuotaSpec,
        org: &Id,
        key: &LimitKey,
        bucket: String,
        bytes: u64,
        now: i64,
    ) -> bool {
        let mut days = self.days.lock();

        let day = days
            .entry(key.clone())
            .or_insert_with(|| QuotaDay::new(org));

        day.last_used = now;

        let bucket = if day.buckets.contains_key(&bucket)
            || spec.has_own_limit(&bucket)
            || day.buckets.len() < MAX_QUOTA_BUCKETS
        {
            bucket
        } else {
            OTHER_BUCKET.to_owned()
        };

        let limit = spec.limit_of(&bucket);
        let counts = day.buckets.entry(bucket).or_default();
        let within = counts.used().admitted(spec.unit) < limit;

        let counted = if within {
            LimitUsage {
                admitted_events: 1,
                admitted_bytes: bytes,
                ..LimitUsage::default()
            }
        } else {
            LimitUsage {
                over_events: 1,
                over_bytes: bytes,
                ..LimitUsage::default()
            }
        };

        counts.pending.add(&counted);
        within
    }

    /// What this node counted since it last wrote, now being written, and the days to read back.
    pub(super) fn begin_sync(&self) -> (Vec<LimitUsageRow>, Vec<LimitKey>) {
        let mut days = self.days.lock();
        let mut rows = Vec::new();

        for (key, day) in days.iter_mut() {
            for (bucket, counts) in &mut day.buckets {
                if counts.pending.is_empty() {
                    continue;
                }

                counts.in_flight.add(&counts.pending);

                rows.push(LimitUsageRow {
                    org_id: day.org.clone(),
                    key: key.clone(),
                    bucket: bucket.clone(),
                    usage: std::mem::take(&mut counts.pending),
                });
            }
        }

        (rows, days.keys().cloned().collect())
    }

    /// The write that [`Self::begin_sync`] started is done (`written`), or failed and is tried again
    /// with the next one.
    pub(super) fn end_write(&self, written: bool) {
        let mut days = self.days.lock();

        for counts in days.values_mut().flat_map(|day| day.buckets.values_mut()) {
            let in_flight = std::mem::take(&mut counts.in_flight);

            if written {
                counts.stored.add(&in_flight);
            } else {
                counts.pending.add(&in_flight);
            }
        }
    }

    /// Takes what the ledger holds now, for every node.
    pub(super) fn read_back(&self, rows: Vec<LimitUsageRow>) {
        let mut days = self.days.lock();

        for row in rows {
            if let Some(day) = days.get_mut(&row.key) {
                day.buckets.entry(row.bucket).or_default().stored = row.usage;
            }
        }
    }

    /// Forgets the days before `today` once they are written, and quotas unused for `idle`.
    pub(super) fn forget(&self, today: i64, now: i64, idle: i64) {
        self.days.lock().retain(|key, day| {
            let unwritten = day
                .buckets
                .values()
                .any(|counts| !counts.pending.is_empty() || !counts.in_flight.is_empty());

            unwritten || (key.day >= today && now - day.last_used < idle)
        });
    }

    #[cfg(test)]
    pub(super) fn used(&self, key: &LimitKey, bucket: &str) -> LimitUsage {
        self.days
            .lock()
            .get(key)
            .and_then(|day| day.buckets.get(bucket))
            .map(Counts::used)
            .unwrap_or_default()
    }
}

impl QuotaDay {
    fn new(org: &Id) -> Self {
        Self {
            org: org.clone(),
            loaded: false,
            buckets: HashMap::new(),
            last_used: 0,
        }
    }
}
