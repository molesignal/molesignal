// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Segment WAL 写入器。
//!
//! 通用入口：`write_raw` / `write_config` / `write_snapshot_mark`。
//! payload 编码由调用方决定；当 lz4 压缩后更短时自动启用（见 `FLAG_LZ4_BIT`）。

use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use anyhow::Result;
use crc32c::crc32c_append;
use lz4_flex::block::compress_prepend_size;

use super::{
    WAL_SEGMENT_EXT, WAL_SEGMENT_PREFIX,
    cleanup::evict_old_segments,
    metrics::{fsync_kind, inc_fsync_error},
    reader::{read_segment_file, scan_segment_file_readonly},
    types::*,
};

/// Segment WAL 写入器。
pub struct SegmentWal {
    wal_dir: PathBuf,
    segment_idx: u64,
    current_size: u64,
    segment_size: u64,
    writer: BufWriter<File>,
    max_segments: Option<usize>,
    max_total_size_bytes: Option<u64>,
    fsync_policy: FsyncPolicy,
    pending_sync: usize,
    last_sync: std::time::Instant,
    current_term: u64,
}

impl SegmentWal {
    fn segment_paths_from_dir(wal_dir: &Path) -> Result<Vec<PathBuf>> {
        Ok(list_segments(wal_dir)?
            .into_iter()
            .map(|idx| wal_dir.join(format!("{WAL_SEGMENT_PREFIX}{idx:06}{WAL_SEGMENT_EXT}")))
            .collect())
    }

    /// 创建或打开 Segment WAL（续写最后一个 segment）。
    pub fn new<P: AsRef<Path>>(
        wal_dir: P,
        segment_size: usize,
        buffer_size: usize,
        max_segments: Option<usize>,
        max_total_size_bytes: Option<u64>,
        fsync_policy: FsyncPolicy,
        initial_term: u64,
    ) -> Result<Self> {
        let wal_dir = wal_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&wal_dir)?;

        let segment_size = segment_size as u64;
        let (segment_idx, current_size, file) = Self::open_latest_or_create(&wal_dir)?;

        Ok(Self {
            wal_dir,
            segment_idx,
            current_size,
            segment_size,
            writer: BufWriter::with_capacity(buffer_size, file),
            max_segments,
            max_total_size_bytes,
            fsync_policy,
            pending_sync: 0,
            last_sync: std::time::Instant::now(),
            current_term: initial_term,
        })
    }

    pub fn set_term(&mut self, term: u64) {
        self.current_term = term;
    }

    /// 当前写入 term（与 record header `term` 字段同步）。
    pub fn current_term(&self) -> u64 {
        self.current_term
    }

    /// 写入 CONFIG 记录（payload 由调用方编码，常用于 schema / 配置变更）。
    pub fn write_config(&mut self, payload: &[u8], index: u64) -> Result<()> {
        self.write_record(WalEntryType::Config, payload, index)
    }

    /// 写入任意类型的 WAL 记录。
    pub fn write_raw(
        &mut self,
        entry_type: WalEntryType,
        payload: &[u8],
        index: u64,
    ) -> Result<()> {
        self.write_record(entry_type, payload, index)
    }

    /// 写入 SNAPSHOT_MARK 哨兵记录（payload = snapshot_index 的 8B LE）。
    pub fn write_snapshot_mark(&mut self, snapshot_index: u64) -> Result<()> {
        let payload = snapshot_index.to_le_bytes();
        self.write_record(WalEntryType::SnapshotMark, &payload, snapshot_index)
    }

    /// 底层写入：32 字节头（含 CRC32C）+ payload。
    fn write_record(&mut self, entry_type: WalEntryType, raw: &[u8], index: u64) -> Result<()> {
        let compressed = compress_prepend_size(raw);
        let (flag, payload) = if compressed.len() < raw.len() {
            (entry_type as u8 | FLAG_LZ4_BIT, compressed.as_slice())
        } else {
            (entry_type as u8, raw)
        };

        let payload_len = payload.len() as u32;
        let mut header = [0u8; WAL_HEADER_SIZE];
        header[0..2].copy_from_slice(&WAL_MAGIC);
        header[2] = WAL_VERSION;
        header[3] = flag;
        // header[4..8] reserved
        header[8..16].copy_from_slice(&self.current_term.to_le_bytes());
        header[16..24].copy_from_slice(&index.to_le_bytes());
        header[24..28].copy_from_slice(&payload_len.to_le_bytes());
        let crc = crc32c_append(crc32c_append(0, &header[0..28]), payload);
        header[28..32].copy_from_slice(&crc.to_le_bytes());

        let record_size = (WAL_HEADER_SIZE + payload.len()) as u64;

        if self.current_size > 0 && self.current_size + record_size > self.segment_size {
            self.rotate()?;
        }

        self.writer.write_all(&header)?;
        self.writer.write_all(payload)?;
        self.current_size += record_size;

        self.flush_and_fsync()?;
        self.enforce_limits()?;
        Ok(())
    }

    /// WAL write path（模型 B）：
    /// 1. `write()`（在 `write_record` 中已完成）
    /// 2. **`flush()` — ALWAYS**
    /// 3. 按 `FsyncPolicy` 决定是否在**本步**调用 `sync_file`，或仅攒批（`Batch`）
    ///
    /// **`flush_strategy = none`**：不调用 `sync_*`，但**仍执行**步骤 2（no sync, but still flush）。
    fn flush_and_fsync(&mut self) -> Result<()> {
        self.writer.flush()?;

        match self.fsync_policy {
            FsyncPolicy::None { .. } => Ok(()),

            FsyncPolicy::EveryWrite { sync_level } => {
                if let Err(e) = sync_file(self.writer.get_ref(), sync_level) {
                    inc_fsync_error(fsync_kind::EVERY_WRITE);
                    return Err(e.into());
                }
                Ok(())
            }

            FsyncPolicy::Batch {
                max_pending,
                max_delay_ms,
                sync_level,
            } => {
                if sync_level.is_none() {
                    return Ok(());
                }
                self.pending_sync += 1;
                let elapsed = self.last_sync.elapsed().as_millis() as u64;
                if self.pending_sync >= max_pending || elapsed >= max_delay_ms {
                    if let Err(e) = sync_file(self.writer.get_ref(), sync_level) {
                        inc_fsync_error(fsync_kind::BATCH_FLUSH);
                        return Err(e.into());
                    }
                    self.pending_sync = 0;
                    self.last_sync = std::time::Instant::now();
                }
                Ok(())
            }
        }
    }

    /// 攒批触发或 rotate / drain 时执行 `sync_file`。
    fn drain_pending_batch_sync(&mut self) -> Result<()> {
        if let FsyncPolicy::Batch { sync_level, .. } = self.fsync_policy
            && !sync_level.is_none()
            && self.pending_sync > 0
        {
            if let Err(e) = sync_file(self.writer.get_ref(), sync_level) {
                inc_fsync_error(fsync_kind::SEGMENT_ROTATE);
                return Err(e.into());
            }
            self.pending_sync = 0;
            self.last_sync = std::time::Instant::now();
        }
        Ok(())
    }

    /// 打开最新 segment 或创建新 segment。
    fn open_latest_or_create(wal_dir: &Path) -> Result<(u64, u64, File)> {
        let segments = list_segments(wal_dir)?;
        if segments.is_empty() {
            let path = wal_dir.join(format!("{WAL_SEGMENT_PREFIX}{:06}{WAL_SEGMENT_EXT}", 0));
            let file = OpenOptions::new().create(true).append(true).open(&path)?;
            let size = file.metadata()?.len();
            return Ok((0, size, file));
        }

        let last_idx = *segments.last().unwrap();
        let path = wal_dir.join(format!(
            "{WAL_SEGMENT_PREFIX}{last_idx:06}{WAL_SEGMENT_EXT}"
        ));
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let size = file.metadata()?.len();
        Ok((last_idx, size, file))
    }

    fn rotate(&mut self) -> Result<()> {
        self.writer.flush()?;
        self.drain_pending_batch_sync()?;
        self.segment_idx += 1;
        self.current_size = 0;
        let path = self.wal_dir.join(format!(
            "{WAL_SEGMENT_PREFIX}{:06}{WAL_SEGMENT_EXT}",
            self.segment_idx
        ));
        let cap = self.writer.capacity();
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        self.writer = BufWriter::with_capacity(cap, file);
        self.enforce_limits()?;
        Ok(())
    }

    /// 强制封口当前活跃 segment：所有已写入 record 落到 sealed segment，开一个新的空活跃段。
    /// 当前段为空时 no-op，避免堆积空段。
    ///
    /// 用途：`flush_one` 在 truncate 之前调用，确保 hwm 覆盖范围内的 record 不会因为
    /// 残留在活跃段而被下次 replay 重复读到。
    pub fn seal_active(&mut self) -> Result<()> {
        if self.current_size == 0 {
            return Ok(());
        }
        self.rotate()
    }

    fn enforce_limits(&mut self) -> Result<()> {
        let segments = list_segments(&self.wal_dir)?;
        evict_old_segments(
            &self.wal_dir,
            segments,
            self.max_segments,
            self.max_total_size_bytes,
            Some(self.segment_idx),
        )
    }

    /// 读取 WAL 目录中所有 segment 的记录，返回 `(records, had_truncation)`。
    pub fn read_records<P: AsRef<Path>>(wal_dir: P) -> Result<(Vec<WalRecord>, bool)> {
        let wal_dir = wal_dir.as_ref();
        if !wal_dir.exists() {
            return Ok((Vec::new(), false));
        }
        let mut all = Vec::new();
        let mut truncated = false;
        for path in Self::segment_paths_from_dir(wal_dir)? {
            let (records, seg_truncated) = read_segment_file(&path)?;
            all.extend(records);
            if seg_truncated {
                truncated = true;
            }
        }
        Ok((all, truncated))
    }

    /// 只读扫描 WAL 目录下全部 segment，**绝不截断或改写** segment 文件。
    pub fn read_records_readonly<P: AsRef<Path>>(wal_dir: P) -> Result<WalReadonlyScan> {
        let wal_dir = wal_dir.as_ref();
        let mut records = Vec::new();
        let mut errors = Vec::new();
        if !wal_dir.exists() {
            return Ok(WalReadonlyScan { records, errors });
        }
        for path in Self::segment_paths_from_dir(wal_dir)? {
            let scan = scan_segment_file_readonly(&path)?;
            records.extend(scan.records);
            if let (Some(off), Some(msg)) = (scan.tail_error_offset, scan.tail_error) {
                errors.push(WalDirScanError {
                    segment_path: path,
                    offset: off,
                    message: msg,
                });
            }
        }
        Ok(WalReadonlyScan { records, errors })
    }

    /// 返回目录下 segment 文件路径（按 segment 序号升序）。
    pub fn segment_paths_sorted<P: AsRef<Path>>(wal_dir: P) -> Result<Vec<PathBuf>> {
        let wal_dir = wal_dir.as_ref();
        Self::segment_paths_from_dir(wal_dir)
    }

    /// 读取 WAL 目录中 index > after_index 的记录。
    pub fn read_records_after<P: AsRef<Path>>(
        wal_dir: P,
        after_index: u64,
    ) -> Result<Vec<WalRecord>> {
        let (records, _) = Self::read_records(wal_dir)?;
        Ok(records
            .into_iter()
            .filter(|r| r.index > after_index)
            .collect())
    }

    /// 对 WAL 目录执行一次清理（按数量 / 总大小限制）。
    pub fn cleanup_dir<P: AsRef<Path>>(
        wal_dir: P,
        max_segments: Option<usize>,
        max_total_size_bytes: Option<u64>,
    ) -> Result<()> {
        let wal_dir = wal_dir.as_ref();
        if !wal_dir.exists() {
            return Ok(());
        }
        let segments = list_segments(wal_dir)?;
        evict_old_segments(wal_dir, segments, max_segments, max_total_size_bytes, None)
    }
}

/// 列出 WAL 目录下的所有 segment 编号（升序）。
pub(crate) fn list_segments(wal_dir: &Path) -> Result<Vec<u64>> {
    if !wal_dir.exists() {
        return Ok(Vec::new());
    }
    let mut segments = Vec::new();
    for e in std::fs::read_dir(wal_dir)? {
        let e = e?;
        let name = e.file_name();
        let s = name.to_string_lossy();
        if s.starts_with(WAL_SEGMENT_PREFIX)
            && s.ends_with(WAL_SEGMENT_EXT)
            && let Ok(num) =
                s[WAL_SEGMENT_PREFIX.len()..s.len() - WAL_SEGMENT_EXT.len()].parse::<u64>()
        {
            segments.push(num);
        }
    }
    segments.sort_unstable();
    Ok(segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_wal(dir: &Path, seg_size: usize) -> SegmentWal {
        SegmentWal::new(
            dir,
            seg_size,
            4096,
            None,
            None,
            FsyncPolicy::none_default(),
            1,
        )
        .unwrap()
    }

    #[test]
    fn append_replay_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let mut w = make_wal(tmp.path(), 1024 * 1024);
            w.write_raw(WalEntryType::Normal, b"hello", 1).unwrap();
            w.write_raw(WalEntryType::Normal, b"world", 2).unwrap();
        }
        let (records, truncated) = SegmentWal::read_records(tmp.path()).unwrap();
        assert!(!truncated);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].payload, b"hello");
        assert_eq!(records[0].index, 1);
        assert_eq!(records[1].payload, b"world");
        assert_eq!(records[1].index, 2);
    }

    #[test]
    fn rotates_when_segment_full() {
        let tmp = tempfile::tempdir().unwrap();
        {
            // segment 大小 = 一条 header + 短 payload 之后就溢出
            let mut w = make_wal(tmp.path(), 64);
            for i in 1..=4 {
                w.write_raw(WalEntryType::Normal, b"x", i).unwrap();
            }
        }
        let segs = SegmentWal::segment_paths_sorted(tmp.path()).unwrap();
        assert!(segs.len() >= 2, "expected rotation, got segments: {segs:?}");
    }

    #[test]
    fn truncates_corrupt_tail() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let mut w = make_wal(tmp.path(), 1024 * 1024);
            w.write_raw(WalEntryType::Normal, b"good", 1).unwrap();
        }
        // 写入垃圾尾部
        let seg = tmp
            .path()
            .join(format!("{WAL_SEGMENT_PREFIX}{:06}{WAL_SEGMENT_EXT}", 0));
        {
            use std::io::Write;
            let mut f = OpenOptions::new().append(true).open(&seg).unwrap();
            f.write_all(&[0xFFu8; 16]).unwrap();
            f.sync_data().unwrap();
        }
        let (records, truncated) = SegmentWal::read_records(tmp.path()).unwrap();
        assert!(truncated, "expected truncation");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].payload, b"good");
    }

    #[test]
    fn readonly_scan_does_not_truncate() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let mut w = make_wal(tmp.path(), 1024 * 1024);
            w.write_raw(WalEntryType::Normal, b"a", 1).unwrap();
        }
        let seg = tmp
            .path()
            .join(format!("{WAL_SEGMENT_PREFIX}{:06}{WAL_SEGMENT_EXT}", 0));
        let size_before = std::fs::metadata(&seg).unwrap().len();
        {
            use std::io::Write;
            let mut f = OpenOptions::new().append(true).open(&seg).unwrap();
            f.write_all(&[0xFFu8; 8]).unwrap();
            f.sync_data().unwrap();
        }
        let scan = SegmentWal::read_records_readonly(tmp.path()).unwrap();
        assert_eq!(scan.records.len(), 1);
        assert_eq!(scan.errors.len(), 1);
        // 文件不应被改写
        assert!(std::fs::metadata(&seg).unwrap().len() > size_before);
    }

    #[test]
    fn snapshot_mark_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let mut w = make_wal(tmp.path(), 1024 * 1024);
            w.write_snapshot_mark(42).unwrap();
        }
        let (records, _) = SegmentWal::read_records(tmp.path()).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].entry_type, WalEntryType::SnapshotMark);
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&records[0].payload[..8]);
        assert_eq!(u64::from_le_bytes(bytes), 42);
    }
}
