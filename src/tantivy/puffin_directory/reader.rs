// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Puffin-backed read directory：通过 `ObjectStore` 把 tantivy 的每次文件读
//! 转成对单个 puffin blob 的 sub-range `get_range`。

use std::{
    io,
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use bytes::Bytes;
use hashbrown::HashMap;
use object_store::ObjectStore;
use tantivy::{
    HasLen,
    directory::{Directory, FileHandle, OwnedBytes, error::OpenReadError},
};

use super::{
    FOOTER_CACHE_BLOB_TAG, SYNC_ATOMIC_READ_TARGETS, TantivyFooter,
    empty_directory::get_empty_file_bytes,
};
use crate::tantivy::puffin::{BlobMetadata, reader::PuffinBytesReader};

/// 用 `PuffinBytesReader` 提供 IO，把 `BlobMetadata` 按 tantivy 文件路径建立映射。
#[derive(Debug, Clone)]
pub struct PuffinDirReader {
    source: Arc<PuffinBytesReader>,
    blobs: Arc<HashMap<PathBuf, Arc<BlobMetadata>>>,
    /// 预物化的 sync `atomic_read` 目标字节（典型为 `meta.json`）。
    /// 构造 reader 时一次性 async 拉取，sync `Directory::atomic_read` 直接 map 查询；
    /// miss 返 `FileDoesNotExist`（已被 17 个 tantivy 测试 + IT 验证覆盖到位）。
    atomic_files: Arc<HashMap<PathBuf, Bytes>>,
}

impl PuffinDirReader {
    /// 从 object_store 读 footer 构造 reader。一次 `parse_footer` →
    /// 至少 2 次 range get + 1 次 puffin_meta 解析 + 每个 sync atomic_read 目标
    /// （目前仅 `meta.json`）1 次 range get 预物化。
    pub async fn from_object_store(
        store: Arc<dyn ObjectStore>,
        location: object_store::path::Path,
        size: u64,
    ) -> Result<Self> {
        let source = PuffinBytesReader::new(store, location, size);
        let (meta, _footer_payload) = source.parse_footer().await?;
        let (blobs, atomic_files) = build_maps(&meta, &source).await?;
        Ok(Self {
            source: Arc::new(source),
            blobs: Arc::new(blobs),
            atomic_files: Arc::new(atomic_files),
        })
    }

    /// 用已 cache 的 [`TantivyFooter`]（含 puffin_meta + 预物化 atomic_files）构造
    /// reader，**零 IO**。Footer cache 命中路径专用。
    pub fn from_cached_footer(
        store: Arc<dyn ObjectStore>,
        location: object_store::path::Path,
        size: u64,
        footer: &TantivyFooter,
    ) -> Self {
        let source = PuffinBytesReader::new(store, location, size);
        let mut blobs = HashMap::new();
        for blob in &footer.puffin_meta.blobs {
            let Some(tag) = blob.properties.get("blob_tag") else {
                continue;
            };
            if tag == FOOTER_CACHE_BLOB_TAG {
                continue;
            }
            blobs.insert(PathBuf::from(tag), Arc::new(blob.clone()));
        }
        Self {
            source: Arc::new(source),
            blobs: Arc::new(blobs),
            atomic_files: footer.atomic_files.clone(),
        }
    }
}

/// 共享给 caller 用：caller 把这个 atomic_files map 一起塞进 `TantivyFooter` 入 cache。
pub async fn build_atomic_files(
    meta: &crate::tantivy::puffin::PuffinMeta,
    source: &PuffinBytesReader,
) -> Result<HashMap<PathBuf, Bytes>> {
    let mut out = HashMap::new();
    for blob in &meta.blobs {
        let Some(tag) = blob.properties.get("blob_tag") else {
            continue;
        };
        if !SYNC_ATOMIC_READ_TARGETS.contains(&tag.as_str()) {
            continue;
        }
        let bytes = source.read_blob_bytes(blob, None).await?;
        out.insert(PathBuf::from(tag), bytes);
    }
    Ok(out)
}

async fn build_maps(
    meta: &crate::tantivy::puffin::PuffinMeta,
    source: &PuffinBytesReader,
) -> Result<(HashMap<PathBuf, Arc<BlobMetadata>>, HashMap<PathBuf, Bytes>)> {
    let mut blobs = HashMap::new();
    for blob in &meta.blobs {
        let Some(tag) = blob.properties.get("blob_tag") else {
            continue;
        };
        if tag == FOOTER_CACHE_BLOB_TAG {
            continue;
        }
        blobs.insert(PathBuf::from(tag), Arc::new(blob.clone()));
    }
    let atomic_files = build_atomic_files(meta, source).await?;
    Ok((blobs, atomic_files))
}

/// Puffin blob 的 tantivy FileHandle 适配。
///
/// **设计**：tantivy 0.25 在 search 路径上走 **sync** `read_bytes`，所以我们必须
/// 把单个 blob 的字节一次性物化到内存（首次 sync read 触发）。之后所有 read 都
/// 切内存。代价：每个被 tantivy 触及的 segment 文件下载一次（仍优于整 archive
/// 下载），收益：tantivy 的 N 次小 read 全部走内存。
///
/// 物化路径：在隔离的 `std::thread` 里建一个 current-thread tokio runtime 跑
/// `source.read_blob_bytes`，避免在主 runtime 上 sync-block。
#[derive(Debug)]
struct PuffinSliceHandle {
    source: Arc<PuffinBytesReader>,
    meta: Arc<BlobMetadata>,
    #[allow(dead_code)] // 仅用于 Debug 输出，方便排查；逻辑上不参与读
    path: PathBuf,
    /// 整 blob 的内存缓存；首次 sync `read_bytes` / `read_bytes_async` 触发物化。
    materialized: std::sync::OnceLock<Bytes>,
}

impl HasLen for PuffinSliceHandle {
    fn len(&self) -> usize {
        self.meta.length as usize
    }
}

impl PuffinSliceHandle {
    fn ensure_materialized_blocking(&self) -> io::Result<&Bytes> {
        if let Some(b) = self.materialized.get() {
            return Ok(b);
        }
        let source = self.source.clone();
        let meta = self.meta.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(Err(io::Error::other(format!("rt build: {e}"))));
                    return;
                }
            };
            let res = rt.block_on(async move {
                source
                    .read_blob_bytes(&meta, None)
                    .await
                    .map_err(|e| io::Error::other(format!("puffin materialize: {e}")))
            });
            let _ = tx.send(res);
        });
        let bytes = rx
            .recv()
            .map_err(|e| io::Error::other(format!("materialize channel: {e}")))??;
        // OnceLock::set 可能因别人先 set 而 Err（rare race in tantivy reads），不致命。
        let _ = self.materialized.set(bytes);
        Ok(self.materialized.get().expect("just set"))
    }

    fn slice_range(&self, byte_range: Range<usize>) -> OwnedBytes {
        let bytes = self.materialized.get().expect("materialized must exist");
        let end = byte_range.end.min(bytes.len());
        let start = byte_range.start.min(end);
        OwnedBytes::new(bytes.slice(start..end).to_vec())
    }
}

#[async_trait::async_trait]
impl FileHandle for PuffinSliceHandle {
    fn read_bytes(&self, byte_range: Range<usize>) -> io::Result<OwnedBytes> {
        if byte_range.is_empty() {
            return Ok(OwnedBytes::empty());
        }
        self.ensure_materialized_blocking()?;
        Ok(self.slice_range(byte_range))
    }

    async fn read_bytes_async(&self, byte_range: Range<usize>) -> io::Result<OwnedBytes> {
        if byte_range.is_empty() {
            return Ok(OwnedBytes::empty());
        }
        // async 路径：直接 sub-range，不走 materialize；commiserate with tantivy
        // 当前 async API（如果未来 tantivy 全 async，这条更省内存）。
        let sub = Range {
            start: byte_range.start as u64,
            end: byte_range.end as u64,
        };
        let bytes = self
            .source
            .read_blob_bytes(&self.meta, Some(sub))
            .await
            .map_err(|e| io::Error::other(format!("puffin slice read: {e}")))?;
        Ok(OwnedBytes::new(bytes.to_vec()))
    }
}

#[derive(Debug, Clone)]
struct StaticBytesHandle {
    bytes: Bytes,
}

impl HasLen for StaticBytesHandle {
    fn len(&self) -> usize {
        self.bytes.len()
    }
}

#[async_trait::async_trait]
impl FileHandle for StaticBytesHandle {
    fn read_bytes(&self, byte_range: Range<usize>) -> io::Result<OwnedBytes> {
        let slice = self
            .bytes
            .slice(byte_range.start..byte_range.end.min(self.bytes.len()));
        Ok(OwnedBytes::new(slice.to_vec()))
    }
}

impl Directory for PuffinDirReader {
    fn get_file_handle(
        &self,
        path: &Path,
    ) -> std::result::Result<Arc<dyn FileHandle>, OpenReadError> {
        if let Some(meta) = self.blobs.get(path) {
            return Ok(Arc::new(PuffinSliceHandle {
                source: self.source.clone(),
                meta: meta.clone(),
                path: path.to_path_buf(),
                materialized: std::sync::OnceLock::new(),
            }));
        }
        // Fall back to bundled empty puffin dir for optional segment files.
        if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && let Some(bytes) = get_empty_file_bytes(ext)
        {
            return Ok(Arc::new(StaticBytesHandle { bytes }));
        }
        Err(OpenReadError::FileDoesNotExist(path.to_path_buf()))
    }

    fn delete(&self, _path: &Path) -> Result<(), tantivy::directory::error::DeleteError> {
        unimplemented!("read-only puffin directory")
    }

    fn exists(&self, path: &Path) -> std::result::Result<bool, OpenReadError> {
        if self.blobs.contains_key(path) {
            return Ok(true);
        }
        Ok(path
            .extension()
            .and_then(|e| e.to_str())
            .map(|ext| get_empty_file_bytes(ext).is_some())
            .unwrap_or(false))
    }

    fn open_write(
        &self,
        _path: &Path,
    ) -> Result<tantivy::directory::WritePtr, tantivy::directory::error::OpenWriteError> {
        unimplemented!("read-only puffin directory")
    }

    fn atomic_read(&self, path: &Path) -> std::result::Result<Vec<u8>, OpenReadError> {
        // 预物化路径：构造 reader 时已把 `meta.json` 等 SYNC_ATOMIC_READ_TARGETS
        // 拉到 `atomic_files`；这里直接 map 查询，零 IO 零线程。
        if let Some(bytes) = self.atomic_files.get(path) {
            return Ok(bytes.to_vec());
        }
        // 兜底：空 puffin directory 的对应扩展名（与 get_file_handle 路径对称）。
        if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && let Some(bytes) = get_empty_file_bytes(ext)
        {
            return Ok(bytes.to_vec());
        }
        Err(OpenReadError::FileDoesNotExist(path.to_path_buf()))
    }

    fn atomic_write(&self, _path: &Path, _data: &[u8]) -> io::Result<()> {
        unimplemented!("read-only puffin directory")
    }

    fn sync_directory(&self) -> io::Result<()> {
        unimplemented!("read-only puffin directory")
    }

    fn watch(
        &self,
        _cb: tantivy::directory::WatchCallback,
    ) -> tantivy::Result<tantivy::directory::WatchHandle> {
        Ok(tantivy::directory::WatchHandle::empty())
    }

    fn acquire_lock(
        &self,
        _lock: &tantivy::directory::Lock,
    ) -> std::result::Result<tantivy::directory::DirectoryLock, tantivy::directory::error::LockError>
    {
        // read-only 不真持锁，返回 noop 包装。tantivy::Directory 内部对 DirectoryLock
        // 仅用 Drop 释放，所以传一个空 Box 即可。
        Ok(tantivy::directory::DirectoryLock::from(Box::new(())))
    }
}
