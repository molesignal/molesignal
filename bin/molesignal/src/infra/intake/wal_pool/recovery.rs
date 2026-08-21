// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! WAL 启动恢复：枚举、校验与读取旧 epoch 目录。
//!
//! 恢复只回放**未提交**的记录，且必须有界：
//! - 枚举阶段只列目录、验 IDENTITY，不读 payload；
//! - 读取以单个 segment 文件为粒度（大小受 `segment_size_mb` 约束），调用方
//!   按累计字节数决定何时强制 flush 腾空 buffer；
//! - 活跃段（每个 epoch 目录最后一个 segment）的不完整尾部截断到最后一条有效
//!   记录；sealed 段损坏时读出可校验前缀后把原文件挪进隔离区并报告，不静默跳过；
//! - IDENTITY 缺失或与路径不符的 epoch 目录整体隔离——错放的段宁可不回放。
//!
//! 回放完成且落盘成功后由调用方用 [`WalPool::purge_epoch_dirs`] 整目录清理。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use super::{
    CURRENT_FILE, EPOCH_IDENTITY_FORMAT_VERSION, EpochIdentity, IDENTITY_FILE, QUARANTINE_DIR,
    WAL_ENC_MAGIC, WalPool, wal_decrypt_body,
};
use crate::{
    domain::storage::{DatasetTypeId, PhysicalDatasetId},
    infra::{
        intake::metrics::{inc_wal_recovery_quarantined, inc_wal_recovery_tail_truncated},
        segment_wal::{SegmentWal, WalRecord, scan_segment_file_readonly},
    },
    shared::ids::Id,
};

/// 一个待回放的 epoch 目录。
#[derive(Debug, Clone)]
pub struct WalEpochDir {
    pub epoch: u64,
    pub dir: PathBuf,
    /// 按 segment 序号升序；最后一个是该 epoch 的活跃段。
    pub segments: Vec<PathBuf>,
}

/// 一个 dataset 的全部待回放 epoch（升序）。身份来自 IDENTITY 清单。
#[derive(Debug, Clone)]
pub struct WalRecoverySource {
    pub dataset_id: PhysicalDatasetId,
    pub organization_id: Id,
    pub dataset_type: DatasetTypeId,
    pub epoch_dirs: Vec<WalEpochDir>,
}

impl WalRecoverySource {
    /// 全部段文件总字节数；恢复调度按它排序 / 报告。
    pub fn total_bytes(&self) -> u64 {
        self.epoch_dirs
            .iter()
            .flat_map(|epoch| epoch.segments.iter())
            .filter_map(|segment| std::fs::metadata(segment).ok().map(|meta| meta.len()))
            .sum()
    }
}

fn quarantine_path(dataset_dir: &Path, epoch: Option<u64>, name: &str) -> PathBuf {
    let quarantine = dataset_dir.join(QUARANTINE_DIR);
    match epoch {
        Some(epoch) => quarantine.join(format!("{epoch}-{name}")),
        None => quarantine.join(name),
    }
}

/// 把文件或目录挪进 dataset 的隔离区（保留原始字节供排障），并计数报告。
fn quarantine(dataset_dir: &Path, source: &Path, epoch: Option<u64>, reason: &str) -> Result<()> {
    let target = quarantine_path(
        dataset_dir,
        epoch,
        &source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unnamed".to_owned()),
    );
    std::fs::create_dir_all(target.parent().expect("quarantine path has parent"))?;
    std::fs::rename(source, &target)
        .with_context(|| format!("quarantine {} -> {}", source.display(), target.display()))?;
    inc_wal_recovery_quarantined();
    tracing::error!(
        source = %source.display(),
        target = %target.display(),
        reason,
        "wal recovery quarantined corrupt entry"
    );
    Ok(())
}

fn load_epoch_identity(dir: &Path) -> Result<EpochIdentity> {
    let raw = std::fs::read(dir.join(IDENTITY_FILE))
        .with_context(|| format!("read IDENTITY in {}", dir.display()))?;
    let identity: EpochIdentity = serde_json::from_slice(&raw)
        .with_context(|| format!("parse IDENTITY in {}", dir.display()))?;
    if identity.format_version != EPOCH_IDENTITY_FORMAT_VERSION {
        return Err(anyhow!(
            "unsupported IDENTITY format_version {}",
            identity.format_version
        ));
    }
    Ok(identity)
}

impl WalPool {
    /// 枚举本节点全部待回放的 WAL：`{root}/{node_id}` 下每个 dataset 目录里
    /// 尚存的 epoch 子目录。必须在首次 append 之前调用——此后新 epoch 目录会
    /// 与恢复目标混在一起。
    pub fn recovery_sources(&self) -> Result<Vec<WalRecoverySource>> {
        let node_dir = self.root().join(self.node_id());
        let mut sources = Vec::new();
        if !node_dir.exists() {
            return Ok(sources);
        }
        for dataset_entry in std::fs::read_dir(&node_dir)? {
            let dataset_entry = dataset_entry?;
            if !dataset_entry.file_type()?.is_dir() {
                continue;
            }
            let dataset_dir = dataset_entry.path();
            let dataset_name = dataset_entry.file_name().to_string_lossy().into_owned();
            if let Some(source) = self.collect_dataset(&dataset_dir, &dataset_name)? {
                sources.push(source);
            }
        }
        sources.sort_by(|a, b| a.dataset_id.as_str().cmp(b.dataset_id.as_str()));
        Ok(sources)
    }

    fn collect_dataset(
        &self,
        dataset_dir: &Path,
        dataset_name: &str,
    ) -> Result<Option<WalRecoverySource>> {
        let mut epoch_dirs = Vec::new();
        let mut identity: Option<(Id, DatasetTypeId)> = None;
        for entry in std::fs::read_dir(dataset_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !entry.file_type()?.is_dir() {
                if name != CURRENT_FILE && !name.ends_with(".tmp") {
                    tracing::warn!(path = %entry.path().display(), "unexpected file in wal dataset dir");
                }
                continue;
            }
            if name == QUARANTINE_DIR {
                continue;
            }
            let Ok(epoch) = name.parse::<u64>() else {
                tracing::warn!(path = %entry.path().display(), "non-epoch dir in wal dataset dir");
                continue;
            };
            let dir = entry.path();
            let manifest = match load_epoch_identity(&dir) {
                Ok(manifest) => manifest,
                Err(error) => {
                    quarantine(dataset_dir, &dir, None, &format!("bad IDENTITY: {error}"))?;
                    continue;
                }
            };
            if manifest.epoch != epoch
                || manifest.dataset_id != dataset_name
                || manifest.node_id != self.node_id()
            {
                quarantine(dataset_dir, &dir, None, "IDENTITY does not match path")?;
                continue;
            }
            let segments = SegmentWal::segment_paths_sorted(&dir)?;
            identity = Some((
                Id::from_string(manifest.organization_id),
                manifest
                    .dataset_type
                    .parse::<DatasetTypeId>()
                    .map_err(|error| anyhow!("IDENTITY dataset_type: {error}"))?,
            ));
            epoch_dirs.push(WalEpochDir {
                epoch,
                dir,
                segments,
            });
        }
        let Some((organization_id, dataset_type)) = identity else {
            return Ok(None);
        };
        epoch_dirs.sort_by_key(|epoch| epoch.epoch);
        Ok(Some(WalRecoverySource {
            dataset_id: PhysicalDatasetId::from_string(dataset_name),
            organization_id,
            dataset_type,
            epoch_dirs,
        }))
    }

    /// 读取单个 segment 的全部记录（解密后返回）。
    ///
    /// `is_active_tail` = 该段是所属 epoch 目录的最后一个 segment：尾部损坏时
    /// 截断文件到最后一条有效记录。sealed 段（非最后）损坏时返回可校验前缀，
    /// 并把原文件挪进隔离区报告。
    pub fn read_segment_records(
        &self,
        dataset_dir_segment: &Path,
        is_active_tail: bool,
    ) -> Result<Vec<WalRecord>> {
        let scan = scan_segment_file_readonly(dataset_dir_segment)?;
        if let Some(offset) = scan.tail_error_offset {
            if is_active_tail {
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .open(dataset_dir_segment)?;
                file.set_len(offset as u64)?;
                file.sync_all()?;
                inc_wal_recovery_tail_truncated();
                tracing::warn!(
                    segment = %dataset_dir_segment.display(),
                    offset,
                    error = scan.tail_error.as_deref().unwrap_or("unknown"),
                    "wal recovery truncated incomplete active tail"
                );
            } else {
                let dataset_dir = dataset_dir_segment
                    .parent()
                    .and_then(Path::parent)
                    .ok_or_else(|| anyhow!("segment path has no dataset dir"))?;
                quarantine(
                    dataset_dir,
                    dataset_dir_segment,
                    dataset_dir_segment
                        .parent()
                        .and_then(|dir| dir.file_name())
                        .and_then(|name| name.to_string_lossy().parse::<u64>().ok()),
                    scan.tail_error
                        .as_deref()
                        .unwrap_or("corrupt sealed segment"),
                )?;
            }
        }
        let mut records = scan.records;
        for record in &mut records {
            if record.payload.starts_with(WAL_ENC_MAGIC) {
                record.payload = wal_decrypt_body(self.cipher(), &record.payload)?;
            }
        }
        Ok(records)
    }

    /// 回放 + 落盘成功后整目录清理旧 epoch。只删除显式传入的目录，
    /// 不碰本进程随后新建的 epoch 与隔离区。
    pub fn purge_epoch_dirs(&self, epoch_dirs: &[WalEpochDir]) -> Result<()> {
        for epoch in epoch_dirs {
            if epoch.dir.exists() {
                std::fs::remove_dir_all(&epoch.dir)
                    .with_context(|| format!("purge wal epoch dir {}", epoch.dir.display()))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::infra::intake::wal_pool::tests::{identity, test_kek, test_pool};

    #[tokio::test]
    async fn recovery_lists_prior_epochs_and_purge_removes_them() {
        let tmp = tempdir().unwrap();
        let id = identity("ds-1");
        {
            let pool = test_pool(tmp.path(), 1024 * 1024);
            pool.append(&id, b"one".to_vec()).await.unwrap();
        }
        {
            let pool = test_pool(tmp.path(), 1024 * 1024);
            pool.append(&id, b"two".to_vec()).await.unwrap();
        }

        let pool = test_pool(tmp.path(), 1024 * 1024);
        let sources = pool.recovery_sources().unwrap();
        assert_eq!(sources.len(), 1);
        let source = &sources[0];
        assert_eq!(source.dataset_id, id.dataset_id);
        assert_eq!(source.organization_id, id.organization_id);
        assert_eq!(source.dataset_type, id.dataset_type);
        assert_eq!(
            source
                .epoch_dirs
                .iter()
                .map(|epoch| epoch.epoch)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );

        let mut payloads = Vec::new();
        for epoch in &source.epoch_dirs {
            for (position, segment) in epoch.segments.iter().enumerate() {
                let is_last = position + 1 == epoch.segments.len();
                for record in pool.read_segment_records(segment, is_last).unwrap() {
                    payloads.push((record.term, record.index, record.payload));
                }
            }
        }
        assert_eq!(
            payloads,
            vec![(1, 1, b"one".to_vec()), (2, 2, b"two".to_vec())],
            "epoch in term, sequence continuous across epochs"
        );

        pool.purge_epoch_dirs(&source.epoch_dirs).unwrap();
        assert!(pool.recovery_sources().unwrap().is_empty());

        // epoch 靠 CURRENT 文件续走（purge 不删它）；sequence 在旧 epoch 全部
        // 回放落盘并清理后从 1 重开——(epoch, sequence) 组合仍然唯一。
        let seq = pool.append(&id, b"three".to_vec()).await.unwrap();
        assert_eq!(seq, 1);
        assert_eq!(pool.current_epoch(&id.dataset_id), Some(3));
    }

    #[tokio::test]
    async fn encrypted_records_decrypt_during_recovery_and_error_without_key() {
        let tmp = tempdir().unwrap();
        let id = identity("ds-e");
        {
            let pool = test_pool(tmp.path(), 1024 * 1024).with_cipher(test_kek());
            pool.append(&id, b"secret".to_vec()).await.unwrap();
        }
        let pool = test_pool(tmp.path(), 1024 * 1024).with_cipher(test_kek());
        let source = &pool.recovery_sources().unwrap()[0];
        let records = pool
            .read_segment_records(&source.epoch_dirs[0].segments[0], true)
            .unwrap();
        assert_eq!(records[0].payload, b"secret");

        let pool_no_key = test_pool(tmp.path(), 1024 * 1024);
        let source = &pool_no_key.recovery_sources().unwrap()[0];
        assert!(
            pool_no_key
                .read_segment_records(&source.epoch_dirs[0].segments[0], true)
                .is_err()
        );
    }

    #[tokio::test]
    async fn epoch_dir_with_mismatched_identity_is_quarantined() {
        let tmp = tempdir().unwrap();
        let id = identity("ds-1");
        {
            let pool = test_pool(tmp.path(), 1024 * 1024);
            pool.append(&id, b"one".to_vec()).await.unwrap();
        }
        // 篡改 IDENTITY 的 dataset_id → 目录必须被隔离，不参与回放。
        let epoch_dir = tmp.path().join("node-test").join("ds-1").join("1");
        let manifest_path = epoch_dir.join(IDENTITY_FILE);
        let mut manifest: EpochIdentity =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        manifest.dataset_id = "ds-other".into();
        std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();

        let pool = test_pool(tmp.path(), 1024 * 1024);
        assert!(pool.recovery_sources().unwrap().is_empty());
        assert!(
            tmp.path()
                .join("node-test")
                .join("ds-1")
                .join(QUARANTINE_DIR)
                .join("1")
                .join(IDENTITY_FILE)
                .exists(),
            "mismatched epoch dir must land in quarantine"
        );
    }

    #[tokio::test]
    async fn active_tail_is_truncated_and_sealed_corruption_is_quarantined() {
        use std::io::Write;

        let tmp = tempdir().unwrap();
        let id = identity("ds-1");
        {
            let pool = test_pool(tmp.path(), 128);
            // 小段逼出多个 sealed segment。
            for i in 0..6u64 {
                pool.append(&id, format!("payload-{i}-{}", "x".repeat(40)).into_bytes())
                    .await
                    .unwrap();
            }
        }
        let pool = test_pool(tmp.path(), 128);
        let source = pool.recovery_sources().unwrap().remove(0);
        let segments = &source.epoch_dirs[0].segments;
        assert!(segments.len() >= 2);

        // 活跃段（最后一个）追加半条垃圾 → 恢复读取应截尾成功。
        let active = segments.last().unwrap();
        let clean_len = std::fs::metadata(active).unwrap().len();
        {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(active)
                .unwrap();
            file.write_all(&[0xCA, 0xFE, 0x02, 0x00, 0x01]).unwrap();
        }
        let records = pool.read_segment_records(active, true).unwrap();
        assert!(!records.is_empty());
        assert_eq!(
            std::fs::metadata(active).unwrap().len(),
            clean_len,
            "incomplete tail must be truncated back to the last valid record"
        );

        // sealed 段中部损坏 → 可校验前缀返回，原文件进隔离区。
        let sealed = &segments[0];
        let good = pool.read_segment_records(sealed, false).unwrap();
        assert!(!good.is_empty());
        let mut bytes = std::fs::read(sealed).unwrap();
        let tail = bytes.len() - 8;
        bytes[tail..].fill(0xFF);
        std::fs::write(sealed, &bytes).unwrap();
        let salvaged = pool.read_segment_records(sealed, false).unwrap();
        assert!(salvaged.len() < good.len());
        assert!(
            !sealed.exists(),
            "corrupt sealed segment must be moved away"
        );
        let quarantine_dir = tmp
            .path()
            .join("node-test")
            .join("ds-1")
            .join(QUARANTINE_DIR);
        assert!(
            std::fs::read_dir(&quarantine_dir).unwrap().next().is_some(),
            "quarantine dir must hold the corrupt segment"
        );
    }

    #[tokio::test]
    async fn clean_shutdown_with_empty_epochs_yields_no_sources() {
        let tmp = tempdir().unwrap();
        let id = identity("ds-1");
        {
            let pool = test_pool(tmp.path(), 1024 * 1024);
            pool.append(&id, b"one".to_vec()).await.unwrap();
            pool.seal_active(&id.dataset_id).await.unwrap();
            pool.truncate_up_to(&id.dataset_id, 1).await.unwrap();
        }
        // seal 把 seq=1 推进 sealed 段，truncate 整段删除；剩下的活跃段为空。
        // source 仍会被枚举（目录还在），但可回放记录必须为 0——不产生重复数据。
        let pool = test_pool(tmp.path(), 1024 * 1024);
        let sources = pool.recovery_sources().unwrap();
        let mut replayable = 0usize;
        for source in &sources {
            for epoch in &source.epoch_dirs {
                for (position, segment) in epoch.segments.iter().enumerate() {
                    let is_last = position + 1 == epoch.segments.len();
                    replayable += pool.read_segment_records(segment, is_last).unwrap().len();
                }
            }
        }
        assert_eq!(replayable, 0, "干净关机后不得回放出任何记录");
    }
}
