// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[wal]` —— Write-Ahead Log 落盘目录、分段与 fsync 策略。

use serde::{Deserialize, Serialize};

/// fsync 触发模式（spec `ingestion / Write-Ahead Log Durability`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WalFlushStrategy {
    /// 仅 `BufWriter::flush()` 到 page cache，**永不** `sync_*`（旧版默认行为）。
    None,
    /// 每条记录都 `BufWriter::flush()` + `sync_file(sync_level)`。
    EveryWrite,
    /// `BufWriter::flush()` 每次；`sync_*` 在
    /// `batch_max_pending` 条或 `batch_max_delay_ms` 毫秒任一触发时执行。
    Batch,
}

/// `sync_file` 深度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WalSyncLevel {
    /// no-op（实际不调用 `sync_*`）。
    None,
    /// `sync_data` —— 仅 data，不强制 metadata。
    Data,
    /// `sync_all` + 父目录 `sync_all`。
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalSettings {
    #[serde(default = "default_wal_dir")]
    pub dir: String,
    #[serde(default = "default_wal_seg")]
    pub segment_size_mb: u32,
    /// fsync 触发模式；默认 `batch`。
    #[serde(default = "default_flush_strategy")]
    pub flush_strategy: WalFlushStrategy,
    /// `sync_file` 深度；默认 `data`（`sync_data`）。
    #[serde(default = "default_sync_level")]
    pub sync_level: WalSyncLevel,
    /// `Batch` 模式下攒批条数上限。
    #[serde(default = "default_batch_max_pending")]
    pub batch_max_pending: u32,
    /// `Batch` 模式下两次 `sync_*` 的最大间隔（毫秒）。
    /// 兼容 alias：旧 TOML 用 `sync_interval_ms = N` 写法仍被接受并映射到本字段。
    #[serde(default = "default_batch_max_delay_ms", alias = "sync_interval_ms")]
    pub batch_max_delay_ms: u32,
    /// WAL 落盘 at-rest 加密：true 时每条 record 的 payload 用 `CipherRootKey`（KEK）加密后
    /// 再写 segment（recover 时解密）。默认 false → 明文落盘（与现状一致）。
    #[serde(default)]
    pub encrypt: bool,
}

fn default_wal_dir() -> String {
    "./data/wal".into()
}
fn default_wal_seg() -> u32 {
    256
}
fn default_flush_strategy() -> WalFlushStrategy {
    WalFlushStrategy::Batch
}
fn default_sync_level() -> WalSyncLevel {
    WalSyncLevel::Data
}
fn default_batch_max_pending() -> u32 {
    64
}
fn default_batch_max_delay_ms() -> u32 {
    50
}

impl Default for WalSettings {
    fn default() -> Self {
        Self {
            dir: default_wal_dir(),
            segment_size_mb: default_wal_seg(),
            flush_strategy: default_flush_strategy(),
            sync_level: default_sync_level(),
            batch_max_pending: default_batch_max_pending(),
            batch_max_delay_ms: default_batch_max_delay_ms(),
            encrypt: false,
        }
    }
}
