// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Segment WAL — 按 segment 分片的预写日志。
//!
//! 通用的 segment 文件管理 + 记录编码 + mmap 读取 + 尾部截断容错。
//! molesignal 用法：
//! - `Normal`       — ingest 批次的二进制 payload
//! - `Config`       — stream schema / 配置变更
//! - `SnapshotMark` — parquet flush 边界标记
//!
//! ## 记录格式（32 字节头 + payload，无尾 CRC）
//!
//! ```text
//! | magic(2B) | version(1B) | flag(1B) | reserved(4B) |
//! | term(8B)    | index(8B)   | payload_len(4B) | crc32c(4B) |
//! | payload(N) |
//! ```
//!
//! - magic: `0xCA 0xFE`
//! - version: 见 [`WAL_VERSION`]
//! - flag: 记录类型（低 7 位）+ `FLAG_LZ4_BIT`（bit7 = payload 为 lz4 压缩）
//! - reserved: 填 0，预留对齐 / 扩展
//! - CRC32C（Castagnoli）覆盖 **头的前 28 字节** + **payload**（与头内 crc 槽位无关）

mod cleanup;
mod metrics;
mod reader;
mod types;
mod writer;

pub use reader::{scan_segment_bytes, scan_segment_file_readonly, scan_segment_max_index};
pub use types::{
    FLAG_LZ4_BIT, FsyncPolicy, SegmentScanResult, SegmentWalConfig, StaticTermSource, SyncLevel,
    TermSource, WAL_HEADER_SIZE, WAL_MAGIC, WAL_VERSION, WalDirScanError, WalEntryType,
    WalReadonlyScan, WalRecord, sync_dir_parent_of, sync_file,
};
pub use writer::SegmentWal;

/// segment 文件命名前缀。
pub const WAL_SEGMENT_PREFIX: &str = "wal-";
/// segment 文件扩展名。
pub const WAL_SEGMENT_EXT: &str = ".seg";
