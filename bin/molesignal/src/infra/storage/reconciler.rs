// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};

use futures::{StreamExt, TryStreamExt};
use object_store::{ObjectStore, ObjectStoreExt, path::Path};
use parking_lot::Mutex;
use prometheus::IntCounterVec;

use crate::{
    config::ReconcilerSettings,
    domain::storage::{
        ArtifactRole, ArtifactState, CatalogInvariant, CatalogObject, FileCatalog, ObjectKey,
        OrganizationScope,
    },
    infra::storage::{
        layout::StorageLayout, manifest::PartitionManifestReader, object_reader::ObjectReader,
    },
    shared::{Result, time::TimestampMicros},
};

pub struct StorageReconciler {
    catalog: Arc<dyn FileCatalog>,
    object_reader: Arc<ObjectReader>,
    manifests: Arc<PartitionManifestReader>,
    origin: Arc<dyn ObjectStore>,
    settings: ReconcilerSettings,
    catalog_cursors: Mutex<HashMap<String, ObjectKey>>,
    orphan_cursors: Mutex<HashMap<(String, String), ObjectKey>>,
}

impl StorageReconciler {
    pub fn new(
        catalog: Arc<dyn FileCatalog>,
        object_reader: Arc<ObjectReader>,
        manifests: Arc<PartitionManifestReader>,
        origin: Arc<dyn ObjectStore>,
        settings: ReconcilerSettings,
    ) -> Self {
        Self {
            catalog,
            object_reader,
            manifests,
            origin,
            settings,
            catalog_cursors: Mutex::new(HashMap::new()),
            orphan_cursors: Mutex::new(HashMap::new()),
        }
    }

    pub fn settings(&self) -> &ReconcilerSettings {
        &self.settings
    }

    pub async fn run_scope(&self, scope: OrganizationScope) -> Result<usize> {
        if !self.settings.enabled {
            return Ok(0);
        }
        let now = TimestampMicros::now().0;
        let stuck_after_secs = u64::from(self.settings.interval_secs)
            .saturating_mul(3)
            .max(300);
        let mut issues = 0_usize;
        for invariant in self
            .catalog
            .catalog_invariant_issues(
                &scope,
                now.saturating_sub(
                    i64::try_from(stuck_after_secs)
                        .unwrap_or(i64::MAX)
                        .saturating_mul(1_000_000),
                ),
            )
            .await?
        {
            let (kind, reason) = invariant_labels(invariant.invariant);
            issue_by(kind, reason, invariant.occurrences);
            issues =
                issues.saturating_add(usize::try_from(invariant.occurrences).unwrap_or(usize::MAX));
        }
        let cursor = self
            .catalog_cursors
            .lock()
            .get(scope.organization_id.as_str())
            .cloned();
        let objects = self
            .catalog
            .catalog_objects(&scope, cursor.as_ref(), self.settings.batch_size)
            .await?;
        for object in &objects {
            issues += self.check_catalog_object(&scope, object).await?;
        }
        self.advance_catalog_cursor(&scope, &objects);
        issues += self.scan_orphans(&scope).await?;
        Ok(issues)
    }

    async fn check_catalog_object(
        &self,
        scope: &OrganizationScope,
        catalog_object: &CatalogObject,
    ) -> Result<usize> {
        if let Some(state) = catalog_object.state
            && state != ArtifactState::Ready
        {
            if catalog_object.role == Some(ArtifactRole::PrimaryData) {
                issue("primary", "not_ready");
                return Ok(1);
            }
            if catalog_object.role == Some(ArtifactRole::Index) {
                issue(
                    "index",
                    if state == ArtifactState::Pending {
                        "pending"
                    } else {
                        "failed"
                    },
                );
                return Ok(1);
            }
            return Ok(0);
        }
        let path = Path::from(catalog_object.object.key.as_str());
        let actual = match self.origin.head(&path).await {
            Ok(actual) => actual,
            Err(object_store::Error::NotFound { .. }) => {
                issue(object_kind(catalog_object), "missing");
                return Ok(1);
            }
            Err(error) => {
                tracing::warn!(error = %error, "reconciler object HEAD failed");
                issue(object_kind(catalog_object), "head_failed");
                return Ok(1);
            }
        };
        if actual.size != catalog_object.object.size_bytes {
            issue(object_kind(catalog_object), "size_mismatch");
            return Ok(1);
        }
        if let Some(pointer) = &catalog_object.manifest {
            if self.manifests.load(pointer).await.is_err() {
                issue("manifest", "corrupt");
                return Ok(1);
            }
            return Ok(0);
        }
        if !self.settings.verify_checksums {
            return Ok(0);
        }
        self.object_reader
            .register(&scope.organization_id, &catalog_object.object)?;
        let bytes = match self.object_reader.store().get(&path).await {
            Ok(result) => result
                .bytes()
                .await
                .map_err(|error| crate::shared::Error::internal(error.to_string()))?,
            Err(error) => {
                tracing::warn!(error = %error, "reconciler object read failed");
                issue(object_kind(catalog_object), "read_failed");
                return Ok(1);
            }
        };
        if let Some(expected) = catalog_object.object.checksum.as_str().strip_prefix("b3:")
            && blake3::hash(&bytes).to_hex().as_str() != expected
        {
            issue(object_kind(catalog_object), "checksum_mismatch");
            return Ok(1);
        }
        Ok(0)
    }

    async fn scan_orphans(&self, scope: &OrganizationScope) -> Result<usize> {
        let mut found = 0;
        let per_prefix = (self.settings.batch_size.max(2) / 2) as usize;
        for prefix in StorageLayout::catalog_scan_prefixes_for(&scope.organization_id) {
            let prefix_path = Path::from(prefix.as_str());
            let cursor_key = (scope.organization_id.as_str().to_owned(), prefix.clone());
            let cursor = self.orphan_cursors.lock().get(&cursor_key).cloned();
            let stream = match cursor.as_ref() {
                Some(cursor) => self
                    .origin
                    .list_with_offset(Some(&prefix_path), &Path::from(cursor.as_str())),
                None => self.origin.list(Some(&prefix_path)),
            };
            let objects = stream
                .take(per_prefix)
                .try_collect::<Vec<_>>()
                .await
                .map_err(|error| {
                    crate::shared::Error::internal(format!("list catalog objects: {error}"))
                })?;
            for object in &objects {
                let key = ObjectKey::from_string(object.location.as_ref());
                if !self.catalog.object_is_referenced(scope, &key).await? {
                    let not_before = TimestampMicros::now()
                        .0
                        .saturating_add(i64::from(self.settings.orphan_grace_secs) * 1_000_000);
                    self.catalog.enqueue_orphan(scope, &key, not_before).await?;
                    issue("orphan", "queued");
                    found += 1;
                }
            }
            let mut cursors = self.orphan_cursors.lock();
            if objects.len() < per_prefix {
                cursors.remove(&cursor_key);
            } else if let Some(last) = objects.last() {
                cursors.insert(cursor_key, ObjectKey::from_string(last.location.as_ref()));
            }
        }
        Ok(found)
    }

    fn advance_catalog_cursor(&self, scope: &OrganizationScope, objects: &[CatalogObject]) {
        let mut cursors = self.catalog_cursors.lock();
        if objects.len() < self.settings.batch_size.max(1) as usize {
            cursors.remove(scope.organization_id.as_str());
        } else if let Some(last) = objects.last() {
            cursors.insert(
                scope.organization_id.as_str().to_owned(),
                last.object.key.clone(),
            );
        }
    }
}

fn object_kind(object: &CatalogObject) -> &'static str {
    match object.role {
        Some(ArtifactRole::PrimaryData) => "primary",
        Some(ArtifactRole::Index) => "index",
        Some(ArtifactRole::Statistics) => "statistics",
        Some(ArtifactRole::Dictionary) => "dictionary",
        None => "manifest",
    }
}

fn issue(kind: &'static str, reason: &'static str) {
    issue_by(kind, reason, 1);
}

fn issue_by(kind: &'static str, reason: &'static str, occurrences: u64) {
    issues_total()
        .with_label_values(&[kind, reason])
        .inc_by(occurrences);
}

fn invariant_labels(invariant: CatalogInvariant) -> (&'static str, &'static str) {
    match invariant {
        CatalogInvariant::ActiveSegmentMissingReadyPrimary => ("primary", "missing_ready"),
        CatalogInvariant::WalCheckpointFlushMismatch => ("wal_checkpoint", "flush_mismatch"),
        CatalogInvariant::GcStuck => ("gc", "stuck"),
        CatalogInvariant::StaleIndexSource => ("index", "stale_source"),
    }
}

fn issues_total() -> &'static IntCounterVec {
    static METRIC: OnceLock<IntCounterVec> = OnceLock::new();
    METRIC.get_or_init(|| {
        crate::shared::metrics::register_int_counter_vec(
            "storage_reconciler_issues_total",
            "Storage reconciliation issues by bounded kind and reason",
            &["kind", "reason"],
        )
    })
}
