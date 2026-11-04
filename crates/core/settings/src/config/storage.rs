// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[storage]` Catalog lifecycle, index, GC, and reconciliation settings.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageSettings {
    #[serde(default)]
    pub catalog: CatalogSettings,
    #[serde(default)]
    pub index: IndexMaintenanceSettings,
    #[serde(default)]
    pub gc: GarbageCollectionSettings,
    #[serde(default)]
    pub reconciler: ReconcilerSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogSettings {
    /// Seal partitions whose end is older than this many hours. Zero disables sealing. A sealed
    /// partition gets its manifest and the Bloom filters of its exact fields, and is no longer
    /// compacted: two days leave time for the hours to be merged into their day.
    #[serde(default = "default_seal_after_hours")]
    pub seal_after_hours: u32,
    /// 清单缓存的内存大小，单位 MiB。
    #[serde(default = "default_manifest_cache_size")]
    pub manifest_cache_size: u64,
    #[serde(default = "default_catalog_interval_secs")]
    pub interval_secs: u32,
    #[serde(default = "default_catalog_batch_size")]
    pub batch_size: u32,
}

fn default_seal_after_hours() -> u32 {
    48
}

fn default_manifest_cache_size() -> u64 {
    256
}

fn default_catalog_interval_secs() -> u32 {
    300
}

fn default_catalog_batch_size() -> u32 {
    64
}

impl Default for CatalogSettings {
    fn default() -> Self {
        Self {
            seal_after_hours: default_seal_after_hours(),
            manifest_cache_size: default_manifest_cache_size(),
            interval_secs: default_catalog_interval_secs(),
            batch_size: default_catalog_batch_size(),
        }
    }
}

impl CatalogSettings {
    pub fn manifest_cache_bytes(&self) -> u64 {
        self.manifest_cache_size.saturating_mul(1024 * 1024)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMaintenanceSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_index_interval_secs")]
    pub interval_secs: u32,
    #[serde(default = "default_index_batch_size")]
    pub batch_size: u32,
    #[serde(default = "default_index_concurrency")]
    pub max_concurrency: usize,
    #[serde(default = "default_index_retry_after_secs")]
    pub retry_after_secs: u32,
}

fn default_index_interval_secs() -> u32 {
    30
}

fn default_index_batch_size() -> u32 {
    64
}

fn default_index_concurrency() -> usize {
    2
}

fn default_index_retry_after_secs() -> u32 {
    300
}

impl Default for IndexMaintenanceSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: default_index_interval_secs(),
            batch_size: default_index_batch_size(),
            max_concurrency: default_index_concurrency(),
            retry_after_secs: default_index_retry_after_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GarbageCollectionSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_gc_interval_secs")]
    pub interval_secs: u32,
    #[serde(default = "default_gc_grace_period_secs")]
    pub grace_period_secs: u32,
    #[serde(default = "default_gc_lease_secs")]
    pub lease_secs: u32,
    #[serde(default = "default_gc_batch_size")]
    pub batch_size: u32,
    #[serde(default = "default_gc_concurrency")]
    pub max_concurrency: usize,
}

fn default_gc_interval_secs() -> u32 {
    30
}

fn default_gc_grace_period_secs() -> u32 {
    3600
}

fn default_gc_lease_secs() -> u32 {
    900
}

fn default_gc_batch_size() -> u32 {
    128
}

fn default_gc_concurrency() -> usize {
    4
}

impl Default for GarbageCollectionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: default_gc_interval_secs(),
            grace_period_secs: default_gc_grace_period_secs(),
            lease_secs: default_gc_lease_secs(),
            batch_size: default_gc_batch_size(),
            max_concurrency: default_gc_concurrency(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcilerSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_reconciler_interval_secs")]
    pub interval_secs: u32,
    #[serde(default = "default_reconciler_batch_size")]
    pub batch_size: u32,
    #[serde(default = "default_reconciler_orphan_grace_secs")]
    pub orphan_grace_secs: u32,
    #[serde(default = "default_reconciler_verify_checksums")]
    pub verify_checksums: bool,
}

fn default_true() -> bool {
    true
}

fn default_reconciler_interval_secs() -> u32 {
    300
}

fn default_reconciler_batch_size() -> u32 {
    256
}

fn default_reconciler_orphan_grace_secs() -> u32 {
    24 * 3600
}

fn default_reconciler_verify_checksums() -> bool {
    true
}

impl Default for ReconcilerSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: default_reconciler_interval_secs(),
            batch_size: default_reconciler_batch_size(),
            orphan_grace_secs: default_reconciler_orphan_grace_secs(),
            verify_checksums: default_reconciler_verify_checksums(),
        }
    }
}
