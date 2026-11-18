// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Minute counts read back for queries. An object never changes once written and is named by
//! its segment or its partition's manifest before they are published, so a read is kept, absence
//! included; queries that miss the same key at once share one read.

use std::{future::Future, sync::Arc};

use futures::{StreamExt, stream};
use kernel::{Error, Result};
use moka::future::Cache;
use object_store::{ObjectStore, ObjectStoreExt, path::Path};

use super::{MinuteCounts, PartitionCounts};

/// Bytes of decoded counts kept, of segments' objects and of partitions' each.
const CACHE_BYTES: u64 = 64 * 1024 * 1024;

/// Objects read at once by [`MinuteCountsCache::get_many`].
const CONCURRENT_READS: usize = 16;

/// Where a segment's minute counts are kept: its own object and, once its partition is sealed,
/// the object of the partition, which holds them too and is read first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinuteCountsKeys {
    pub own: String,
    pub sealed: Option<String>,
}

impl MinuteCountsKeys {
    /// Counts kept in the segment's own object alone.
    pub fn own(key: impl Into<String>) -> Self {
        Self {
            own: key.into(),
            sealed: None,
        }
    }
}

#[derive(Clone)]
pub struct MinuteCountsCache {
    segments: Cache<String, Option<Arc<MinuteCounts>>>,
    partitions: Cache<String, Option<Arc<PartitionCounts>>>,
}

impl Default for MinuteCountsCache {
    fn default() -> Self {
        let segments = Cache::builder()
            .max_capacity(CACHE_BYTES)
            .weigher(|_key: &String, value: &Option<Arc<MinuteCounts>>| {
                weight(value.as_ref().map(|counts| counts.weight()))
            })
            .build();

        let partitions = Cache::builder()
            .max_capacity(CACHE_BYTES)
            .weigher(|_key: &String, value: &Option<Arc<PartitionCounts>>| {
                weight(value.as_ref().map(|counts| counts.weight()))
            })
            .build();

        Self {
            segments,
            partitions,
        }
    }
}

impl MinuteCountsCache {
    /// The counts of `segment`, from its sealed partition's object when that holds them, else
    /// from its own; `None` when neither can be read, and the file is then read as before.
    pub async fn get(
        &self,
        store: &dyn ObjectStore,
        segment: &str,
        keys: &MinuteCountsKeys,
    ) -> Option<Arc<MinuteCounts>> {
        if let Some(sealed) = &keys.sealed {
            let partition = kept(
                &self.partitions,
                sealed,
                read(store, sealed, PartitionCounts::decode),
            )
            .await;

            if let Some(counts) = partition.and_then(|partition| partition.get(segment).cloned()) {
                return Some(counts);
            }
        }

        kept(
            &self.segments,
            &keys.own,
            read(store, &keys.own, MinuteCounts::decode),
        )
        .await
    }

    /// [`Self::get`] for each `(segment, keys)` of `wanted`, in order, at most
    /// [`CONCURRENT_READS`] at once; `None` for none wanted.
    pub async fn get_many(
        &self,
        store: &dyn ObjectStore,
        wanted: Vec<Option<(String, MinuteCountsKeys)>>,
    ) -> Vec<Option<Arc<MinuteCounts>>> {
        stream::iter(wanted)
            .map(|wanted| async move {
                let (segment, keys) = wanted?;
                self.get(store, &segment, &keys).await
            })
            .buffered(CONCURRENT_READS)
            .collect()
            .await
    }
}

fn weight(bytes: Option<usize>) -> u32 {
    bytes.map_or(1, |bytes| u32::try_from(bytes).unwrap_or(u32::MAX).max(1))
}

/// The value kept for `key`, or what `reading` reads; a failed read is logged and not kept.
async fn kept<T>(
    cache: &Cache<String, Option<Arc<T>>>,
    key: &str,
    reading: impl Future<Output = Result<Option<Arc<T>>>>,
) -> Option<Arc<T>>
where
    T: Send + Sync + 'static,
{
    match cache.try_get_with(key.to_owned(), reading).await {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(object_key = key, %error, "minute counts unreadable");
            None
        }
    }
}

async fn read<T>(
    store: &dyn ObjectStore,
    key: &str,
    decode: fn(&[u8]) -> Result<T>,
) -> Result<Option<Arc<T>>> {
    match store.get(&Path::from(key)).await {
        Ok(result) => {
            let bytes = result
                .bytes()
                .await
                .map_err(|error| Error::internal(format!("collect minute counts: {error}")))?;

            Ok(Some(Arc::new(decode(&bytes)?)))
        }
        Err(object_store::Error::NotFound { .. }) => Ok(None),
        Err(error) => Err(Error::internal(format!("read minute counts: {error}"))),
    }
}

#[cfg(test)]
mod tests {
    use object_store::{PutPayload, memory::InMemory};

    use super::*;

    fn counts(rows: u64) -> MinuteCounts {
        MinuteCounts {
            fields: vec!["service".into()],
            values: vec![vec!["api".into()]],
            minutes: vec![1],
            ids: vec![vec![0]],
            rows: vec![rows],
        }
    }

    async fn put(store: &InMemory, key: &str, bytes: bytes::Bytes) {
        store
            .put(&Path::from(key), PutPayload::from(bytes))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn counts_are_read_once_and_a_missing_object_is_none() {
        let store = InMemory::new();
        put(&store, "counts", counts(4).encode().unwrap()).await;

        let cache = MinuteCountsCache::default();
        let keys = MinuteCountsKeys::own("counts");

        assert_eq!(
            cache.get(&store, "s1", &keys).await.as_deref(),
            Some(&counts(4))
        );

        store.delete(&Path::from("counts")).await.unwrap();

        assert!(cache.get(&store, "s1", &keys).await.is_some());

        assert!(
            cache
                .get(&store, "s1", &MinuteCountsKeys::own("missing"))
                .await
                .is_none()
        );
    }

    /// A sealed partition's object answers for the segments it holds; the others, and every
    /// segment of a partition whose object is missing, are read from their own objects.
    #[tokio::test]
    async fn a_sealed_partition_answers_first_and_its_segments_own_objects_after() {
        let store = InMemory::new();
        let mut partition = PartitionCounts::default();
        partition.insert("s1".into(), Arc::new(counts(1)));

        put(&store, "p.minute_counts", partition.encode().unwrap()).await;
        put(&store, "s1.counts", counts(10).encode().unwrap()).await;
        put(&store, "s2.counts", counts(20).encode().unwrap()).await;

        let cache = MinuteCountsCache::default();

        let keys = |own: &str, sealed: &str| MinuteCountsKeys {
            own: own.into(),
            sealed: Some(sealed.into()),
        };

        let found = cache
            .get_many(
                &store,
                vec![
                    Some(("s1".into(), keys("s1.counts", "p.minute_counts"))),
                    Some(("s2".into(), keys("s2.counts", "p.minute_counts"))),
                    Some(("s1".into(), keys("s1.counts", "q.minute_counts"))),
                    None,
                ],
            )
            .await
            .into_iter()
            .map(|counts| counts.map(|counts| counts.total()))
            .collect::<Vec<_>>();

        assert_eq!(found, [Some(1), Some(20), Some(10), None]);
    }
}
