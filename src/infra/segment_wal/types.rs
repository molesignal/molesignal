// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! WAL 记录格式、类型定义、fsync 策略。

use std::{
    fs::File,
    path::{Path, PathBuf},
};

use anyhow::Result;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const WAL_MAGIC: [u8; 2] = [0xCA, 0xFE];

/// 记录布局版本号（与 [`WAL_HEADER_SIZE`] 头格式绑定）。
pub const WAL_VERSION: u8 = 2;

/// 固定头长度：`magic` … `crc32c`（含校验和槽位），后跟 `payload`。
pub const WAL_HEADER_SIZE: usize = 32;

pub const FLAG_LZ4_BIT: u8 = 0x80;

/// 持久化「深度」：控制是否调用 `sync_data` / `sync_all`（**不**替代 `BufWriter::flush`）。
///
/// ## `SyncLevel::NONE`（0）契约（WAL 路径建议原样写入代码注释）
///
/// ### MUST
/// - `write()` 已执行
/// - **`BufWriter::flush()` 已调用**；数据进入内核 page cache
///
/// ### GUARANTEE
/// - 同进程内 append-only 顺序一致性（读方须基于已 flush 的同一 fd / 视图）
///
/// ### NOT GUARANTEED
/// - crash durability；metadata consistency；跨进程可见（弱，勿依赖未 fsync 即全局可见）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SyncLevel(pub u8);

impl SyncLevel {
    pub const NONE: SyncLevel = SyncLevel(0);
    pub const DATA: SyncLevel = SyncLevel(1);
    pub const ALL: SyncLevel = SyncLevel(2);

    #[inline]
    pub fn is_none(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub fn is_data(self) -> bool {
        self.0 == 1
    }

    #[inline]
    pub fn is_all(self) -> bool {
        self.0 == 2
    }
}

impl TryFrom<u8> for SyncLevel {
    type Error = String;

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        if v <= 2 {
            Ok(SyncLevel(v))
        } else {
            Err(format!("sync_level must be 0..=2, got {v}"))
        }
    }
}

impl From<SyncLevel> for u8 {
    fn from(s: SyncLevel) -> u8 {
        s.0
    }
}

impl Serialize for SyncLevel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.0)
    }
}

impl<'de> Deserialize<'de> for SyncLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = u8::deserialize(deserializer)?;
        SyncLevel::try_from(v).map_err(serde::de::Error::custom)
    }
}

/// 按级别对打开的文件执行 `sync_*`（`NONE` 为 no-op）。
#[inline]
pub fn sync_file(file: &File, sync_level: SyncLevel) -> std::io::Result<()> {
    match sync_level.0 {
        0 => Ok(()),
        1 => file.sync_data(),
        2 => file.sync_all(),
        _ => unreachable!(),
    }
}

/// 当 `sync_level == ALL`（2）时，对 `path` 的**父目录**执行 `sync_all`，
/// 保证目录项在崩溃后可见。
///
/// Linux 上仅 `fsync` 数据文件不能保证 `rename` 后的目录项已落盘；
/// manifest / vote / snapshot 替换后须配合本函数。
pub fn sync_dir_parent_of(path: &Path, sync_level: SyncLevel) -> std::io::Result<()> {
    if sync_level.0 != 2 {
        return Ok(());
    }
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    let dir = File::open(parent)?;
    dir.sync_all()
}

/// WAL 日志配置。
pub struct SegmentWalConfig {
    pub path: PathBuf,
    pub segment_size: usize,
    pub buffer_size: usize,
    pub max_segments: Option<usize>,
    pub max_total_size_bytes: Option<u64>,
    pub fsync_policy: FsyncPolicy,
    pub initial_term: u64,
}

/// WAL 记录类型（低 7 位）。
///
/// molesignal 用法：
/// - `Normal`       — ingest 批次的二进制 payload
/// - `Config`       — stream schema / 配置变更
/// - `SnapshotMark` — parquet flush 边界标记，payload = snapshot_index (8B LE)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WalEntryType {
    Normal = 0,
    Config = 1,
    SnapshotMark = 2,
}

impl WalEntryType {
    pub fn from_flag(flag: u8) -> Result<Self> {
        match flag & !FLAG_LZ4_BIT {
            0 => Ok(Self::Normal),
            1 => Ok(Self::Config),
            2 => Ok(Self::SnapshotMark),
            other => anyhow::bail!("unknown WAL entry type: {other}"),
        }
    }
}

/// fsync 持久化策略（类型名保留）。
/// **`flush_strategy = none` 时仍必须 `BufWriter::flush()`**，仅省略 `sync_*`。
#[derive(Debug, Clone, Copy)]
pub enum FsyncPolicy {
    /// `flush_strategy = none`：每条记录后 **flush**，不调用 `sync_*`
    /// （`sync_level` 字段保留供配置对齐，此策略下不参与落盘屏障）。
    None { sync_level: SyncLevel },
    /// 每条记录 **flush** 后按 `sync_level` 调用 `sync_file`。
    EveryWrite { sync_level: SyncLevel },
    /// 每条记录 **flush**；`sync_*` 仅在攒批触发或 rotate / drain 时执行。
    Batch {
        max_pending: usize,
        max_delay_ms: u64,
        sync_level: SyncLevel,
    },
}

impl FsyncPolicy {
    /// 等价于历史上 `FsyncPolicy::None`：`none` 刷盘策略 + `SyncLevel::NONE`。
    #[inline]
    pub fn none_default() -> Self {
        Self::None {
            sync_level: SyncLevel::NONE,
        }
    }
}

/// 从 WAL 读回的单条记录。
#[derive(Debug, Clone)]
pub struct WalRecord {
    pub entry_type: WalEntryType,
    pub term: u64,
    pub index: u64,
    pub payload: Vec<u8>,
}

/// 只读扫描单个 segment（内存或 mmap）的结果，**永不写盘**。
#[derive(Debug, Clone)]
pub struct SegmentScanResult {
    pub records: Vec<WalRecord>,
    /// 解析失败处的文件内字节偏移；`None` 表示完整扫完。
    pub tail_error_offset: Option<usize>,
    pub tail_error: Option<String>,
}

/// 目录级只读扫描中某一 segment 的首个错误。
#[derive(Debug, Clone)]
pub struct WalDirScanError {
    pub segment_path: PathBuf,
    pub offset: usize,
    pub message: String,
}

/// WAL 目录只读扫描汇总（跨 segment 顺序拼接已成功解析的记录）。
#[derive(Debug, Clone)]
pub struct WalReadonlyScan {
    pub records: Vec<WalRecord>,
    pub errors: Vec<WalDirScanError>,
}

/// 供 `WalPool` 在 append 之前查询当前 raft / consensus term 的注入点。
///
/// OSS bootstrap 注入 [`StaticTermSource`]（固定值 1）；未来 raft 接入只需
/// `impl TermSource for RaftTermSource { fn current_term(&self) -> u64 { self.raft.current_term() } }`
/// 然后在 bootstrap 阶段 swap，**不改 `WalPool::new` / `SegmentWal::new` 签名**。
pub trait TermSource: Send + Sync {
    /// 返回写入下一条 WAL 记录时应使用的 term。要求实现使用原子读，免锁、轻量。
    fn current_term(&self) -> u64;
}

/// 固定 term 值实现，单机非共识场景的默认 [`TermSource`]。
#[derive(Debug, Clone, Copy)]
pub struct StaticTermSource(pub u64);

impl TermSource for StaticTermSource {
    fn current_term(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_term_source_returns_inner_value() {
        let s = StaticTermSource(7);
        assert_eq!(s.current_term(), 7);
        // Send + Sync: 应能跨 thread。
        let h = std::thread::spawn(move || s.current_term());
        assert_eq!(h.join().unwrap(), 7);
    }
}
