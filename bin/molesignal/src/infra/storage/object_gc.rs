// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    sync::{Arc, OnceLock},
    time::Instant,
};

use futures::{StreamExt, stream};
use object_store::{ObjectStore, ObjectStoreExt, path::Path};
use prometheus::{Histogram, IntCounter};

use crate::{
    config::GarbageCollectionSettings,
    domain::storage::{FileCatalog, GcQueueEntry, OrganizationScope},
    shared::{Result, time::TimestampMicros},
};

pub struct ObjectGcWorker {
    catalog: Arc<dyn FileCatalog>,
    object_store: Arc<dyn ObjectStore>,
    settings: GarbageCollectionSettings,
}

impl ObjectGcWorker {
    pub fn new(
        catalog: Arc<dyn FileCatalog>,
        object_store: Arc<dyn ObjectStore>,
        settings: GarbageCollectionSettings,
    ) -> Self {
        Self {
            catalog,
            object_store,
            settings,
        }
    }

    pub fn settings(&self) -> &GarbageCollectionSettings {
        &self.settings
    }

    pub async fn run_scope(&self, scope: OrganizationScope) -> Result<usize> {
        if !self.settings.enabled {
            return Ok(0);
        }
        let now = TimestampMicros::now().0;
        let lease_expired = now.saturating_sub(i64::from(self.settings.lease_secs) * 1_000_000);
        let entries = self
            .catalog
            .claim_gc_entries(&scope, now, lease_expired, self.settings.batch_size)
            .await?;
        let deleted = stream::iter(entries)
            .map(|entry| self.delete_one(&scope, entry))
            .buffer_unordered(self.settings.max_concurrency.max(1))
            .fold(0_usize, |count, result| async move {
                match result {
                    Ok(true) => count + 1,
                    Ok(false) => count,
                    Err(error) => {
                        tracing::warn!(error = %error, "delayed object GC task failed");
                        count
                    }
                }
            })
            .await;
        Ok(deleted)
    }

    async fn delete_one(&self, scope: &OrganizationScope, entry: GcQueueEntry) -> Result<bool> {
        let started = Instant::now();
        if !self.catalog.gc_entry_is_safe(scope, &entry).await? {
            unsafe_total().inc();
            self.retry(
                scope,
                &entry,
                "object regained or retained a Catalog reference",
            )
            .await?;
            return Ok(false);
        }
        let path = Path::from(entry.object_key.as_str());
        match self.object_store.delete(&path).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => {
                self.catalog
                    .complete_gc_entry(scope, &entry.object_key)
                    .await?;
                deleted_total().inc();
                delete_duration().observe(started.elapsed().as_secs_f64());
                Ok(true)
            }
            Err(error) => {
                failed_total().inc();
                self.retry(scope, &entry, &error.to_string()).await?;
                Err(crate::shared::Error::internal(format!(
                    "delete GC object: {error}"
                )))
            }
        }
    }

    async fn retry(
        &self,
        scope: &OrganizationScope,
        entry: &GcQueueEntry,
        error: &str,
    ) -> Result<()> {
        let exponent = entry.attempt_count.min(10);
        let delay_secs = 5_u64.saturating_mul(1_u64 << exponent).min(3600);
        let not_before = TimestampMicros::now()
            .0
            .saturating_add((delay_secs as i64) * 1_000_000);
        let error: String = error.chars().take(1024).collect();
        self.catalog
            .retry_gc_entry(scope, &entry.object_key, &error, not_before)
            .await
    }
}

fn deleted_total() -> &'static IntCounter {
    static METRIC: OnceLock<IntCounter> = OnceLock::new();
    METRIC.get_or_init(|| {
        crate::shared::metrics::register_int_counter(
            "object_gc_deleted_total",
            "Objects deleted after delayed GC safety checks",
        )
    })
}

fn failed_total() -> &'static IntCounter {
    static METRIC: OnceLock<IntCounter> = OnceLock::new();
    METRIC.get_or_init(|| {
        crate::shared::metrics::register_int_counter(
            "object_gc_failed_total",
            "Delayed object GC delete failures",
        )
    })
}

fn unsafe_total() -> &'static IntCounter {
    static METRIC: OnceLock<IntCounter> = OnceLock::new();
    METRIC.get_or_init(|| {
        crate::shared::metrics::register_int_counter(
            "object_gc_safety_rejections_total",
            "GC claims rejected by final Catalog reference checks",
        )
    })
}

fn delete_duration() -> &'static Histogram {
    static METRIC: OnceLock<Histogram> = OnceLock::new();
    METRIC.get_or_init(|| {
        crate::shared::metrics::register_histogram(
            "object_gc_delete_duration_seconds",
            "Delayed object deletion latency",
            vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0],
        )
    })
}
