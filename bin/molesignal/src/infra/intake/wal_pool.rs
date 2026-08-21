// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! WalPool：按 [`PhysicalDatasetId`] 维护独立 `SegmentWal` 实例。
//!
//! 目录布局只含稳定 ID，不含 stream name / signal 类型：
//!
//! ```text
//! {root}/{node_id}/{dataset_id}/CURRENT              当前 writer epoch（十进制）
//! {root}/{node_id}/{dataset_id}/{epoch}/IDENTITY     该 epoch 的身份清单（JSON）
//! {root}/{node_id}/{dataset_id}/{epoch}/wal-*.seg    记录段
//! {root}/{node_id}/{dataset_id}/quarantine/          损坏段隔离区
//! ```
//!
//! - **writer epoch**：进程每次取得某 dataset 的写入所有权（首次 append）时把
//!   `CURRENT` 原子推进一格并新建 epoch 目录；旧 epoch 目录只由启动恢复读取。
//!   `CURRENT` 无条件 fsync——epoch 号丢失会让 (epoch, sequence) 撞车，进而在
//!   flush 幂等去重时吞掉真实数据。
//! - **sequence**：per-dataset 单调递增（record header 的 `index`），新 epoch 从
//!   全部旧 epoch 的最大值 +1 续走，保证 buffer 高水位跨恢复仍然单调。
//!   record header 的 `term` 存 epoch。
//! - `append` 在 `SegmentWal` 互斥区内分配 sequence，文件内严格有序。
//! - `truncate_up_to(dataset, seq)` 只作用于当前 epoch 目录；旧 epoch 目录由
//!   恢复流程整目录清理（见 `wal_pool/recovery.rs`）。

mod recovery;

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use anyhow::{Context, Result, anyhow};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

pub use self::recovery::{WalEpochDir, WalRecoverySource};
use crate::{
    domain::storage::{DatasetTypeId, PhysicalDatasetId, WalCodecId, WalSequence, WriterEpoch},
    infra::{
        cipher::CipherRootKey,
        intake::metrics::{WalInflightGuard, observe_wal_lock_wait},
        segment_wal::{
            FsyncPolicy, SegmentWal, WalEntryType, scan_segment_max_index, sync_dir_parent_of,
        },
    },
    shared::ids::Id,
};

/// 一条 WAL 流的写入身份；由 dataset 解析层构造。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalStreamIdentity {
    pub organization_id: Id,
    pub dataset_id: PhysicalDatasetId,
    pub dataset_type: DatasetTypeId,
    pub wal_codec: WalCodecId,
}

/// Position durably assigned to one WAL record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalAppendPosition {
    pub writer_epoch: WriterEpoch,
    pub sequence: WalSequence,
}

/// epoch 目录内的身份清单。恢复时与目录路径互验，防止段文件被错放 / 错拷。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EpochIdentity {
    pub format_version: u32,
    pub organization_id: String,
    pub dataset_id: String,
    pub dataset_type: String,
    pub node_id: String,
    pub epoch: u64,
    pub wal_codec: String,
}

pub(crate) const EPOCH_IDENTITY_FORMAT_VERSION: u32 = 1;
pub(crate) const CURRENT_FILE: &str = "CURRENT";
pub(crate) const IDENTITY_FILE: &str = "IDENTITY";
pub(crate) const QUARANTINE_DIR: &str = "quarantine";

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

/// 原子写小文件：tmp + sync_all + rename + 父目录 sync_all。
fn write_file_durable(path: &Path, contents: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut file =
            std::fs::File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        use std::io::Write;
        file.write_all(contents)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
    sync_dir_parent_of(path, crate::infra::segment_wal::SyncLevel::ALL)?;
    Ok(())
}

struct DatasetWal {
    epoch: u64,
    epoch_dir: PathBuf,
    next_seq: AtomicU64,
    wal: Mutex<SegmentWal>,
}

pub struct WalPool {
    root: PathBuf,
    node_id: String,
    segment_size_bytes: usize,
    fsync_policy: FsyncPolicy,
    pools: DashMap<PhysicalDatasetId, Arc<DatasetWal>>,
    /// at-rest 加密 KEK；`None` = 明文落盘（默认）。`with_cipher` 注入。
    cipher: Option<CipherRootKey>,
}

impl WalPool {
    /// `segment_size_bytes` 为每个 segment 文件的滚动上限；`node_id` 进目录布局，
    /// 同一 dataset 在不同节点的 WAL 与 (epoch, sequence) 序号空间彼此独立。
    pub fn new(
        root: impl Into<PathBuf>,
        node_id: impl Into<String>,
        segment_size_bytes: usize,
        fsync_policy: FsyncPolicy,
    ) -> Self {
        Self {
            root: root.into(),
            node_id: node_id.into(),
            segment_size_bytes,
            fsync_policy,
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

    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub(crate) fn cipher(&self) -> Option<&CipherRootKey> {
        self.cipher.as_ref()
    }

    fn node_dir(&self) -> PathBuf {
        self.root.join(&self.node_id)
    }

    fn dataset_dir(&self, dataset_id: &PhysicalDatasetId) -> PathBuf {
        self.node_dir().join(dataset_id.as_str())
    }

    /// 读取 `CURRENT`；缺失 = 0，损坏显式报错（静默归零会造成 epoch 复用）。
    fn read_current_epoch(dataset_dir: &Path) -> Result<u64> {
        let path = dataset_dir.join(CURRENT_FILE);
        match std::fs::read_to_string(&path) {
            Ok(raw) => raw
                .trim()
                .parse::<u64>()
                .with_context(|| format!("corrupt CURRENT at {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(e) => Err(anyhow!("read CURRENT at {}: {e}", path.display())),
        }
    }

    /// 目录中现存 epoch 子目录的最大编号（防 `CURRENT` 写入与建目录间崩溃后回退）。
    fn max_existing_epoch(dataset_dir: &Path) -> Result<u64> {
        let mut max = 0u64;
        if dataset_dir.exists() {
            for entry in std::fs::read_dir(dataset_dir)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                if let Ok(epoch) = entry.file_name().to_string_lossy().parse::<u64>() {
                    max = max.max(epoch);
                }
            }
        }
        Ok(max)
    }

    /// 全部现存 epoch 目录里最大的 record index；新 epoch 的 sequence 从其 +1 续走。
    fn max_existing_sequence(dataset_dir: &Path) -> Result<u64> {
        let mut max = 0u64;
        if !dataset_dir.exists() {
            return Ok(max);
        }
        for entry in std::fs::read_dir(dataset_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir()
                || entry.file_name().to_string_lossy().parse::<u64>().is_err()
            {
                continue;
            }
            for segment in SegmentWal::segment_paths_sorted(entry.path())? {
                if let Some(index) = scan_segment_max_index(&segment)? {
                    max = max.max(index);
                }
            }
        }
        Ok(max)
    }

    /// 取得写入所有权：推进 `CURRENT`、建 epoch 目录、写 IDENTITY、打开 SegmentWal。
    fn acquire(&self, identity: &WalStreamIdentity) -> Result<DatasetWal> {
        let dataset_dir = self.dataset_dir(&identity.dataset_id);
        std::fs::create_dir_all(&dataset_dir)
            .with_context(|| format!("create wal dataset dir {}", dataset_dir.display()))?;

        let epoch = Self::read_current_epoch(&dataset_dir)?
            .max(Self::max_existing_epoch(&dataset_dir)?)
            + 1;
        let next_seq = Self::max_existing_sequence(&dataset_dir)? + 1;
        write_file_durable(
            &dataset_dir.join(CURRENT_FILE),
            epoch.to_string().as_bytes(),
        )?;

        let epoch_dir = dataset_dir.join(epoch.to_string());
        std::fs::create_dir_all(&epoch_dir)?;
        let manifest = EpochIdentity {
            format_version: EPOCH_IDENTITY_FORMAT_VERSION,
            organization_id: identity.organization_id.as_str().to_owned(),
            dataset_id: identity.dataset_id.as_str().to_owned(),
            dataset_type: identity.dataset_type.as_str().to_owned(),
            node_id: self.node_id.clone(),
            epoch,
            wal_codec: identity.wal_codec.as_str().to_owned(),
        };
        write_file_durable(
            &epoch_dir.join(IDENTITY_FILE),
            &serde_json::to_vec_pretty(&manifest)?,
        )?;

        let wal = SegmentWal::new(
            &epoch_dir,
            self.segment_size_bytes,
            DEFAULT_BUFFER_BYTES,
            None,
            None,
            self.fsync_policy,
            epoch,
        )
        .with_context(|| format!("open wal at {}", epoch_dir.display()))?;
        Ok(DatasetWal {
            epoch,
            epoch_dir,
            next_seq: AtomicU64::new(next_seq),
            wal: Mutex::new(wal),
        })
    }

    fn open_or_create(&self, identity: &WalStreamIdentity) -> Result<Arc<DatasetWal>> {
        if let Some(entry) = self.pools.get(&identity.dataset_id) {
            return Ok(entry.clone());
        }
        // 竞争建 entry：entry() 持 shard 锁串行化同 dataset 的 acquire，
        // 保证一个进程内每个 dataset 只推进一次 epoch。
        let entry = self
            .pools
            .entry(identity.dataset_id.clone())
            .or_try_insert_with(|| self.acquire(identity).map(Arc::new))?;
        Ok(entry.clone())
    }

    /// 当前进程为该 dataset 取得的 writer epoch；尚未 append 过时为 None。
    pub fn current_epoch(&self, dataset_id: &PhysicalDatasetId) -> Option<u64> {
        self.pools.get(dataset_id).map(|entry| entry.epoch)
    }

    /// 兼容入口：只返回 sequence。需要构造 flush provenance 的写入方应使用
    /// [`Self::append_position`]，避免在 append 后另读 epoch。
    pub async fn append(&self, identity: &WalStreamIdentity, payload: Vec<u8>) -> Result<u64> {
        Ok(self.append_position(identity, payload).await?.sequence.0)
    }

    /// 追加一条 WAL 记录（Normal entry），返回原子取得的 epoch + sequence。
    ///
    /// sequence 在 `SegmentWal` 互斥区内分配，段文件里严格单调；写入本身是同步
    /// 文件 IO，跑在 blocking 池上（`Batch` fsync 策略会在临界区里同步刷盘）。
    #[tracing::instrument(
        name = "wal.append",
        skip_all,
        fields(
            otel.kind = "internal",
            molesignal.wal.payload_bytes = payload.len(),
            molesignal.dataset.r#type = %identity.dataset_type
        )
    )]
    pub async fn append_position(
        &self,
        identity: &WalStreamIdentity,
        payload: Vec<u8>,
    ) -> Result<WalAppendPosition> {
        let entry = self.open_or_create(identity)?;
        let writer_epoch = WriterEpoch(entry.epoch);
        let dataset_type_label = identity.dataset_type.as_str().to_owned();
        let payload = match &self.cipher {
            Some(k) => wal_encrypt(k, &payload)?,
            None => payload,
        };
        let lock_started = Instant::now();
        tokio::task::spawn_blocking(move || {
            let mut guard = entry.wal.blocking_lock();
            observe_wal_lock_wait(&dataset_type_label, lock_started.elapsed().as_secs_f64());
            let _inflight = WalInflightGuard::enter(&dataset_type_label);
            let seq = entry.next_seq.fetch_add(1, Ordering::SeqCst);
            guard.write_raw(WalEntryType::Normal, &payload, seq)?;
            Ok(WalAppendPosition {
                writer_epoch,
                sequence: WalSequence(seq),
            })
        })
        .await
        .map_err(|e| anyhow!("wal append join: {e}"))?
    }

    /// 强制封口当前活跃 segment（若非空）。flush 在 truncate 前调用，
    /// 把所有已写 record 推到 sealed segment，便于随后被整段删除。
    ///
    /// 该 dataset 本进程尚未写过时 no-op。
    pub async fn seal_active(&self, dataset_id: &PhysicalDatasetId) -> Result<()> {
        let Some(entry) = self.pools.get(dataset_id).map(|entry| entry.clone()) else {
            return Ok(());
        };
        let mut guard = entry.wal.lock().await;
        guard.seal_active()?;
        Ok(())
    }

    /// 把当前 epoch 目录里 record index ≤ `seq` 的 sealed segments 整段删除
    /// （活跃 segment 保留）。旧 epoch 目录不在此处理，由恢复流程清理。
    pub async fn truncate_up_to(&self, dataset_id: &PhysicalDatasetId, seq: u64) -> Result<()> {
        let Some(entry) = self.pools.get(dataset_id).map(|entry| entry.clone()) else {
            return Ok(());
        };
        let dir = entry.epoch_dir.clone();
        tokio::task::spawn_blocking(move || truncate_dir_blocking(&dir, seq))
            .await
            .map_err(|e| anyhow!("truncate join: {e}"))?
    }
}

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

#[cfg(test)]
pub(crate) mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::{
        domain::storage::type_id,
        infra::segment_wal::{SyncLevel, scan_segment_file_readonly},
    };

    pub(crate) fn identity(dataset: &str) -> WalStreamIdentity {
        WalStreamIdentity {
            organization_id: Id::from_string("org-a"),
            dataset_id: PhysicalDatasetId::from_string(dataset),
            dataset_type: DatasetTypeId::builtin(type_id::builtin::DATASET_LOG_RECORDS),
            wal_codec: WalCodecId::builtin(type_id::builtin::WAL_CODEC_ROW_BATCH),
        }
    }

    pub(crate) fn test_pool(root: &Path, seg: usize) -> WalPool {
        WalPool::new(root, "node-test", seg, FsyncPolicy::none_default())
    }

    fn epoch_dir(pool: &WalPool, dataset: &str, epoch: u64) -> PathBuf {
        pool.root()
            .join(pool.node_id())
            .join(dataset)
            .join(epoch.to_string())
    }

    #[tokio::test]
    async fn append_assigns_monotonic_sequences_and_returns_them() {
        let tmp = tempdir().unwrap();
        let pool = test_pool(tmp.path(), 1024 * 1024);
        let id = identity("ds-1");
        assert_eq!(pool.append(&id, b"a".to_vec()).await.unwrap(), 1);
        assert_eq!(pool.append(&id, b"b".to_vec()).await.unwrap(), 2);
        assert_eq!(pool.current_epoch(&id.dataset_id), Some(1));

        let scan = scan_segment_file_readonly(
            SegmentWal::segment_paths_sorted(epoch_dir(&pool, "ds-1", 1)).unwrap()[0].clone(),
        )
        .unwrap();
        assert_eq!(scan.records.len(), 2);
        assert_eq!(scan.records[0].index, 1);
        assert_eq!(scan.records[0].term, 1, "record term must carry the epoch");
    }

    #[tokio::test]
    async fn reopen_bumps_epoch_and_continues_sequence() {
        let tmp = tempdir().unwrap();
        {
            let pool = test_pool(tmp.path(), 1024 * 1024);
            let id = identity("ds-1");
            pool.append(&id, b"a".to_vec()).await.unwrap();
            pool.append(&id, b"b".to_vec()).await.unwrap();
        }
        let pool = test_pool(tmp.path(), 1024 * 1024);
        let id = identity("ds-1");
        let seq = pool.append(&id, b"c".to_vec()).await.unwrap();
        assert_eq!(seq, 3, "sequence continues past the previous epoch");
        assert_eq!(pool.current_epoch(&id.dataset_id), Some(2));

        let scan = scan_segment_file_readonly(
            SegmentWal::segment_paths_sorted(epoch_dir(&pool, "ds-1", 2)).unwrap()[0].clone(),
        )
        .unwrap();
        assert_eq!(scan.records[0].term, 2);
    }

    #[tokio::test]
    async fn identity_manifest_written_per_epoch() {
        let tmp = tempdir().unwrap();
        let pool = test_pool(tmp.path(), 1024 * 1024);
        let id = identity("ds-1");
        pool.append(&id, b"a".to_vec()).await.unwrap();

        let manifest: EpochIdentity = serde_json::from_slice(
            &std::fs::read(epoch_dir(&pool, "ds-1", 1).join(IDENTITY_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.epoch, 1);
        assert_eq!(manifest.dataset_id, "ds-1");
        assert_eq!(manifest.node_id, "node-test");
        assert_eq!(manifest.dataset_type, type_id::builtin::DATASET_LOG_RECORDS);
        assert_eq!(manifest.wal_codec, type_id::builtin::WAL_CODEC_ROW_BATCH);
    }

    #[tokio::test]
    async fn corrupt_current_file_fails_loudly_instead_of_reusing_epochs() {
        let tmp = tempdir().unwrap();
        let id = identity("ds-1");
        {
            let pool = test_pool(tmp.path(), 1024 * 1024);
            pool.append(&id, b"a".to_vec()).await.unwrap();
        }
        let current = tmp.path().join("node-test").join("ds-1").join(CURRENT_FILE);
        std::fs::write(&current, b"not-a-number").unwrap();
        let pool = test_pool(tmp.path(), 1024 * 1024);
        assert!(pool.append(&id, b"b".to_vec()).await.is_err());
    }

    #[tokio::test]
    async fn truncate_removes_only_fully_flushed_sealed_segments() {
        let tmp = tempdir().unwrap();
        let pool = test_pool(tmp.path(), 512);
        let id = identity("ds-1");
        for i in 1..=20u64 {
            let payload = format!("{}-{i}", "x".repeat(60));
            assert_eq!(pool.append(&id, payload.into_bytes()).await.unwrap(), i);
        }
        let dir = epoch_dir(&pool, "ds-1", 1);
        let before = SegmentWal::segment_paths_sorted(&dir).unwrap().len();
        assert!(before > 1, "需要多个 segment 才有意义");

        // seq=0：什么都没落盘，一段都不该删。
        pool.truncate_up_to(&id.dataset_id, 0).await.unwrap();
        assert_eq!(
            SegmentWal::segment_paths_sorted(&dir).unwrap().len(),
            before
        );

        pool.seal_active(&id.dataset_id).await.unwrap();
        pool.truncate_up_to(&id.dataset_id, 20).await.unwrap();
        let after = SegmentWal::segment_paths_sorted(&dir).unwrap();
        assert_eq!(after.len(), 1, "只应剩下活跃段，实剩 {}", after.len());
    }

    #[tokio::test]
    async fn every_write_fsync_round_trip() {
        let tmp = tempdir().unwrap();
        let pool = WalPool::new(
            tmp.path(),
            "node-test",
            1024 * 1024,
            FsyncPolicy::EveryWrite {
                sync_level: SyncLevel::DATA,
            },
        );
        let id = identity("ds-x");
        assert_eq!(pool.append(&id, b"payload-xyz".to_vec()).await.unwrap(), 1);
        let scan = SegmentWal::read_records_readonly(epoch_dir(&pool, "ds-x", 1)).unwrap();
        assert_eq!(scan.records.len(), 1);
        assert_eq!(scan.records[0].payload, b"payload-xyz");
        assert!(scan.errors.is_empty());
    }

    /// spawn 8 个并发 append 到同 dataset，结束后 inflight gauge 归零且 lock-wait
    /// histogram 至少累加 8。用专属 dataset type 标签与其它单测隔离。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn lock_wait_metrics_recorded_under_concurrent_append() {
        use crate::infra::intake::metrics::{wal_inflight_count, wal_lock_wait_sample_count};

        let tmp = tempdir().unwrap();
        let pool = std::sync::Arc::new(test_pool(tmp.path(), 1024 * 1024));
        let mut id = identity("ds-metrics");
        id.dataset_type = DatasetTypeId::new("vendor.metrics_probe").unwrap();
        let label = id.dataset_type.as_str().to_owned();
        let count_before = wal_lock_wait_sample_count(&label);

        let mut handles = Vec::new();
        for i in 0u64..8 {
            let pool = pool.clone();
            let id = id.clone();
            handles.push(tokio::spawn(async move {
                pool.append(&id, format!("payload-{i}").into_bytes())
                    .await
                    .unwrap();
            }));
        }
        let mut seqs = Vec::new();
        for handle in handles {
            handle.await.unwrap();
        }
        // 并发分配的 sequence 必须无重复、无空洞。
        let sources = {
            let scan = SegmentWal::read_records_readonly(
                tmp.path().join("node-test").join("ds-metrics").join("1"),
            )
            .unwrap();
            for record in &scan.records {
                seqs.push(record.index);
            }
            scan.records.len()
        };
        assert_eq!(sources, 8);
        seqs.sort_unstable();
        assert_eq!(seqs, (1..=8).collect::<Vec<_>>());

        let count_after = wal_lock_wait_sample_count(&label);
        assert!(
            count_after - count_before >= 8,
            "histogram delta = {} should >= 8",
            count_after - count_before
        );
        assert_eq!(wal_inflight_count(&label), 0, "gauge must return to 0");
    }

    pub(crate) fn test_kek() -> CipherRootKey {
        // 32 字节全零 base64（仅测试）。
        CipherRootKey::from_base64("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").unwrap()
    }

    #[tokio::test]
    async fn encrypted_payload_is_ciphertext_on_disk() {
        let tmp = tempdir().unwrap();
        let pool = test_pool(tmp.path(), 1024 * 1024).with_cipher(test_kek());
        let id = identity("ds-e");
        pool.append(&id, b"secret-payload".to_vec()).await.unwrap();

        let scan = SegmentWal::read_records_readonly(epoch_dir(&pool, "ds-e", 1)).unwrap();
        assert!(
            scan.records[0].payload.starts_with(b"WEN1"),
            "stored payload must be encrypted"
        );
        assert_ne!(scan.records[0].payload, b"secret-payload");
    }
}
