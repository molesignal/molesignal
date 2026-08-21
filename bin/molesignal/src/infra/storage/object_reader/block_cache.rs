// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    collections::{BTreeMap, HashMap},
    io,
    ops::Range,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use bytes::{BufMut, Bytes, BytesMut};
use moka::sync::Cache;
use object_store::{GetOptions, ObjectStore, path::Path as ObjectPath};
use parking_lot::Mutex;
use tokio::{io::AsyncWriteExt, sync::Semaphore};

use super::{RegisteredObject, metrics};
use crate::{
    config::ObjectCacheSettings,
    shared::{Error, Result},
};

mod codec;
mod disk;

use codec::{decode_block, encode_block, verify_object_checksum};
use disk::available_bytes;

#[derive(Debug)]
struct Entry {
    file_bytes: u64,
    lru_token: u64,
}

#[derive(Debug, Default)]
struct Index {
    entries: HashMap<String, Entry>,
    lru: BTreeMap<u64, String>,
    total_bytes: u64,
    next_token: u64,
}

impl Index {
    fn touch(&mut self, key: &str) {
        self.next_token = self.next_token.saturating_add(1);
        let token = self.next_token;
        if let Some(entry) = self.entries.get_mut(key) {
            self.lru.remove(&entry.lru_token);
            entry.lru_token = token;
            self.lru.insert(token, key.to_owned());
        }
    }

    fn insert(&mut self, key: String, file_bytes: u64) {
        self.remove(&key);
        self.next_token = self.next_token.saturating_add(1);
        let token = self.next_token;
        self.total_bytes = self.total_bytes.saturating_add(file_bytes);
        self.lru.insert(token, key.clone());
        self.entries.insert(
            key,
            Entry {
                file_bytes,
                lru_token: token,
            },
        );
    }

    fn remove(&mut self, key: &str) {
        if let Some(entry) = self.entries.remove(key) {
            self.lru.remove(&entry.lru_token);
            self.total_bytes = self.total_bytes.saturating_sub(entry.file_bytes);
        }
    }

    fn pop_lru(&mut self) -> Option<String> {
        let (_, key) = self.lru.pop_first()?;
        if let Some(entry) = self.entries.remove(&key) {
            self.total_bytes = self.total_bytes.saturating_sub(entry.file_bytes);
        }
        Some(key)
    }
}

pub(super) struct RangeBlockCache {
    root: PathBuf,
    max_bytes: u64,
    min_free_bytes: u64,
    block_size: u64,
    index: Mutex<Index>,
    flights: Cache<String, Arc<tokio::sync::Mutex<()>>>,
    object_limits: Cache<String, Arc<Semaphore>>,
    global_limit: Arc<Semaphore>,
    per_object_limit: usize,
    next_temp: AtomicU64,
}

impl RangeBlockCache {
    pub(super) fn new(settings: &ObjectCacheSettings) -> Result<Self> {
        std::fs::create_dir_all(&settings.root)
            .map_err(|error| Error::internal(format!("create object cache root: {error}")))?;
        std::fs::set_permissions(&settings.root, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| Error::internal(format!("chmod object cache root: {error}")))?;
        let index = scan_index(&settings.root)?;
        metrics::set_disk_bytes(index.total_bytes);
        let cache = Self {
            root: settings.root.clone(),
            max_bytes: settings.max_bytes,
            min_free_bytes: settings.min_free_bytes,
            block_size: settings.block_size_bytes,
            index: Mutex::new(index),
            flights: Cache::builder().max_capacity(100_000).build(),
            object_limits: Cache::builder().max_capacity(10_000).build(),
            global_limit: Arc::new(Semaphore::new(settings.max_concurrent_fetches)),
            per_object_limit: settings.max_concurrent_fetches_per_object,
            next_temp: AtomicU64::new(0),
        };
        cache.evict_blocking();
        Ok(cache)
    }

    pub(super) async fn read_range(
        &self,
        origin: &Arc<dyn ObjectStore>,
        registered: &RegisteredObject,
        range: Range<u64>,
    ) -> object_store::Result<Bytes> {
        if range.is_empty() {
            return Ok(Bytes::new());
        }
        let first = range.start / self.block_size;
        let last = (range.end - 1) / self.block_size;
        let block_count = last
            .checked_sub(first)
            .and_then(|count| count.checked_add(1))
            .ok_or_else(|| cache_error("requested block range overflow"))?;
        let indexes: Vec<u64> = (first..=last)
            .take(cache_usize(block_count, "requested block count")?)
            .collect();
        let mut blocks = HashMap::with_capacity(indexes.len());
        let mut missing = Vec::new();
        for index in &indexes {
            match self.read_block(registered, *index).await {
                Some(bytes) => {
                    metrics::record_hit(bytes.len());
                    blocks.insert(*index, bytes);
                }
                None => missing.push(*index),
            }
        }

        let mut guards = Vec::with_capacity(missing.len());
        for index in &missing {
            let key = block_key(registered, *index);
            let flight = self
                .flights
                .get_with(key, || Arc::new(tokio::sync::Mutex::new(())));
            let guard = match flight.clone().try_lock_owned() {
                Ok(guard) => guard,
                Err(_) => {
                    let _waiter = metrics::SingleflightWaiter::enter();
                    flight.lock_owned().await
                }
            };
            guards.push(guard);
        }

        let mut still_missing = Vec::new();
        for index in missing {
            match self.read_block(registered, index).await {
                Some(bytes) => {
                    metrics::record_hit(bytes.len());
                    blocks.insert(index, bytes);
                }
                None => still_missing.push(index),
            }
        }

        for group in contiguous_groups(&still_missing) {
            let fetched = self.fetch_group(origin, registered, group.clone()).await?;
            for (index, bytes) in fetched {
                blocks.insert(index, bytes);
            }
        }
        drop(guards);

        let mut output = BytesMut::with_capacity(cache_usize(
            range.end - range.start,
            "requested byte range",
        )?);
        for index in indexes {
            let block = blocks
                .get(&index)
                .ok_or_else(|| cache_error("missing fetched block"))?;
            let block_start = index * self.block_size;
            let copy_start = cache_usize(
                range.start.max(block_start) - block_start,
                "block copy start",
            )?;
            let copy_end = cache_usize(
                range.end.min(block_start + block.len() as u64) - block_start,
                "block copy end",
            )?;
            output.put_slice(&block[copy_start..copy_end]);
        }
        Ok(output.freeze())
    }

    async fn fetch_group(
        &self,
        origin: &Arc<dyn ObjectStore>,
        registered: &RegisteredObject,
        indexes: Range<u64>,
    ) -> object_store::Result<Vec<(u64, Bytes)>> {
        let object_key = registered.object.key.as_str();
        let per_object = self.object_limits.get_with(object_key.to_owned(), || {
            Arc::new(Semaphore::new(self.per_object_limit))
        });
        let _global = self
            .global_limit
            .acquire()
            .await
            .map_err(|_| cache_error("global fetch limiter closed"))?;
        let _object = per_object
            .acquire()
            .await
            .map_err(|_| cache_error("per-object fetch limiter closed"))?;
        let start = indexes.start * self.block_size;
        let end = (indexes.end * self.block_size).min(registered.object.size_bytes);
        let mut options = GetOptions::new().with_range(Some(start..end));
        if let Some(etag) = &registered.object.etag {
            options = options.with_if_match(Some(etag.clone()));
        }
        let bytes = origin
            .get_opts(&ObjectPath::from(object_key), options)
            .await?
            .bytes()
            .await?;
        let expected = cache_usize(end - start, "origin range length")?;
        if bytes.len() != expected {
            return Err(cache_error(format!(
                "origin range length mismatch: expected {expected}, got {}",
                bytes.len()
            )));
        }
        if start == 0 && end == registered.object.size_bytes {
            verify_object_checksum(&registered.object.checksum.0, &bytes)?;
        }
        metrics::record_miss(bytes.len());

        let mut result = Vec::with_capacity(cache_usize(
            indexes.end - indexes.start,
            "fetched block count",
        )?);
        for index in indexes {
            let offset = cache_usize((index * self.block_size) - start, "fetched block offset")?;
            let len = cache_usize(
                self.expected_block_len(registered, index),
                "fetched block length",
            )?;
            let block = bytes.slice(offset..offset + len);
            if let Err(error) = self.write_block(registered, index, &block).await {
                tracing::warn!(
                    error = %error,
                    "object block cache write failed; serving origin bytes"
                );
            }
            result.push((index, block));
        }
        Ok(result)
    }

    async fn read_block(&self, registered: &RegisteredObject, index: u64) -> Option<Bytes> {
        let key = block_key(registered, index);
        let path = self.path_for(&key);
        let raw = match tokio::fs::read(&path).await {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
            Err(error) => {
                tracing::warn!(error = %error, "object block cache read failed");
                return None;
            }
        };
        let expected = match cache_usize(
            self.expected_block_len(registered, index),
            "cached block length",
        ) {
            Ok(expected) => expected,
            Err(error) => {
                tracing::warn!(%error, "object block cache length is not addressable");
                return None;
            }
        };
        match decode_block(&raw, expected) {
            Ok(bytes) => {
                self.index.lock().touch(&key);
                Some(bytes)
            }
            Err(error) => {
                tracing::warn!(error = %error, "discarding corrupt object cache block");
                metrics::record_corrupt();
                self.remove_block(&key, &path).await;
                None
            }
        }
    }

    async fn write_block(
        &self,
        registered: &RegisteredObject,
        index: u64,
        bytes: &Bytes,
    ) -> io::Result<()> {
        let key = block_key(registered, index);
        let path = self.path_for(&key);
        let parent = path.parent().expect("block path has parent");
        tokio::fs::create_dir_all(parent).await?;
        tokio::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).await?;
        let suffix = self.next_temp.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(".{key}.{}.{}.tmp", std::process::id(), suffix));
        let encoded = encode_block(bytes);
        let mut file = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .await?;
        tokio::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600)).await?;
        if let Err(error) = async {
            file.write_all(&encoded).await?;
            file.sync_all().await?;
            drop(file);
            tokio::fs::rename(&temp, &path).await
        }
        .await
        {
            let _ = tokio::fs::remove_file(&temp).await;
            return Err(error);
        }
        let total = {
            let mut index = self.index.lock();
            index.insert(key, encoded.len() as u64);
            index.total_bytes
        };
        metrics::set_disk_bytes(total);
        self.evict().await;
        Ok(())
    }

    fn expected_block_len(&self, registered: &RegisteredObject, index: u64) -> u64 {
        let start = index * self.block_size;
        registered
            .object
            .size_bytes
            .saturating_sub(start)
            .min(self.block_size)
    }

    async fn evict(&self) {
        while self.should_evict() {
            let victim = self.index.lock().pop_lru();
            let Some(key) = victim else { break };
            let _ = tokio::fs::remove_file(self.path_for(&key)).await;
            metrics::record_eviction();
        }
        metrics::set_disk_bytes(self.index.lock().total_bytes);
    }

    fn evict_blocking(&self) {
        while self.should_evict() {
            let victim = self.index.lock().pop_lru();
            let Some(key) = victim else { break };
            let _ = std::fs::remove_file(self.path_for(&key));
            metrics::record_eviction();
        }
        metrics::set_disk_bytes(self.index.lock().total_bytes);
    }

    fn should_evict(&self) -> bool {
        self.index.lock().total_bytes > self.max_bytes
            || available_bytes(&self.root).is_some_and(|free| free < self.min_free_bytes)
    }

    async fn remove_block(&self, key: &str, path: &Path) {
        let _ = tokio::fs::remove_file(path).await;
        let total = {
            let mut index = self.index.lock();
            index.remove(key);
            index.total_bytes
        };
        metrics::set_disk_bytes(total);
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.root.join(&key[..2]).join(format!("{}.blk", &key[2..]))
    }
}

fn block_key(registered: &RegisteredObject, index: u64) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(registered.organization_id.as_str().as_bytes());
    hasher.update(&[0]);
    hasher.update(registered.object.key.as_str().as_bytes());
    hasher.update(&[0]);
    hasher.update(registered.object.checksum.0.as_bytes());
    hasher.update(&index.to_le_bytes());
    hasher.finalize().to_hex().to_string()
}

fn contiguous_groups(indexes: &[u64]) -> Vec<Range<u64>> {
    let Some(&first) = indexes.first() else {
        return Vec::new();
    };
    let mut groups = Vec::new();
    let mut start = first;
    let mut previous = first;
    for &index in &indexes[1..] {
        if index != previous + 1 {
            groups.push(start..previous + 1);
            start = index;
        }
        previous = index;
    }
    groups.push(start..previous + 1);
    groups
}

fn scan_index(root: &Path) -> Result<Index> {
    let mut index = Index::default();
    for first in std::fs::read_dir(root)
        .map_err(|error| Error::internal(format!("scan object cache: {error}")))?
    {
        let first =
            first.map_err(|error| Error::internal(format!("scan object cache: {error}")))?;
        if !first.file_type().is_ok_and(|kind| kind.is_dir()) {
            if first.path().extension().is_some_and(|ext| ext == "tmp") {
                let _ = std::fs::remove_file(first.path());
            }
            continue;
        }
        std::fs::set_permissions(first.path(), std::fs::Permissions::from_mode(0o700))
            .map_err(|error| Error::internal(format!("chmod object cache shard: {error}")))?;
        for file in std::fs::read_dir(first.path())
            .map_err(|error| Error::internal(format!("scan object cache shard: {error}")))?
        {
            let file =
                file.map_err(|error| Error::internal(format!("scan object cache: {error}")))?;
            let path = file.path();
            if path.extension().is_some_and(|ext| ext == "tmp") {
                let _ = std::fs::remove_file(path);
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "blk") {
                continue;
            }
            let shard = first.file_name().to_string_lossy().into_owned();
            let stem = path
                .file_stem()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default();
            let key = format!("{shard}{stem}");
            if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                continue;
            }
            let size = file.metadata().map(|meta| meta.len()).unwrap_or(0);
            index.insert(key, size);
        }
    }
    Ok(index)
}

fn cache_error(message: impl Into<String>) -> object_store::Error {
    object_store::Error::Generic {
        store: "CachedObjectStore",
        source: Box::new(io::Error::other(message.into())),
    }
}

fn cache_usize(value: u64, what: &str) -> object_store::Result<usize> {
    usize::try_from(value).map_err(|_| cache_error(format!("{what} exceeds address space")))
}

#[cfg(test)]
mod tests {
    use object_store::{ObjectStoreExt, memory::InMemory};

    use super::*;
    use crate::{
        domain::storage::{ObjectChecksum, ObjectKey, StoredObject},
        shared::ids::Id,
    };

    fn registered(bytes: &[u8]) -> RegisteredObject {
        RegisteredObject {
            organization_id: Id::from_string("org-a"),
            object: StoredObject {
                key: ObjectKey::from_string("v1/artifacts/a"),
                size_bytes: bytes.len() as u64,
                checksum: ObjectChecksum::from_string(format!(
                    "b3:{}",
                    blake3::hash(bytes).to_hex()
                )),
                etag: None,
            },
        }
    }

    fn settings(root: PathBuf) -> ObjectCacheSettings {
        ObjectCacheSettings {
            root,
            max_bytes: 1024 * 1024,
            block_size_bytes: 1024 * 1024,
            min_free_bytes: 0,
            ..ObjectCacheSettings::default()
        }
    }

    #[tokio::test]
    async fn range_is_split_into_verified_blocks_and_survives_restart() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = Bytes::from(vec![7_u8; 1024 * 1024 + 31]);
        let origin: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let object = registered(&bytes);
        origin
            .put(
                &ObjectPath::from(object.object.key.as_str()),
                bytes.clone().into(),
            )
            .await
            .unwrap();
        let cache = RangeBlockCache::new(&settings(temp.path().into())).unwrap();
        let actual = cache
            .read_range(&origin, &object, 17..bytes.len() as u64 - 7)
            .await
            .unwrap();
        assert_eq!(actual, bytes.slice(17..bytes.len() - 7));
        drop(cache);

        let reopened = RangeBlockCache::new(&settings(temp.path().into())).unwrap();
        let actual = reopened
            .read_range(&origin, &object, 0..bytes.len() as u64)
            .await
            .unwrap();
        assert_eq!(actual, bytes);
    }

    #[tokio::test]
    async fn corrupt_block_is_refetched() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = Bytes::from_static(b"catalog-backed object");
        let origin: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let object = registered(&bytes);
        origin
            .put(
                &ObjectPath::from(object.object.key.as_str()),
                bytes.clone().into(),
            )
            .await
            .unwrap();
        let cache = RangeBlockCache::new(&settings(temp.path().into())).unwrap();
        cache
            .read_range(&origin, &object, 0..bytes.len() as u64)
            .await
            .unwrap();
        let key = block_key(&object, 0);
        tokio::fs::write(cache.path_for(&key), b"corrupt")
            .await
            .unwrap();
        let actual = cache
            .read_range(&origin, &object, 0..bytes.len() as u64)
            .await
            .unwrap();
        assert_eq!(actual, bytes);
    }
}
