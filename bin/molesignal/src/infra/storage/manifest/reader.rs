// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    sync::{Arc, OnceLock},
    time::Instant,
};

use moka::future::Cache;
use object_store::{ObjectStoreExt, path::Path};
use prometheus::{Histogram, IntCounter};

use super::codec;
use crate::{
    domain::storage::{PartitionManifest, PartitionManifestPointer},
    infra::storage::{filters::Filters, object_reader::ObjectReader},
    shared::{
        Error, Result,
        metrics::{register_histogram, register_int_counter},
    },
};

struct CachedManifest {
    manifest: Arc<PartitionManifest>,
    weight: u32,
}

pub struct PartitionManifestReader {
    objects: Arc<ObjectReader>,
    cache: Cache<String, Arc<CachedManifest>>,
    /// Filter objects by key: a partition's beside its manifest, or a segment's own filter
    /// artifact. `None` for a key with no object.
    blooms: Cache<String, Option<Arc<Filters>>>,
}

impl PartitionManifestReader {
    pub fn new(objects: Arc<ObjectReader>, max_cache_bytes: u64) -> Self {
        let cache = Cache::builder()
            .max_capacity(max_cache_bytes)
            .weigher(|_key: &String, value: &Arc<CachedManifest>| value.weight)
            .build();

        let blooms = Cache::builder()
            .max_capacity(max_cache_bytes)
            .weigher(|_key: &String, value: &Option<Arc<Filters>>| {
                value.as_ref().map_or(1, |blooms| {
                    u32::try_from(blooms.weight()).unwrap_or(u32::MAX).max(1)
                })
            })
            .build();

        Self {
            objects,
            cache,
            blooms,
        }
    }

    /// The filters stored at `key`, beside a manifest or as a segment's filter artifact, or `None`
    /// when there is no such object. Filters are written before the manifest or segment that names
    /// them is published and never change, so an answer is kept, absence included.
    pub async fn blooms(&self, key: &str) -> Result<Option<Arc<Filters>>> {
        if let Some(cached) = self.blooms.get(key).await {
            return Ok(cached);
        }

        let blooms = match self.objects.origin().get(&Path::from(key)).await {
            Ok(result) => {
                let bytes = result.bytes().await.map_err(|error| {
                    Error::internal(format!("collect partition blooms: {error}"))
                })?;

                Some(Arc::new(Filters::decode(&bytes)?))
            }
            Err(object_store::Error::NotFound { .. }) => None,
            Err(error) => {
                return Err(Error::internal(format!("read partition blooms: {error}")));
            }
        };

        self.blooms.insert(key.to_owned(), blooms.clone()).await;
        Ok(blooms)
    }

    pub async fn load(&self, pointer: &PartitionManifestPointer) -> Result<Arc<PartitionManifest>> {
        let key = pointer.cache_key();

        if let Some(cached) = self.cache.get(&key).await {
            return Ok(cached.manifest.clone());
        }

        let started = Instant::now();
        self.objects
            .register(&pointer.organization_id, &pointer.object)?;

        let bytes = self
            .objects
            .store()
            .get(&Path::from(pointer.object.key.as_str()))
            .await
            .map_err(|error| Error::internal(format!("read partition manifest: {error}")))?
            .bytes()
            .await
            .map_err(|error| Error::internal(format!("collect partition manifest: {error}")))?;

        if let Some(expected) = pointer.object.checksum.as_str().strip_prefix("b3:")
            && blake3::hash(&bytes).to_hex().as_str() != expected
        {
            corrupt_total().inc();

            return Err(Error::internal(format!(
                "partition manifest {} checksum mismatch",
                pointer.object.key
            )));
        }

        let manifest = match codec::decode(bytes.clone()) {
            Ok(manifest) => manifest,
            Err(error) => {
                corrupt_total().inc();
                return Err(error);
            }
        };

        if manifest.organization_id != pointer.organization_id
            || manifest.dataset_id != pointer.dataset_id
            || manifest.partition != pointer.partition
            || manifest.generation != pointer.generation
            || manifest.segments.len() != pointer.segment_count as usize
        {
            corrupt_total().inc();

            return Err(Error::internal(format!(
                "partition manifest {} identity does not match Catalog pointer",
                pointer.object.key
            )));
        }

        let manifest = Arc::new(manifest);

        self.cache
            .insert(
                key,
                Arc::new(CachedManifest {
                    manifest: manifest.clone(),
                    weight: bytes.len().min(u32::MAX as usize) as u32,
                }),
            )
            .await;

        load_duration().observe(started.elapsed().as_secs_f64());
        Ok(manifest)
    }
}

fn load_duration() -> &'static Histogram {
    static METRIC: OnceLock<Histogram> = OnceLock::new();

    METRIC.get_or_init(|| {
        register_histogram(
            "partition_manifest_load_duration_seconds",
            "Partition manifest fetch and decode latency",
            vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0],
        )
    })
}

fn corrupt_total() -> &'static IntCounter {
    static METRIC: OnceLock<IntCounter> = OnceLock::new();

    METRIC.get_or_init(|| {
        register_int_counter(
            "partition_manifest_corrupt_total",
            "Partition manifests rejected for corruption or identity mismatch",
        )
    })
}
