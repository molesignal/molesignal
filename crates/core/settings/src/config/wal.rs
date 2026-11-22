// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[wal]`: the write-ahead log in object storage.
//!
//! A write is acknowledged once its group's WAL object is in the object store; nothing a node
//! acknowledged lives only on its disk. Each process holds a writer lease in Postgres and stops
//! acknowledging before it can lapse; once it has lapsed, another node takes the writer's
//! unflushed objects over.

use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalSettings {
    /// The writer lease in seconds. A node that stops renewing it is taken over this long after
    /// its last renewal (plus up to one `takeover_interval_secs`). Minimum 3.
    #[serde(default = "default_lease_secs")]
    pub lease_secs: u32,
    /// The most one WAL object holds, in MiB. Writes that arrive while the previous object of a
    /// dataset is being written are grouped into the next one, up to this size.
    #[serde(default = "default_max_group_size")]
    pub max_group_size: u32,
    /// How long one object write may take, in seconds.
    #[serde(default = "default_put_timeout_secs")]
    pub put_timeout_secs: u32,
    /// Attempts per object write before the writes in it are refused.
    #[serde(default = "default_put_attempts")]
    pub put_attempts: u32,
    /// How often a node looks for lapsed writers to take over, in seconds.
    #[serde(default = "default_takeover_interval_secs")]
    pub takeover_interval_secs: u32,
    /// Seal WAL objects with the cipher root key (KEK).
    #[serde(default)]
    pub encrypt: bool,
    /// The most a takeover holds in memory before it publishes a segment, in MiB. 0 counts as 1.
    #[serde(default = "default_max_takeover_size")]
    pub max_takeover_size: u32,
}

impl WalSettings {
    pub fn lease(&self) -> Duration {
        Duration::from_secs(u64::from(self.lease_secs.max(3)))
    }

    pub fn max_group_bytes(&self) -> usize {
        (self.max_group_size.max(1) as usize).saturating_mul(1024 * 1024)
    }

    pub fn put_timeout(&self) -> Duration {
        Duration::from_secs(u64::from(self.put_timeout_secs.max(1)))
    }

    pub fn takeover_interval(&self) -> Duration {
        Duration::from_secs(u64::from(self.takeover_interval_secs.max(1)))
    }

    pub fn max_takeover_bytes(&self) -> usize {
        (self.max_takeover_size.max(1) as usize).saturating_mul(1024 * 1024)
    }
}

fn default_lease_secs() -> u32 {
    15
}

fn default_max_group_size() -> u32 {
    8
}

fn default_put_timeout_secs() -> u32 {
    10
}

fn default_put_attempts() -> u32 {
    3
}

fn default_takeover_interval_secs() -> u32 {
    5
}

fn default_max_takeover_size() -> u32 {
    512
}

impl Default for WalSettings {
    fn default() -> Self {
        Self {
            lease_secs: default_lease_secs(),
            max_group_size: default_max_group_size(),
            put_timeout_secs: default_put_timeout_secs(),
            put_attempts: default_put_attempts(),
            takeover_interval_secs: default_takeover_interval_secs(),
            encrypt: false,
            max_takeover_size: default_max_takeover_size(),
        }
    }
}
