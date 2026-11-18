// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    sync::{Arc, OnceLock},
    time::Instant,
};

use domain::storage::{PartitionManifest, PartitionManifestPointer};
use kernel::{Error, Result};
use moka::future::Cache;
use object_store::{ObjectStoreExt, path::Path};
use prometheus::{Histogram, IntCounter};
use signals::metrics::{register_histogram, register_int_counter};

use super::codec;
use crate::infra::storage::{filters::Filters, object_reader::ObjectReader};

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
    /// them is published and never change, so an answer is kept, absence included. Callers that
    /// miss the same key at once share one read: each would otherwise hold its own decoded copy.
    pub async fn blooms(&self, key: &str) -> Result<Option<Arc<Filters>>> {
        self.blooms
            .try_get_with(key.to_owned(), async {
                match self.objects.origin().get(&Path::from(key)).await {
                    Ok(result) => {
                        let bytes = result.bytes().await.map_err(|error| {
                            Error::internal(format!("collect partition blooms: {error}"))
                        })?;

                        Ok(Some(Arc::new(Filters::decode(&bytes)?)))
                    }
                    Err(object_store::Error::NotFound { .. }) => Ok(None),
                    Err(error) => Err(Error::internal(format!("read partition blooms: {error}"))),
                }
            })
            .await
            .map_err(|error| Error::internal(error.to_string()))
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

#[cfg(test)]
mod tests {
    use std::{
        fmt,
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    use futures::stream::BoxStream;
    use object_store::{
        CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
        PutMultipartOptions, PutOptions, PutPayload, PutResult, Result as ObjectStoreResult,
        memory::InMemory, path::Path as ObjPath,
    };
    use settings::ObjectCacheSettings;

    use super::*;

    /// 每次读之前等一会儿、并数一共读了几次的内存对象存储。
    #[derive(Debug, Default)]
    struct Slow {
        inner: InMemory,
        gets: AtomicUsize,
    }

    impl fmt::Display for Slow {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "Slow")
        }
    }

    #[async_trait::async_trait]
    impl ObjectStore for Slow {
        async fn put_opts(
            &self,
            location: &ObjPath,
            payload: PutPayload,
            options: PutOptions,
        ) -> ObjectStoreResult<PutResult> {
            self.inner.put_opts(location, payload, options).await
        }

        async fn put_multipart_opts(
            &self,
            location: &ObjPath,
            options: PutMultipartOptions,
        ) -> ObjectStoreResult<Box<dyn MultipartUpload>> {
            self.inner.put_multipart_opts(location, options).await
        }

        async fn get_opts(
            &self,
            location: &ObjPath,
            options: GetOptions,
        ) -> ObjectStoreResult<GetResult> {
            self.gets.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(20)).await;
            self.inner.get_opts(location, options).await
        }

        fn delete_stream(
            &self,
            locations: BoxStream<'static, ObjectStoreResult<ObjPath>>,
        ) -> BoxStream<'static, ObjectStoreResult<ObjPath>> {
            self.inner.delete_stream(locations)
        }

        fn list(
            &self,
            prefix: Option<&ObjPath>,
        ) -> BoxStream<'static, ObjectStoreResult<ObjectMeta>> {
            self.inner.list(prefix)
        }

        async fn list_with_delimiter(
            &self,
            prefix: Option<&ObjPath>,
        ) -> ObjectStoreResult<ListResult> {
            self.inner.list_with_delimiter(prefix).await
        }

        async fn copy_opts(
            &self,
            from: &ObjPath,
            to: &ObjPath,
            options: CopyOptions,
        ) -> ObjectStoreResult<()> {
            self.inner.copy_opts(from, to, options).await
        }
    }

    /// 几个查询同时没命中同一个过滤器对象：只读一次，大家拿到同一份。
    #[tokio::test]
    async fn misses_of_the_same_filters_at_once_read_them_once() {
        let store = Arc::new(Slow::default());

        store
            .put(
                &ObjPath::from("manifests/p.bloom"),
                PutPayload::from(Filters::default().encode().unwrap()),
            )
            .await
            .unwrap();

        let objects =
            ObjectReader::build(store.clone(), "local", &ObjectCacheSettings::default()).unwrap();
        let reader = PartitionManifestReader::new(objects, 1024 * 1024);

        let loads =
            futures::future::join_all((0..8).map(|_| reader.blooms("manifests/p.bloom"))).await;

        let first = loads[0].as_ref().unwrap().clone().unwrap();

        for load in &loads {
            assert!(Arc::ptr_eq(
                load.as_ref().unwrap().as_ref().unwrap(),
                &first
            ));
        }

        assert_eq!(store.gets.load(Ordering::SeqCst), 1);

        // 没有的对象也只问一次，之后由缓存回答。
        assert!(reader.blooms("manifests/q.bloom").await.unwrap().is_none());
        assert!(reader.blooms("manifests/q.bloom").await.unwrap().is_none());
        assert_eq!(store.gets.load(Ordering::SeqCst), 2);
    }
}
