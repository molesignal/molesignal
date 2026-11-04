// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Puffin v1 reader：通过 `object_store::ObjectStore` 按 range 读 footer / blob。
//!
//! [`PuffinBytesReader::parse_footer`] 最少 2 次 range read：
//! 1. `get_range(size - FOOTER_SIZE .. size)` 拿 12 字节末尾 footer，解 `payload_size`。
//! 2. `get_range(size - FOOTER_SIZE - payload_size - MAGIC_SIZE .. size - FOOTER_SIZE)`
//!    拿 inner-MAGIC + JSON payload。
//!
//! [`PuffinBytesReader::parse_footer_from_tail`] 一次读文件末尾一段：footer 与紧挨它的小 blob
//! 落在这段里时只要 1 次 range read。[`PuffinBytesReader::open_tail`] 不需要事先知道对象大小：
//! 后缀范围读取的响应带回对象大小，省掉一次 HEAD。
//!
//! 之后每个 blob 读取走 `read_blob_bytes` → 1 次 `get_range`。

use std::sync::Arc;

use anyhow::{Result, anyhow, ensure};
use bytes::Bytes;
use object_store::{GetOptions, GetRange, ObjectStore, ObjectStoreExt, path::Path};

use super::{
    BlobMetadata, FOOTER_PAYLOAD_SIZE_SIZE, FOOTER_SIZE, MAGIC, MAGIC_SIZE, MIN_FILE_SIZE,
    PuffinFooterFlags, PuffinMeta,
};
use crate::metrics::{
    blob_range_reads, directory_open_seconds, directory_open_total, footer_bytes_read,
};

#[derive(Debug, Clone)]
pub struct PuffinBytesReader {
    store: Arc<dyn ObjectStore>,
    location: Path,
    size: u64,
}

/// The last bytes of the file, read once with the footer.
#[derive(Debug, Clone)]
pub struct PuffinTail {
    start: u64,
    bytes: Bytes,
}

impl PuffinTail {
    /// The bytes of `range` when the tail holds all of it.
    pub fn slice(&self, range: &core::ops::Range<u64>) -> Option<Bytes> {
        if range.start < self.start || range.end > self.start + self.bytes.len() as u64 {
            return None;
        }

        Some(
            self.bytes
                .slice((range.start - self.start) as usize..(range.end - self.start) as usize),
        )
    }
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
        self.ensure_min_size()?;

        // 1) tail 12 bytes
        let tail = self
            .store
            .get_range(&self.location, (self.size - FOOTER_SIZE)..self.size)
            .await
            .map_err(|e| anyhow!("puffin footer tail get_range: {e}"))?;

        footer_bytes_read().inc_by(tail.len() as u64);

        // 2) inner MAGIC + JSON payload
        let payload_range = self.payload_range(footer_payload_size(&tail)?)?;
        let payload_bytes = self.read_payload(payload_range.clone()).await?;
        let meta = parse_payload(&payload_bytes, &payload_range)?;
        directory_open_seconds().observe(started.elapsed().as_secs_f64());
        Ok((meta, payload_bytes))
    }

    /// 一次读末尾 `hint` 字节（至少覆盖固定 footer）再从中解析 footer；payload 起点落在这段之前
    /// 时补读一次。返回的 [`PuffinTail`] 供调用方直接切出同样落在末尾的小 blob。
    pub async fn parse_footer_from_tail(
        &self,
        hint: u64,
    ) -> Result<(PuffinMeta, Bytes, PuffinTail)> {
        let started = std::time::Instant::now();
        directory_open_total().inc();
        self.ensure_min_size()?;

        let tail_start = self.size - hint.clamp(FOOTER_SIZE, self.size);

        let bytes = self
            .store
            .get_range(&self.location, tail_start..self.size)
            .await
            .map_err(|e| anyhow!("puffin tail get_range: {e}"))?;

        footer_bytes_read().inc_by(bytes.len() as u64);

        ensure!(
            bytes.len() as u64 == self.size - tail_start,
            "puffin tail short read: {}",
            bytes.len()
        );

        self.parse_tail(
            PuffinTail {
                start: tail_start,
                bytes,
            },
            started,
        )
        .await
    }

    /// 不知道对象大小时打开：一次后缀范围读取拿到末尾 `hint` 字节，响应同时带回对象大小。
    /// 对象不存在时错误链里是 [`object_store::Error::NotFound`]。
    pub async fn open_tail(
        store: Arc<dyn ObjectStore>,
        location: Path,
        hint: u64,
    ) -> Result<(Self, PuffinMeta, Bytes, PuffinTail)> {
        let started = std::time::Instant::now();
        directory_open_total().inc();

        let result = store
            .get_opts(
                &location,
                GetOptions::new().with_range(Some(GetRange::Suffix(hint.max(FOOTER_SIZE)))),
            )
            .await
            .map_err(|e| anyhow::Error::new(e).context("puffin tail get"))?;

        let size = result.meta.size;
        let start = result.range.start;

        let bytes = result
            .bytes()
            .await
            .map_err(|e| anyhow::Error::new(e).context("puffin tail read"))?;

        footer_bytes_read().inc_by(bytes.len() as u64);

        let reader = Self::new(store, location, size);
        reader.ensure_min_size()?;

        ensure!(
            start + bytes.len() as u64 == size,
            "puffin tail does not reach the object end: {start} + {} != {size}",
            bytes.len()
        );

        let (meta, payload, tail) = reader
            .parse_tail(PuffinTail { start, bytes }, started)
            .await?;

        Ok((reader, meta, payload, tail))
    }

    async fn parse_tail(
        &self,
        tail: PuffinTail,
        started: std::time::Instant,
    ) -> Result<(PuffinMeta, Bytes, PuffinTail)> {
        let footer = tail
            .slice(&((self.size - FOOTER_SIZE)..self.size))
            .ok_or_else(|| anyhow!("puffin tail misses the fixed footer"))?;

        let payload_range = self.payload_range(footer_payload_size(&footer)?)?;

        let payload_bytes = match tail.slice(&payload_range) {
            Some(bytes) => bytes,
            None => self.read_payload(payload_range.clone()).await?,
        };

        let meta = parse_payload(&payload_bytes, &payload_range)?;
        directory_open_seconds().observe(started.elapsed().as_secs_f64());
        Ok((meta, payload_bytes, tail))
    }

    fn ensure_min_size(&self) -> Result<()> {
        ensure!(
            self.size >= MIN_FILE_SIZE,
            "Puffin file too small: {} < {}",
            self.size,
            MIN_FILE_SIZE
        );

        Ok(())
    }

    /// inner MAGIC + JSON payload 的绝对范围。
    fn payload_range(&self, payload_size: u64) -> Result<core::ops::Range<u64>> {
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

        Ok(payload_start..(self.size - FOOTER_SIZE))
    }

    async fn read_payload(&self, range: core::ops::Range<u64>) -> Result<Bytes> {
        let bytes = self
            .store
            .get_range(&self.location, range)
            .await
            .map_err(|e| anyhow!("puffin footer payload get_range: {e}"))?;

        footer_bytes_read().inc_by(bytes.len() as u64);
        Ok(bytes)
    }

    /// 读一个完整 blob；落在 `tail` 里时直接切片，不回源。
    pub async fn read_blob_from_tail(
        &self,
        meta: &BlobMetadata,
        tail: &PuffinTail,
    ) -> Result<Bytes> {
        if meta.compression_codec.is_none()
            && let Some(bytes) = tail.slice(&meta.absolute_range(None))
        {
            return Ok(bytes);
        }

        self.read_blob_bytes(meta, None).await
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

/// 校验末尾固定 footer（`payload_size | flags | MAGIC`）并取出 `payload_size`。
fn footer_payload_size(footer: &[u8]) -> Result<u64> {
    ensure!(
        footer.len() == FOOTER_SIZE as usize,
        "puffin footer tail short read: {}",
        footer.len()
    );

    let payload_size = u32::from_le_bytes(
        footer[0..FOOTER_PAYLOAD_SIZE_SIZE as usize]
            .try_into()
            .expect("4 bytes"),
    ) as u64;

    let flags_bytes = &footer[FOOTER_PAYLOAD_SIZE_SIZE as usize
        ..(FOOTER_PAYLOAD_SIZE_SIZE + super::FLAGS_SIZE) as usize];
    let flags_u32 = u32::from_le_bytes(flags_bytes.try_into().expect("4 bytes"));

    // currently no compression on payload supported
    PuffinFooterFlags::from_bits(flags_u32)
        .ok_or_else(|| anyhow!("puffin footer flags invalid: 0x{:x}", flags_u32))?;

    // The tail must end with MAGIC.
    ensure!(
        footer[(FOOTER_PAYLOAD_SIZE_SIZE + super::FLAGS_SIZE) as usize..] == MAGIC[..],
        "puffin tail MAGIC mismatch"
    );

    Ok(payload_size)
}

/// 校验 inner MAGIC 后解析 JSON payload。
fn parse_payload(payload: &Bytes, range: &core::ops::Range<u64>) -> Result<PuffinMeta> {
    ensure!(
        payload.len() as u64 == range.end - range.start,
        "puffin payload short read: {} expected {}",
        payload.len(),
        range.end - range.start
    );

    ensure!(
        payload[..MAGIC_SIZE as usize] == MAGIC[..],
        "puffin inner MAGIC mismatch"
    );

    serde_json::from_slice(&payload[MAGIC_SIZE as usize..])
        .map_err(|e| anyhow!("PuffinMeta json: {e}"))
}
