// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A writer epoch and how long this process may still acknowledge writes under it.
//!
//! The registry extends the lease to `now() + lease` by the database clock. The process counts
//! from the moment it *sent* the renewal, which is no later than the database's `now()`, and stops
//! a fifth of the lease early. So it stops acknowledging before the database could see the lease
//! lapse, and a takeover, which only claims lapsed writers, never misses an acknowledged object.
//! This holds as long as the two clocks run at about the same rate, not that they agree.

use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use domain::storage::{WriterEpoch, WriterNodeId};

use crate::infra::intake::BufferWriter;

pub struct WriterLease {
    node_id: WriterNodeId,
    epoch: WriterEpoch,
    valid_until: Mutex<Instant>,
    fenced: AtomicBool,
}

impl WriterLease {
    /// A lease the registry granted for `lease` from a request sent at `sent_at`.
    pub fn granted(
        node_id: WriterNodeId,
        epoch: WriterEpoch,
        sent_at: Instant,
        lease: Duration,
    ) -> Self {
        Self {
            node_id,
            epoch,
            valid_until: Mutex::new(sent_at + usable(lease)),
            fenced: AtomicBool::new(false),
        }
    }

    pub fn epoch(&self) -> WriterEpoch {
        self.epoch
    }

    pub fn node_id(&self) -> &WriterNodeId {
        &self.node_id
    }

    /// The identity rows written under this lease carry in the buffer.
    pub fn writer(&self) -> BufferWriter {
        BufferWriter::new(self.node_id.clone(), self.epoch)
    }

    /// Whether a write may be acknowledged under this lease now.
    pub fn is_valid(&self) -> bool {
        !self.is_fenced() && Instant::now() < *self.valid_until.lock().expect("lease lock")
    }

    pub fn is_fenced(&self) -> bool {
        self.fenced.load(Ordering::Acquire)
    }

    /// A renewal sent at `sent_at` succeeded.
    pub fn extend(&self, sent_at: Instant, lease: Duration) {
        let mut valid_until = self.valid_until.lock().expect("lease lock");
        *valid_until = (*valid_until).max(sent_at + usable(lease));
    }

    /// The writer was fenced, or released: nothing is acknowledged under it again.
    pub fn fence(&self) {
        self.fenced.store(true, Ordering::Release);
    }
}

/// The part of a lease the process uses: four fifths, so a slow renewal or a little clock-rate
/// drift never lets it outlive what the database granted.
fn usable(lease: Duration) -> Duration {
    lease.mul_f64(0.8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lease_ends_early_extends_forward_only_and_stays_fenced() {
        let lease = Duration::from_secs(10);
        let long_ago = Instant::now() - Duration::from_secs(9);
        let lease_holder =
            WriterLease::granted(WriterNodeId::new("n"), WriterEpoch(4), long_ago, lease);
        assert!(!lease_holder.is_valid(), "8 of 10 seconds are usable");

        lease_holder.extend(Instant::now(), lease);
        assert!(lease_holder.is_valid());

        lease_holder.extend(long_ago, lease);
        assert!(
            lease_holder.is_valid(),
            "an older renewal never shortens the lease"
        );

        lease_holder.fence();
        lease_holder.extend(Instant::now(), lease);
        assert!(!lease_holder.is_valid());
        assert_eq!(lease_holder.writer().writer_epoch, WriterEpoch(4));
    }
}
