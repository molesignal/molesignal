// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Puffin v1 reader：通过 `object_store::ObjectStore` 按 range 读 footer / blob。
//!
//! Footer 解析最少 2 次 range read：
//! 1. `get_range(size - FOOTER_SIZE .. size)` 拿 12 字节末尾 footer，解 `payload_size`。
//! 2. `get_range(size - FOOTER_SIZE - payload_size - MAGIC_SIZE .. size - FOOTER_SIZE)`
//!    拿 inner-MAGIC + JSON payload。
//!
//! 之后每个 blob 读取走 `read_blob_bytes` → 1 次 `get_range`。

use std::sync::Arc;

use anyhow::{Result, anyhow, ensure};
use bytes::Bytes;
use object_store::{ObjectStore, ObjectStoreExt, path::Path};

use super::{
    BlobMetadata, FOOTER_PAYLOAD_SIZE_SIZE, FOOTER_SIZE, MAGIC, MAGIC_SIZE, MIN_FILE_SIZE,
    PuffinFooterFlags, PuffinMeta,
};
use crate::tantivy::metrics::{
    blob_range_reads, directory_open_seconds, directory_open_total, footer_bytes_read,
};

#[derive(Debug, Clone)]
pub struct PuffinBytesReader {
    store: Arc<dyn ObjectStore>,
    location: Path,
    size: u64,
}

impl PuffinBytesReader {
    pub fn new(store: Arc<dyn ObjectStore>, location: Path, size: u64) -> Self {
        Self {
            store,
            location,
            size,
        }
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn location(&self) -> &Path {
        &self.location
    }

    /// 拉 footer + 解析为 `PuffinMeta`；附带返回 footer payload 原始字节（含 inner MAGIC），
    /// 供 cache value 复用而不必再回源。`puffin_meta` clone 廉价（`Vec<BlobMetadata>` 小）。
    pub async fn parse_footer(&self) -> Result<(PuffinMeta, Bytes)> {
        let started = std::time::Instant::now();
        directory_open_total().inc();
        ensure!(
            self.size >= MIN_FILE_SIZE,
            "Puffin file too small: {} < {}",
            self.size,
            MIN_FILE_SIZE
        );

        // 1) tail 12 bytes
        let tail_range = (self.size - FOOTER_SIZE)..self.size;
        let tail = self
            .store
            .get_range(&self.location, tail_range.start..tail_range.end)
            .await
            .map_err(|e| anyhow!("puffin footer tail get_range: {e}"))?;
        footer_bytes_read().inc_by(tail.len() as u64);
        ensure!(
            tail.len() == FOOTER_SIZE as usize,
            "puffin footer tail short read: {}",
            tail.len()
        );
        let payload_size = u32::from_le_bytes(
            tail[0..FOOTER_PAYLOAD_SIZE_SIZE as usize]
                .try_into()
                .expect("4 bytes"),
        ) as u64;
        let flags_bytes = &tail[FOOTER_PAYLOAD_SIZE_SIZE as usize
            ..(FOOTER_PAYLOAD_SIZE_SIZE + super::FLAGS_SIZE) as usize];
        let flags_u32 = u32::from_le_bytes(flags_bytes.try_into().expect("4 bytes"));
        let flags = PuffinFooterFlags::from_bits(flags_u32)
            .ok_or_else(|| anyhow!("puffin footer flags invalid: 0x{:x}", flags_u32))?;
        // The tail must end with MAGIC.
        ensure!(
            tail[(FOOTER_PAYLOAD_SIZE_SIZE + super::FLAGS_SIZE) as usize..] == MAGIC[..],
            "puffin tail MAGIC mismatch"
        );
        let _ = flags; // currently no compression on payload supported

        // 2) inner MAGIC + JSON payload
        let payload_start = self
            .size
            .checked_sub(FOOTER_SIZE + payload_size + MAGIC_SIZE)
            .ok_or_else(|| {
                anyhow!(
                    "puffin payload range underflow: size={}, payload_size={}",
                    self.size,
                    payload_size
                )
            })?;
        let payload_range = payload_start..(self.size - FOOTER_SIZE);
        let payload_bytes = self
            .store
            .get_range(&self.location, payload_range.start..payload_range.end)
            .await
            .map_err(|e| anyhow!("puffin footer payload get_range: {e}"))?;
        footer_bytes_read().inc_by(payload_bytes.len() as u64);
        ensure!(
            payload_bytes.len() == (payload_size + MAGIC_SIZE) as usize,
            "puffin payload short read: {} expected {}",
            payload_bytes.len(),
            payload_size + MAGIC_SIZE
        );
        ensure!(
            payload_bytes[..MAGIC_SIZE as usize] == MAGIC[..],
            "puffin inner MAGIC mismatch"
        );

        let payload_json = &payload_bytes[MAGIC_SIZE as usize..];
        let meta: PuffinMeta =
            serde_json::from_slice(payload_json).map_err(|e| anyhow!("PuffinMeta json: {e}"))?;
        directory_open_seconds().observe(started.elapsed().as_secs_f64());
        Ok((meta, payload_bytes))
    }

    /// 读取一个 blob 的字节（按可选 sub-range）。每次调用 → 1 次 `get_range` →
    /// `tantivy_puffin_blob_range_reads_total += 1`。
    pub async fn read_blob_bytes(
        &self,
        meta: &BlobMetadata,
        sub: Option<core::ops::Range<u64>>,
    ) -> Result<Bytes> {
        if let Some(codec) = meta.compression_codec {
            return Err(anyhow!(
                "puffin blob compression {:?} not supported in this build",
                codec
            ));
        }
        let range = meta.absolute_range(sub);
        blob_range_reads().inc();
        let bytes = self
            .store
            .get_range(&self.location, range.start..range.end)
            .await
            .map_err(|e| anyhow!("puffin blob get_range: {e}"))?;
        Ok(bytes)
    }
}
