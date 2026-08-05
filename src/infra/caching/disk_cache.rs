// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Parquet 磁盘缓存。
//!
//! 设计权衡：
//! - 不引 `sled`；用一个本地目录 + 内存索引（`HashMap` 存条目 + `BTreeMap` 按 LRU token
//!   排序，淘汰走 `pop_first` 而非全表扫描）
//! - key = `blake3(object_key)`；文件落到 `<dir>/<first2>/<rest>.bin`
//! - `max_size_gb` 超阈值则按 LRU 顺序 unlink
//! - hit：异步直读 `tokio::fs::read` 返 `Bytes`
//! - miss：调用方拿 object_store 返回的 `Bytes` 后调 `insert`（异步落盘 + 入 index）
//! - 启动时扫描目录重建索引：否则重启后 `total` 从 0 起算，磁盘上已有的文件既不被
//!   计数也不被淘汰，实际占用会突破 `max_size_gb` 且跨重启无界增长

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::SystemTime,
};

use bytes::Bytes;
use parking_lot::Mutex;
use prometheus::{Gauge, IntCounter, Opts};

use crate::shared::{
    Error, Result,
    metrics::{global_registry, register_int_counter},
};

/// 缓存层 metric family（spec 命名固定，与 `cache_<level>_*` 同风格）。
struct DiskCacheMetrics {
    hits: IntCounter,
    misses: IntCounter,
    evictions: IntCounter,
    hit_ratio: Gauge,
    hits_view: AtomicU64,
    misses_view: AtomicU64,
}

fn disk_cache_metrics() -> &'static DiskCacheMetrics {
    static M: OnceLock<DiskCacheMetrics> = OnceLock::new();
    M.get_or_init(|| {
        let hits = register_int_counter("cache_parquet_disk_hits_total", "parquet disk cache hits");
        let misses = register_int_counter(
            "cache_parquet_disk_misses_total",
            "parquet disk cache misses",
        );
        let evictions = register_int_counter(
            "cache_parquet_disk_evictions_total",
            "parquet disk cache LRU evictions",
        );
        let hit_ratio = {
            let g = Gauge::with_opts(Opts::new(
                "cache_parquet_disk_hit_ratio",
                "parquet disk cache hit ratio in [0.0, 1.0]",
            ))
            .expect("create gauge");
            match global_registry().register(Box::new(g.clone())) {
                Ok(()) | Err(prometheus::Error::AlreadyReg) => g,
                Err(e) => panic!("register gauge: {e}"),
            }
        };
        DiskCacheMetrics {
            hits,
            misses,
            evictions,
            hit_ratio,
            hits_view: AtomicU64::new(0),
            misses_view: AtomicU64::new(0),
        }
    })
}

fn record_hit() {
    let m = disk_cache_metrics();
    m.hits.inc();
    m.hits_view.fetch_add(1, Ordering::Relaxed);
    refresh_hit_ratio(m);
}

fn record_miss() {
    let m = disk_cache_metrics();
    m.misses.inc();
    m.misses_view.fetch_add(1, Ordering::Relaxed);
    refresh_hit_ratio(m);
}

fn record_evict() {
    let m = disk_cache_metrics();
    m.evictions.inc();
}

fn refresh_hit_ratio(m: &DiskCacheMetrics) {
    let h = m.hits_view.load(Ordering::Relaxed) as f64;
    let miss = m.misses_view.load(Ordering::Relaxed) as f64;
    let total = h + miss;
    let ratio = if total == 0.0 { 0.0 } else { h / total };
    m.hit_ratio.set(ratio);
}

#[derive(Debug, Clone)]
pub struct DiskCacheSettings {
    pub dir: PathBuf,
    pub max_bytes: u64,
}

impl DiskCacheSettings {
    pub fn new(dir: impl Into<PathBuf>, max_gb: u32) -> Self {
        Self {
            dir: dir.into(),
            max_bytes: (max_gb as u64) * 1024 * 1024 * 1024,
        }
    }
}

struct Entry {
    size: u64,
    /// LRU 顺序：单调递增 token，evict 时挑最小。
    last_used: u64,
}

pub struct ParquetDiskCache {
    settings: DiskCacheSettings,
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    index: HashMap<String, Entry>,
    /// `last_used` token → hash。token 单调递增且唯一，故与 `index` 一一对应。
    /// 淘汰取 `pop_first`（最久未用），避免对整个 index 做线性扫描。
    lru: BTreeMap<u64, String>,
    total: u64,
    next_tok: u64,
}

impl Inner {
    /// 分配一个新 token 并把 `hash` 移到 LRU 末端。返回新 token。
    fn touch(&mut self, hash: &str) -> u64 {
        self.next_tok += 1;
        let tok = self.next_tok;
        if let Some(e) = self.index.get_mut(hash) {
            self.lru.remove(&e.last_used);
            e.last_used = tok;
            self.lru.insert(tok, hash.to_string());
        }
        tok
    }

    fn remove(&mut self, hash: &str) {
        if let Some(e) = self.index.remove(hash) {
            self.lru.remove(&e.last_used);
            self.total = self.total.saturating_sub(e.size);
        }
    }

    /// 弹出最久未用的条目，返回 `(hash, size)`。
    fn pop_lru(&mut self) -> Option<(String, u64)> {
        let (_, hash) = self.lru.pop_first()?;
        let size = self.index.remove(&hash).map(|e| e.size).unwrap_or(0);
        self.total = self.total.saturating_sub(size);
        Some((hash, size))
    }
}

impl ParquetDiskCache {
    pub fn new(settings: DiskCacheSettings) -> Result<Self> {
        std::fs::create_dir_all(&settings.dir)
            .map_err(|e| Error::internal(format!("disk_cache mkdir: {e}")))?;
        let mut inner = Inner {
            index: HashMap::new(),
            lru: BTreeMap::new(),
            total: 0,
            next_tok: 1,
        };
        // 重建索引：磁盘上的文件跨重启存活，不扫回来就等于泄漏配额。
        // 没有真实访问时间可用，用 mtime 定初始 LRU 顺序（老的先被淘汰）。
        let mut found = scan_cache_dir(&settings.dir)?;
        found.sort_by_key(|(mtime, _, _)| *mtime);
        for (_, hash, size) in found {
            inner.next_tok += 1;
            let tok = inner.next_tok;
            inner.total = inner.total.saturating_add(size);
            inner.lru.insert(tok, hash.clone());
            inner.index.insert(
                hash,
                Entry {
                    size,
                    last_used: tok,
                },
            );
        }

        let cache = Self {
            settings,
            inner: Arc::new(Mutex::new(inner)),
        };
        // 重建后可能已经超配额（上次运行时上限更大，或人为塞了文件）。同步收敛到上限内，
        // 否则要等到下一次 insert 才裁剪。
        cache.evict_to_cap_blocking();
        Ok(cache)
    }

    /// 同步把总量裁到 `max_bytes` 以内。仅启动期用（`new` 不是 async）。
    fn evict_to_cap_blocking(&self) {
        let mut victims = Vec::new();
        {
            let mut inner = self.inner.lock();
            while inner.total > self.settings.max_bytes
                && let Some((hash, _)) = inner.pop_lru()
            {
                victims.push(hash);
            }
        }
        for hash in victims {
            let _ = std::fs::remove_file(self.path_for(&hash));
            record_evict();
        }
    }

    fn hash_key(object_key: &str) -> String {
        blake3::hash(object_key.as_bytes()).to_hex().to_string()
    }

    fn path_for(&self, hash: &str) -> PathBuf {
        let (a, b) = hash.split_at(2);
        self.settings.dir.join(a).join(format!("{b}.bin"))
    }

    /// hit → Some(bytes)；miss → None。
    pub async fn get(&self, object_key: &str) -> Option<Bytes> {
        let hash = Self::hash_key(object_key);
        let path = self.path_for(&hash);
        // 更新 LRU
        let exists = {
            let mut inner = self.inner.lock();
            if inner.index.contains_key(&hash) {
                inner.touch(&hash);
                true
            } else {
                false
            }
        };
        if !exists {
            record_miss();
            return None;
        }
        match tokio::fs::read(&path).await {
            Ok(v) => {
                record_hit();
                Some(Bytes::from(v))
            }
            Err(_) => {
                // 文件不在了（手动删？）→ 移除 index 条目
                self.inner.lock().remove(&hash);
                record_miss();
                None
            }
        }
    }

    pub async fn insert(&self, object_key: &str, bytes: Bytes) -> Result<()> {
        let hash = Self::hash_key(object_key);
        let path = self.path_for(&hash);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| Error::internal(format!("disk_cache mkdir: {e}")))?;
        }
        let size = bytes.len() as u64;
        tokio::fs::write(&path, &bytes)
            .await
            .map_err(|e| Error::internal(format!("disk_cache write: {e}")))?;
        // 入 index
        let mut to_evict = Vec::new();
        {
            let mut inner = self.inner.lock();
            // 覆盖写：先摘掉旧条目（连带它的 lru token 与计数），再挂新的。
            inner.remove(&hash);
            inner.next_tok += 1;
            let last_used = inner.next_tok;
            inner.index.insert(hash.clone(), Entry { size, last_used });
            inner.lru.insert(last_used, hash);
            inner.total = inner.total.saturating_add(size);
            while inner.total > self.settings.max_bytes
                && let Some((victim_key, _)) = inner.pop_lru()
            {
                to_evict.push(victim_key);
            }
        }
        for vk in to_evict {
            let p = self.path_for(&vk);
            let _ = tokio::fs::remove_file(p).await;
            record_evict();
        }
        Ok(())
    }

    pub fn size_bytes(&self) -> u64 {
        self.inner.lock().total
    }

    pub fn dir(&self) -> &Path {
        &self.settings.dir
    }
}

/// 扫 `<dir>/<first2>/<rest>.bin`，返回 `(mtime, hash, size)`。
/// 无法识别的文件/目录跳过：缓存目录是自管的，外来文件不该影响启动。
fn scan_cache_dir(dir: &Path) -> Result<Vec<(SystemTime, String, u64)>> {
    let mut out = Vec::new();
    let shards = match std::fs::read_dir(dir) {
        Ok(it) => it,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(Error::internal(format!("disk_cache scan: {e}"))),
    };
    for shard in shards.flatten() {
        if !shard.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let prefix = shard.file_name().to_string_lossy().to_string();
        if prefix.len() != 2 {
            continue;
        }
        let Ok(files) = std::fs::read_dir(shard.path()) else {
            continue;
        };
        for f in files.flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            let Some(rest) = name.strip_suffix(".bin") else {
                continue;
            };
            let Ok(meta) = f.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            out.push((mtime, format!("{prefix}{rest}"), meta.len()));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn miss_then_insert_then_hit() {
        let tmp = TempDir::new().unwrap();
        let c = ParquetDiskCache::new(DiskCacheSettings {
            dir: tmp.path().to_path_buf(),
            max_bytes: 1 << 20,
        })
        .unwrap();
        assert!(c.get("obj/1").await.is_none());
        c.insert("obj/1", Bytes::from_static(b"hello"))
            .await
            .unwrap();
        let got = c.get("obj/1").await.unwrap();
        assert_eq!(&got[..], b"hello");
    }

    #[tokio::test]
    async fn lru_eviction_respects_cap() {
        let tmp = TempDir::new().unwrap();
        let c = ParquetDiskCache::new(DiskCacheSettings {
            dir: tmp.path().to_path_buf(),
            max_bytes: 10, // 极小
        })
        .unwrap();
        c.insert("a", Bytes::from_static(b"AAAAAA")).await.unwrap(); // 6
        c.insert("b", Bytes::from_static(b"BBBBBB")).await.unwrap(); // 6 → 总 12 > 10，evict a
        assert!(c.get("a").await.is_none());
        assert!(c.get("b").await.is_some());
    }

    #[tokio::test]
    async fn rebuilds_index_from_disk_on_restart() {
        // 重启前写 3 个文件，重启后新实例必须把它们算进 total —— 否则 total 从 0 起算，
        // 这些文件永远不被计数也不被淘汰，磁盘占用跨重启无界增长。
        let tmp = TempDir::new().unwrap();
        let settings = DiskCacheSettings {
            dir: tmp.path().to_path_buf(),
            max_bytes: 1 << 20,
        };
        {
            let c = ParquetDiskCache::new(settings.clone()).unwrap();
            for k in ["a", "b", "c"] {
                c.insert(k, Bytes::from_static(b"1234567890"))
                    .await
                    .unwrap();
            }
            assert_eq!(c.size_bytes(), 30);
        }

        let reopened = ParquetDiskCache::new(settings).unwrap();
        assert_eq!(
            reopened.size_bytes(),
            30,
            "重启后必须扫回磁盘上已有的 30 字节"
        );
        // 重建的条目要能命中，不能只是计数。
        assert_eq!(&reopened.get("b").await.unwrap()[..], b"1234567890");
    }

    #[tokio::test]
    async fn restart_evicts_down_to_cap_when_disk_exceeds_it() {
        // 上次运行时上限更大 / 人为塞过文件 → 重启时磁盘已超配额，必须当场收敛，
        // 而不是等下一次 insert 才裁剪。
        let tmp = TempDir::new().unwrap();
        {
            let big = ParquetDiskCache::new(DiskCacheSettings {
                dir: tmp.path().to_path_buf(),
                max_bytes: 1 << 20,
            })
            .unwrap();
            for k in ["a", "b", "c", "d"] {
                big.insert(k, Bytes::from_static(b"1234567890"))
                    .await
                    .unwrap();
            }
            assert_eq!(big.size_bytes(), 40);
        }

        // 缩小上限重开：40 字节 > 25，必须裁到 25 以内。
        let small = ParquetDiskCache::new(DiskCacheSettings {
            dir: tmp.path().to_path_buf(),
            max_bytes: 25,
        })
        .unwrap();
        assert!(
            small.size_bytes() <= 25,
            "重启后应收敛到上限内，实得 {}",
            small.size_bytes()
        );
        let on_disk = scan_cache_dir(tmp.path()).unwrap();
        assert!(
            on_disk.len() <= 2,
            "超配额的文件应被 unlink，磁盘上还剩 {} 个",
            on_disk.len()
        );
    }

    #[tokio::test]
    async fn metrics_register_on_first_access() {
        // 不依赖 hit/miss 计数器具体值（其它测试也在跑同一进程的 global registry），
        // 只断言四条 metric 名字都已注册并在 /metrics 文本里出现。
        let tmp = TempDir::new().unwrap();
        let c = ParquetDiskCache::new(DiskCacheSettings {
            dir: tmp.path().to_path_buf(),
            max_bytes: 1 << 20,
        })
        .unwrap();
        // miss + hit + evict 三种事件各触发一次。
        assert!(c.get("metrics_probe").await.is_none()); // miss
        c.insert("metrics_probe", Bytes::from_static(b"x"))
            .await
            .unwrap();
        assert!(c.get("metrics_probe").await.is_some()); // hit

        let text = crate::shared::metrics::gather_text().expect("scrape");
        assert!(
            text.contains("cache_parquet_disk_hits_total"),
            "hits metric must register"
        );
        assert!(
            text.contains("cache_parquet_disk_misses_total"),
            "misses metric must register"
        );
        assert!(
            text.contains("cache_parquet_disk_evictions_total"),
            "evictions metric must register"
        );
        assert!(
            text.contains("cache_parquet_disk_hit_ratio"),
            "hit_ratio gauge must register"
        );
    }
}
