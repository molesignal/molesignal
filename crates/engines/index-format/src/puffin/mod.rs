// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Puffin v1 文件格式（Apache Iceberg [Puffin spec](https://iceberg.apache.org/puffin-spec/)）。
//!
//! 物理布局：
//! ```text
//! MAGIC(4) | <blob bytes>… | MAGIC(4) | PAYLOAD(JSON) | PAYLOAD_SIZE(4 LE) | FLAGS(4 LE) | MAGIC(4)
//! ```
//!
//! - `MAGIC = [0x50, 0x46, 0x41, 0x31]`（"PFA1"），共 4 字节，分别出现在文件起始、payload 之前、文件末尾。
//! - `PAYLOAD` 是 JSON-encoded [`PuffinMeta`]。
//! - footer 末尾固定 12 字节（`FOOTER_SIZE`）：`payload_size(u32 LE) | flags(u32 LE) | MAGIC`。
//!
//! Reader 的 fast path：先 `get_range(size - FOOTER_SIZE .. size)` 拿 12 字节 footer →
//! 解析 `payload_size` → `get_range(size - FOOTER_SIZE - payload_size - MAGIC_SIZE .. size - FOOTER_SIZE)`
//! 拿 payload + 起始 MAGIC（payload 前的 MAGIC 是边界确认）。

use std::collections::HashMap;

use bitflags::bitflags;
use serde::{Deserialize, Serialize};

pub mod reader;
pub mod writer;

/// PFA1 magic 字节序列。
pub const MAGIC: [u8; 4] = [0x50, 0x46, 0x41, 0x31];
pub const MAGIC_SIZE: u64 = MAGIC.len() as u64;
pub const FLAGS_SIZE: u64 = 4;
pub const FOOTER_PAYLOAD_SIZE_SIZE: u64 = 4;
/// footer 末尾固定大小：4(magic) + 4(flags) + 4(payload_size) = 12。
pub const FOOTER_SIZE: u64 = MAGIC_SIZE + FLAGS_SIZE + FOOTER_PAYLOAD_SIZE_SIZE;
/// 一个最小合法 puffin 文件至少这么大（起始 magic + 4(payload_size) + 4(flags) + 末尾 magic）。
pub const MIN_DATA_SIZE: u64 = MAGIC_SIZE + FLAGS_SIZE + FOOTER_PAYLOAD_SIZE_SIZE + MAGIC_SIZE;
pub const MIN_FILE_SIZE: u64 = MAGIC_SIZE + MIN_DATA_SIZE;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PuffinFooterFlags: u32 {
        const DEFAULT = 0b00000000;
        const COMPRESSED = 0b00000001;
    }
}

/// Puffin 文件 footer payload。`blobs` 顺序与文件中 blob 顺序一致。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PuffinMeta {
    pub blobs: Vec<BlobMetadata>,
    #[serde(default)]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub properties: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub struct BlobMetadata {
    #[serde(rename = "type")]
    pub blob_type: BlobTypes,
    #[serde(default)]
    pub fields: Vec<u32>,
    #[serde(default)]
    pub snapshot_id: u64,
    #[serde(default)]
    pub sequence_number: u64,
    /// blob 起始绝对 offset（含起始 MAGIC，即 offset ≥ 4）。
    pub offset: u64,
    /// blob 字节长度（解压前，puffin 当前不开 blob 内部压缩）。
    pub length: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compression_codec: Option<CompressionCodec>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub properties: HashMap<String, String>,
}

impl BlobMetadata {
    pub fn absolute_range(&self, sub: Option<core::ops::Range<u64>>) -> core::ops::Range<u64> {
        match sub {
            None => self.offset..(self.offset + self.length),
            Some(r) => self.offset + r.start..self.offset + r.end,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompressionCodec {
    Lz4,
    Zstd,
}

/// blob 语义类型。`TantivySegmentV1` = 单个 tantivy 段文件；`TantivyFooterV1` = 我们 bundle
/// 进 puffin 的 tantivy segment-meta cache。各 variant 的 serde tag（`ms-ttv-*`）即写入
/// puffin footer 的 blob 类型标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum BlobTypes {
    #[default]
    #[serde(rename = "ms-ttv-v1")]
    TantivySegmentV1,
    #[serde(rename = "ms-ttv-footer-v1")]
    TantivyFooterV1,
}
