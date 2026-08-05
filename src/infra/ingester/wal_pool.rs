// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! WalPool：按 `(org, stream_type, stream)` 维护独立 `SegmentWal` 实例。
//!
//! - 每个 key 一个独立子目录（`{root}/{org_id}/{stream_type}/{stream_name}`），
//!   彼此独立 rotate / fsync / replay。
//! - `append` 串行化到 `Arc<Mutex<SegmentWal>>`：同 key 不并发写。
//! - `truncate_up_to(key, seq)` 把"全部 record index ≤ seq"的封口 segment 整段删除；
//!   当前正写入的 segment 不动（即使该 segment 内全部 ≤ seq 也保留，避免删活跃写文件）。
//! - `recover()` 扫 root 目录重建 key 列表并把每个 wal 的 record 全部读出，供 IngesterWorker
//!   在 ready 之前 replay 回 buffer。
//!
//! **目录命名约定**：`stream_name` 必须满足领域层统一的 path-safe 校验，否则
//! [`WalPool::append`] 返错（sanitize 不安全会导致冲突；约束也在 ingest 入口做）。

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use anyhow::{Context, Result, anyhow};
use dashmap::DashMap;
use tokio::sync::Mutex;

use crate::{
    domain::{storage::PhysicalDatasetKind, stream::StreamType},
    infra::{
        cipher::CipherRootKey,
        ingester::metrics::{WalInflightGuard, observe_wal_lock_wait},
        segment_wal::{
            FsyncPolicy, SegmentWal, TermSource, WalEntryType, WalRecord, scan_segment_max_index,
        },
    },
    shared::ids::Id,
};

pub type WalKey = (Id, StreamType, String, PhysicalDatasetKind);

/// [`WalPool::truncate_up_to`] 的同步实现（在 blocking 池上跑）。
fn truncate_dir_blocking(dir: &Path, seq: u64) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let segments = SegmentWal::segment_paths_sorted(dir)?;
    if segments.len() <= 1 {
        // 0 segments or only the active (head) one - nothing to seal
        return Ok(());
    }
    let active_idx = segments.len() - 1;
    for seg_path in segments.iter().take(active_idx) {
        // 只读 header 链取 max index：这里只需判断本段是否全部 ≤ seq，
        // 没必要为此把每条记录解压 + 拷贝一遍。
        match scan_segment_max_index(seg_path)? {
            // empty / corrupt-only segment：直接删
            None => std::fs::remove_file(seg_path)?,
            Some(max_index) if max_index <= seq => std::fs::remove_file(seg_path)?,
            // 出现含 > seq 的旧 segment 即停（理论上不该发生 — index 单调递增）
            Some(_) => break,
        }
    }
    Ok(())
}

const DEFAULT_BUFFER_BYTES: usize = 64 * 1024;

/// WAL at-rest 加密 record 的 magic 前缀（区分明文 / 旧 segment）。
const WAL_ENC_MAGIC: &[u8; 4] = b"WEN1";

/// 用 KEK 把 record payload 封成 `WEN1 || nonce(12) || ciphertext`。
fn wal_encrypt(kek: &CipherRootKey, payload: &[u8]) -> Result<Vec<u8>> {
    let (nonce, ct) = kek.seal(payload).map_err(|e| anyhow!("wal seal: {e}"))?;
    let mut out = Vec::with_capacity(WAL_ENC_MAGIC.len() + nonce.len() + ct.len());
    out.extend_from_slice(WAL_ENC_MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// 还原带 `WEN1` 前缀的 record（caller 已确认前缀）。无 KEK 时报错（数据加密但缺 key）。
fn wal_decrypt_body(kek: Option<&CipherRootKey>, stored: &[u8]) -> Result<Vec<u8>> {
    let kek = kek.ok_or_else(|| anyhow!("wal record is encrypted but no cipher key configured"))?;
    let body = &stored[WAL_ENC_MAGIC.len()..];
    if body.len() < 12 {
        return Err(anyhow!("wal encrypted record too short"));
    }
    let (nonce, ct) = body.split_at(12);
    kek.open(nonce, ct).map_err(|e| anyhow!("wal open: {e}"))
}

fn stream_type_dir(st: StreamType) -> &'static str {
    match st {
        StreamType::Logs => "logs",
        StreamType::Metrics => "metrics",
        StreamType::Traces => "traces",
        StreamType::Profiles => "profiles",
        StreamType::Extend => "extend",
    }
}

fn stream_type_from_dir(s: &str) -> Option<StreamType> {
    match s {
        "logs" => Some(StreamType::Logs),
        "metrics" => Some(StreamType::Metrics),
        "traces" => Some(StreamType::Traces),
        "profiles" => Some(StreamType::Profiles),
        "extend" => Some(StreamType::Extend),
        _ => None,
    }
}

fn key_dir(root: &Path, key: &WalKey) -> PathBuf {
    root.join(key.0.as_str())
        .join(stream_type_dir(key.1))
        .join(key.3.as_str())
        .join(&key.2)
}

pub struct WalPool {
    root: PathBuf,
    segment_size_bytes: usize,
    fsync_policy: FsyncPolicy,
    term_source: Arc<dyn TermSource>,
    pools: DashMap<WalKey, Arc<Mutex<SegmentWal>>>,
    /// at-rest 加密 KEK；`None` = 明文落盘（默认）。`with_cipher` 注入。
    cipher: Option<CipherRootKey>,
}

impl WalPool {
    /// `segment_size_bytes` 为每个 segment 文件的滚动上限。
    ///
    /// - `fsync_policy` 由 bootstrap 从 `WalSettings` 构造（见 `src/bootstrap/storage.rs`
    ///   的 `build_fsync_policy`）；新建 `SegmentWal` 时透传。
    /// - `term_source` 为 raft / consensus 注入点；OSS 默认 `StaticTermSource(1)`。
    pub fn new(
        root: impl Into<PathBuf>,
        segment_size_bytes: usize,
        fsync_policy: FsyncPolicy,
        term_source: Arc<dyn TermSource>,
    ) -> Self {
        Self {
            root: root.into(),
            segment_size_bytes,
            fsync_policy,
            term_source,
            pools: DashMap::new(),
            cipher: None,
        }
    }

    /// 注入 at-rest 加密 KEK：append 时 payload 加密落盘、recover 时解密。
    pub fn with_cipher(mut self, cipher: CipherRootKey) -> Self {
        self.cipher = Some(cipher);
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 拿到或新建给定 key 的 `Arc<Mutex<SegmentWal>>`。
    fn open_or_create(&self, key: &WalKey) -> Result<Arc<Mutex<SegmentWal>>> {
        if let Some(w) = self.pools.get(key) {
            return Ok(w.clone());
        }
        crate::domain::stream::validate_stream_name(&key.2)
            .map_err(|error| anyhow!(error.to_string()))?;
        let dir = key_dir(&self.root, key);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("create wal dir {}", dir.display()))?;
        let wal = SegmentWal::new(
            &dir,
            self.segment_size_bytes,
            DEFAULT_BUFFER_BYTES,
            None,
            None,
            self.fsync_policy,
            self.term_source.current_term(),
        )
        .with_context(|| format!("open wal at {}", dir.display()))?;
        let arc = Arc::new(Mutex::new(wal));
        self.pools.insert(key.clone(), arc.clone());
        Ok(arc)
    }

    /// 追加一条 WAL 记录（Normal entry）。
    ///
    /// 写入本身是同步文件 IO，跑在 blocking 池上而非 tokio worker：默认的 `Batch` 策略
    /// 每攒够 `max_pending` 条（或超时）就在那一次 `write_raw` 里同步 `sync_file`，
    /// 毫秒级；留在 worker 上会把整个 runtime 的其他任务一起卡住。
    ///
    /// `payload` 收 owned `Vec` 是为了能直接 move 进 blocking task —— 调用方本来
    /// 持有的就是 owned buffer，不必为此多拷一份。
    #[tracing::instrument(
        name = "wal.append",
        skip_all,
        fields(
            otel.kind = "internal",
            molesignal.wal.payload_bytes = payload.len(),
            molesignal.stream.type = ?key.1
        )
    )]
    pub async fn append(&self, key: &WalKey, payload: Vec<u8>, seq: u64) -> Result<()> {
        let wal = self.open_or_create(key)?;
        let stream_type_label = stream_type_dir(key.1);
        // term 透传：若 raft 在两次 append 间 leader election 切换，下一条记录的 header
        // 必须反映新的 term。term_source 默认是 StaticTermSource，比较是 noop。
        let term = self.term_source.current_term();
        let payload = match &self.cipher {
            Some(k) => wal_encrypt(k, &payload)?,
            None => payload,
        };
        let lock_started = Instant::now();
        tokio::task::spawn_blocking(move || {
            let mut guard = wal.blocking_lock();
            observe_wal_lock_wait(stream_type_label, lock_started.elapsed().as_secs_f64());
            let _inflight = WalInflightGuard::enter(stream_type_label);
            if guard.current_term() != term {
                guard.set_term(term);
            }
            guard.write_raw(WalEntryType::Normal, &payload, seq)
        })
        .await
        .map_err(|e| anyhow!("wal append join: {e}"))?
    }

    /// 强制封口当前活跃 segment（若非空）。flush_one 在 truncate 前调用，
    /// 把所有已写 record 推到 sealed segment，便于随后被 [`Self::truncate_up_to`] 整段删除。
    ///
    /// 若 key 对应的 pool 尚未初始化（从未 append 过），no-op。
    pub async fn seal_active(&self, key: &WalKey) -> Result<()> {
        let Some(wal) = self.pools.get(key).map(|w| w.clone()) else {
            return Ok(());
        };
        let mut guard = wal.lock().await;
        guard.seal_active()?;
        Ok(())
    }

    /// 把所有 record index ≤ `seq` 的 sealed segments 整段删除（当前活跃 segment 保留）。
    /// 遇到"含 index > seq 记录"的 segment 立即停止（保持时间序完整）。
    pub async fn truncate_up_to(&self, key: &WalKey, seq: u64) -> Result<()> {
        let dir = key_dir(&self.root, key);
        // 列目录、扫 header、unlink 全是同步文件 IO，挪到 blocking 池，别占 tokio worker。
        tokio::task::spawn_blocking(move || truncate_dir_blocking(&dir, seq))
            .await
            .map_err(|e| anyhow!("truncate join: {e}"))?
    }

    /// 启动期扫整 root 目录恢复未 flush 的记录。结果按 key 分组返回。
    pub fn recover(&self) -> Result<Vec<(WalKey, Vec<WalRecord>)>> {
        let mut out = Vec::new();
        if !self.root.exists() {
            return Ok(out);
        }
        for org_entry in std::fs::read_dir(&self.root)? {
            let org_entry = org_entry?;
            if !org_entry.file_type()?.is_dir() {
                continue;
            }
            let org_id = Id::from_string(org_entry.file_name().to_string_lossy().to_string());
            for st_entry in std::fs::read_dir(org_entry.path())? {
                let st_entry = st_entry?;
                if !st_entry.file_type()?.is_dir() {
                    continue;
                }
                let Some(stream_type) =
                    stream_type_from_dir(&st_entry.file_name().to_string_lossy())
                else {
                    continue;
                };
                for dataset_entry in std::fs::read_dir(st_entry.path())? {
                    let dataset_entry = dataset_entry?;
                    if !dataset_entry.file_type()?.is_dir() {
                        continue;
                    }
                    let Ok(dataset_kind) = dataset_entry
                        .file_name()
                        .to_string_lossy()
                        .parse::<PhysicalDatasetKind>()
                    else {
                        continue;
                    };
                    for stream_entry in std::fs::read_dir(dataset_entry.path())? {
                        let stream_entry = stream_entry?;
                        if !stream_entry.file_type()?.is_dir() {
                            continue;
                        }
                        let stream_name = stream_entry.file_name().to_string_lossy().to_string();
                        let key = (org_id.clone(), stream_type, stream_name, dataset_kind);
                        let (mut records, _truncated) =
                            SegmentWal::read_records(stream_entry.path())?;
                        // at-rest 解密：仅对带 magic 前缀的 record（明文原样透传）。
                        for r in &mut records {
                            if r.payload.starts_with(WAL_ENC_MAGIC) {
                                r.payload = wal_decrypt_body(self.cipher.as_ref(), &r.payload)?;
                            }
                        }
                        if !records.is_empty() {
                            out.push((key, records));
                        }
                    }
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use tempfile::tempdir;

    use super::*;
    use crate::infra::segment_wal::{StaticTermSource, SyncLevel, scan_segment_file_readonly};

    fn key_logs(org: &str, stream: &str) -> WalKey {
        (
            Id::from_string(org),
            StreamType::Logs,
            stream.to_string(),
            PhysicalDatasetKind::Raw,
        )
    }

    fn test_pool(root: &Path, seg: usize) -> WalPool {
        WalPool::new(
            root,
            seg,
            FsyncPolicy::none_default(),
            Arc::new(StaticTermSource(1)),
        )
    }

    /// `truncate` 靠 `scan_segment_max_index` 判断一整段能否删除，它必须和全量扫描
    /// 给出一致的 max index —— 不一致就会误删还没落 parquet 的 WAL 段。
    #[tokio::test]
    async fn max_index_scan_agrees_with_full_scan() {
        let tmp = tempdir().unwrap();
        // 段开小，逼出多个 sealed segment；payload 给大一点让 LZ4 分支也走到。
        let pool = test_pool(tmp.path(), 512);
        let k = key_logs("org-a", "app");
        for i in 1..=20u64 {
            let payload = format!("{}-{i}", "x".repeat(60));
            pool.append(&k, payload.into_bytes(), i).await.unwrap();
        }

        let dir = key_dir(tmp.path(), &k);
        let segments = SegmentWal::segment_paths_sorted(&dir).unwrap();
        assert!(segments.len() > 1, "需要多个 segment 才有意义");
        for seg in &segments {
            let full = scan_segment_file_readonly(seg).unwrap();
            let expected = full.records.iter().map(|r| r.index).max();
            assert_eq!(
                scan_segment_max_index(seg).unwrap(),
                expected,
                "segment {seg:?} 的 max index 与全量扫描不一致"
            );
        }
    }

    /// truncate 按 segment 整段删，且遇到 `max_index > seq` 即停。而 flush 的对象存储 IO
    /// 期间新到的写入会落进同一个活跃段，`seal_active` 把它和已 flush 的记录封进同一段，
    /// 该段 `max_index > hwm` → 整段删不掉。于是**已落 parquet 的记录会继续留在 WAL 里**。
    ///
    /// 稳态持续写入下这是常态而非边角情况，因此「flush 失败时按 index ≤ hwm 从 WAL 重放」
    /// 不安全：会把已落盘的记录重放成重复数据。
    #[tokio::test]
    async fn successful_truncate_still_retains_flushed_records_when_write_races_flush() {
        let tmp = tempdir().unwrap();
        // 段开大，让所有记录落进同一个活跃段（逼出"已 flush 的和新写入的同段混装"）。
        let pool = test_pool(tmp.path(), 1024 * 1024);
        let k = key_logs("org-a", "app");
        // 快照时 buffer 吸收了 seq 1..=10 → hwm = 10。
        for i in 1..=10u64 {
            pool.append(&k, format!("flushed-{i}").into_bytes(), i)
                .await
                .unwrap();
        }
        // flush 的对象存储 IO 期间又来一条写入（finish_and_clear 已 drop 了 buffer 锁）。
        pool.append(&k, b"raced".to_vec(), 11).await.unwrap();

        // flush 成功后的 step 3：seal + truncate 到 hwm=10，两步都返回 Ok。
        pool.seal_active(&k).await.unwrap();
        pool.truncate_up_to(&k, 10).await.unwrap();

        let recovered = pool.recover().unwrap();
        let indices: Vec<u64> = recovered[0].1.iter().map(|r| r.index).collect();
        assert!(
            indices.contains(&1) && indices.contains(&10),
            "已 flush 的 seq 1..=10 应仍残留在 WAL 中（truncate 删不掉混装段），实得 {indices:?}"
        );
    }

    #[tokio::test]
    async fn truncate_removes_only_fully_flushed_sealed_segments() {
        let tmp = tempdir().unwrap();
        let pool = test_pool(tmp.path(), 512);
        let k = key_logs("org-a", "app");
        for i in 1..=20u64 {
            let payload = format!("{}-{i}", "x".repeat(60));
            pool.append(&k, payload.into_bytes(), i).await.unwrap();
        }
        let dir = key_dir(tmp.path(), &k);
        let before = SegmentWal::segment_paths_sorted(&dir).unwrap().len();

        // seq=0：什么都没落盘，一段都不该删。
        pool.truncate_up_to(&k, 0).await.unwrap();
        assert_eq!(
            SegmentWal::segment_paths_sorted(&dir).unwrap().len(),
            before,
            "seq=0 时不得删除任何 segment"
        );

        // 全部 flush 后：活跃段仍须保留，其余 sealed 段应被删光。
        pool.truncate_up_to(&k, 20).await.unwrap();
        let after = SegmentWal::segment_paths_sorted(&dir).unwrap();
        assert_eq!(after.len(), 1, "只应剩下活跃段，实剩 {}", after.len());

        // 幸存记录的 index 必须都 > 已 flush 的水位之外的部分仍可恢复。
        let recovered = pool.recover().unwrap();
        let recs = &recovered[0].1;
        assert!(
            recs.iter().all(|r| r.index >= 1 && r.index <= 20),
            "恢复出的记录 index 越界"
        );
    }

    #[tokio::test]
    async fn append_and_recover_round_trip() {
        let tmp = tempdir().unwrap();
        let pool = test_pool(tmp.path(), 1024 * 1024);
        let k = key_logs("org-a", "app");
        pool.append(&k, b"first".to_vec(), 1).await.unwrap();
        pool.append(&k, b"second".to_vec(), 2).await.unwrap();

        // drop pool, scan fs
        drop(pool);
        let pool2 = test_pool(tmp.path(), 1024 * 1024);
        let recovered = pool2.recover().unwrap();
        assert_eq!(recovered.len(), 1);
        let (rk, recs) = &recovered[0];
        assert_eq!(rk, &k);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].payload, b"first");
        assert_eq!(recs[1].payload, b"second");
    }

    #[tokio::test]
    async fn truncate_up_to_drops_sealed_segments_only() {
        let tmp = tempdir().unwrap();
        // segment size 小到一条记录就触发 rotation
        let pool = test_pool(tmp.path(), 64);
        let k = key_logs("org-a", "logs");
        for i in 1..=4u64 {
            pool.append(&k, i.to_le_bytes().to_vec(), i).await.unwrap();
        }
        // 应至少有 2 个 segments，最后一个是 active
        let segs_before = SegmentWal::segment_paths_sorted(key_dir(pool.root(), &k)).unwrap();
        assert!(segs_before.len() >= 2);
        // truncate 到 seq=2：sealed segments 中 record.index 全部 ≤ 2 的删，其它保留
        pool.truncate_up_to(&k, 2).await.unwrap();
        let segs_after = SegmentWal::segment_paths_sorted(key_dir(pool.root(), &k)).unwrap();
        assert!(
            segs_after.len() < segs_before.len(),
            "expected sealed segments to be removed: before={} after={}",
            segs_before.len(),
            segs_after.len()
        );
    }

    #[tokio::test]
    async fn append_rejects_unsafe_stream_name() {
        let tmp = tempdir().unwrap();
        let pool = test_pool(tmp.path(), 1024);
        let bad: WalKey = (
            Id::from_string("org-a"),
            StreamType::Logs,
            "../escape".into(),
            PhysicalDatasetKind::Raw,
        );
        let r = pool.append(&bad, b"x".to_vec(), 1).await;
        assert!(r.is_err());
    }

    /// 构造时传 `FsyncPolicy::EveryWrite { sync_level: Data }`，写 1 条后扫
    /// segment 文件验证 payload 完整 + CRC 校验通过（read_records_readonly 内做 CRC 校验）。
    #[tokio::test]
    async fn every_write_fsync_round_trip() {
        let tmp = tempdir().unwrap();
        let pool = WalPool::new(
            tmp.path(),
            1024 * 1024,
            FsyncPolicy::EveryWrite {
                sync_level: SyncLevel::DATA,
            },
            Arc::new(StaticTermSource(1)),
        );
        let k = key_logs("org-x", "app");
        pool.append(&k, b"payload-xyz".to_vec(), 42).await.unwrap();
        let scan = SegmentWal::read_records_readonly(key_dir(pool.root(), &k)).unwrap();
        assert_eq!(scan.records.len(), 1);
        assert_eq!(scan.records[0].payload, b"payload-xyz");
        assert_eq!(scan.records[0].index, 42);
        assert!(scan.errors.is_empty());
    }

    /// `StaticTermSource(7)` 注入后 append 一条，header 的 term 字段 == 7。
    #[tokio::test]
    async fn static_term_source_propagates_to_record_header() {
        let tmp = tempdir().unwrap();
        let pool = WalPool::new(
            tmp.path(),
            1024 * 1024,
            FsyncPolicy::none_default(),
            Arc::new(StaticTermSource(7)),
        );
        let k = key_logs("org-a", "app");
        pool.append(&k, b"hello".to_vec(), 1).await.unwrap();
        let scan = SegmentWal::read_records_readonly(key_dir(pool.root(), &k)).unwrap();
        assert_eq!(scan.records.len(), 1);
        assert_eq!(scan.records[0].term, 7);
    }

    /// spawn 8 个并发 append 到同 key，drop 后 wal_append_inflight{extend} == 0
    /// 且 wal_append_lock_wait_seconds_count{extend} 至少累加 8。
    /// 用 Extend 标签是为了与其它单测 (Logs/Metrics/Traces) 隔离。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn lock_wait_metrics_recorded_under_concurrent_append() {
        use crate::infra::ingester::metrics::{wal_inflight_count, wal_lock_wait_sample_count};

        let tmp = tempdir().unwrap();
        let pool = Arc::new(test_pool(tmp.path(), 1024 * 1024));
        let key: WalKey = (
            Id::from_string("org-c"),
            StreamType::Extend,
            "obs-key".into(),
            PhysicalDatasetKind::Raw,
        );
        let count_before = wal_lock_wait_sample_count("extend");

        let mut handles = Vec::new();
        for i in 0u64..8 {
            let pool = pool.clone();
            let k = key.clone();
            handles.push(tokio::spawn(async move {
                pool.append(&k, format!("payload-{i}").into_bytes(), i + 1)
                    .await
                    .unwrap();
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        let count_after = wal_lock_wait_sample_count("extend");
        assert!(
            count_after - count_before >= 8,
            "histogram delta = {} should >= 8",
            count_after - count_before
        );
        assert_eq!(wal_inflight_count("extend"), 0, "gauge must return to 0");
    }

    /// 自定义 TermSource，两次 append 之间 term 从 7 切到 9，两条 record 的
    /// header 分别为 7 / 9（验证 `set_term` 在 append 路径被正确调用）。
    #[tokio::test]
    async fn term_change_between_appends_reflected_in_record_headers() {
        struct AtomicTermSource(AtomicU64);
        impl TermSource for AtomicTermSource {
            fn current_term(&self) -> u64 {
                self.0.load(Ordering::SeqCst)
            }
        }

        let term = Arc::new(AtomicTermSource(AtomicU64::new(7)));
        let tmp = tempdir().unwrap();
        let pool = WalPool::new(
            tmp.path(),
            1024 * 1024,
            FsyncPolicy::none_default(),
            term.clone() as Arc<dyn TermSource>,
        );
        let k = key_logs("org-a", "app");
        pool.append(&k, b"first".to_vec(), 1).await.unwrap();
        term.0.store(9, Ordering::SeqCst);
        pool.append(&k, b"second".to_vec(), 2).await.unwrap();

        let scan = SegmentWal::read_records_readonly(key_dir(pool.root(), &k)).unwrap();
        assert_eq!(scan.records.len(), 2);
        assert_eq!(scan.records[0].term, 7);
        assert_eq!(scan.records[1].term, 9);
    }

    fn test_kek() -> CipherRootKey {
        // 32 字节全零 base64（仅测试）。
        CipherRootKey::from_base64("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").unwrap()
    }

    #[tokio::test]
    async fn encrypted_payload_is_ciphertext_on_disk_and_recovers() {
        let tmp = tempdir().unwrap();
        let kek = test_kek();
        let pool = test_pool(tmp.path(), 1024 * 1024).with_cipher(kek.clone());
        let k = key_logs("org-e", "app");
        pool.append(&k, b"secret-payload".to_vec(), 1)
            .await
            .unwrap();

        // 盘上 record 是密文（WEN1 前缀），不含明文。
        let scan = SegmentWal::read_records_readonly(key_dir(pool.root(), &k)).unwrap();
        assert!(
            scan.records[0].payload.starts_with(b"WEN1"),
            "stored payload must be encrypted"
        );
        assert_ne!(scan.records[0].payload, b"secret-payload");

        // 带同一 KEK recover → 还原明文。
        drop(pool);
        let pool2 = test_pool(tmp.path(), 1024 * 1024).with_cipher(kek);
        let recovered = pool2.recover().unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].1[0].payload, b"secret-payload");
    }

    #[tokio::test]
    async fn encrypted_record_without_key_errors_on_recover() {
        let tmp = tempdir().unwrap();
        let pool = test_pool(tmp.path(), 1024 * 1024).with_cipher(test_kek());
        let k = key_logs("org-e", "app");
        pool.append(&k, b"secret".to_vec(), 1).await.unwrap();
        drop(pool);
        // 无 KEK 的 pool 读到加密 record → 报错（数据加密但缺 key）。
        let pool_no_key = test_pool(tmp.path(), 1024 * 1024);
        assert!(pool_no_key.recover().is_err());
    }

    #[tokio::test]
    async fn plaintext_pool_recovers_when_encryption_off() {
        // encrypt=off（默认）：与现状一致，明文落盘 + recover。
        let tmp = tempdir().unwrap();
        let pool = test_pool(tmp.path(), 1024 * 1024);
        let k = key_logs("org-p", "app");
        pool.append(&k, b"plain".to_vec(), 1).await.unwrap();
        let scan = SegmentWal::read_records_readonly(key_dir(pool.root(), &k)).unwrap();
        assert_eq!(scan.records[0].payload, b"plain");
        drop(pool);
        let pool2 = test_pool(tmp.path(), 1024 * 1024);
        assert_eq!(pool2.recover().unwrap()[0].1[0].payload, b"plain");
    }
}
