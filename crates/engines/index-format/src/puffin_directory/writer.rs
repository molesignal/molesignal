// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Puffin-backed write directory：内部 `MmapDirectory` (tempdir) 给 tantivy 用，
//! 序列化时把每个产出的文件作为一个 Puffin blob 追加。

use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use anyhow::{Result, anyhow};
use hashbrown::{HashMap, HashSet};
use tantivy::{
    HasLen,
    directory::{Directory, MmapDirectory, WatchCallback, WatchHandle, error::OpenReadError},
};

use super::{ALLOWED_FILE_EXT, FOOTER_CACHE_BLOB_TAG, META_JSON};
use crate::puffin::{BlobTypes, writer::PuffinBytesWriter};

#[derive(Debug)]
pub struct PuffinDirWriter {
    mmap: Arc<MmapDirectory>,
    /// 捕获 tantivy 通过 `open_write` / `atomic_write` 写过的路径，
    /// `to_puffin_bytes` 只对这些路径序列化。
    file_paths: Arc<RwLock<HashSet<PathBuf>>>,
    properties: Arc<RwLock<HashMap<String, String>>>,
    _tempdir: Arc<tempfile::TempDir>,
}

impl PuffinDirWriter {
    pub fn new() -> Result<Self> {
        let tempdir = tempfile::tempdir().map_err(|e| anyhow!("puffin writer tempdir: {e}"))?;
        let mmap =
            MmapDirectory::open(tempdir.path()).map_err(|e| anyhow!("puffin writer mmap: {e}"))?;

        Ok(Self {
            mmap: Arc::new(mmap),
            file_paths: Arc::new(RwLock::new(HashSet::default())),
            properties: Arc::new(RwLock::new(HashMap::default())),
            _tempdir: Arc::new(tempdir),
        })
    }

    pub fn set_property(&self, key: impl Into<String>, value: impl Into<String>) {
        self.properties
            .write()
            .expect("poisoned")
            .insert(key.into(), value.into());
    }

    /// 序列化 puffin 文件 bytes。
    /// 写入顺序：先是 tantivy 段文件，再是 `.managed.json` 与 `meta.json`，最后是
    /// [`BlobTypes::TantivyFooterV1`] 的 segment-open [`hot_cache`](super::hot_cache)。读端打开
    /// 归档时需要的小 blob 都紧挨 footer，一次末尾 range read 就能连同 footer 一起拿到。
    pub fn to_puffin_bytes(&self) -> Result<Vec<u8>> {
        let mut out: Vec<u8> = Vec::new();
        self.write_puffin(&mut out)?;
        Ok(out)
    }

    /// 同 [`Self::to_puffin_bytes`]，但写进 `out`（例如本地文件），返回写出的字节数。
    pub fn write_puffin<W: io::Write>(&self, out: W) -> Result<u64> {
        let hot_cache = super::hot_cache::build(self.mmap.clone())?.encode();
        let mut counted = CountingWriter {
            inner: out,
            written: 0,
        };

        {
            let mut writer = PuffinBytesWriter::new(&mut counted);

            for (k, v) in self.properties.read().expect("poisoned").iter() {
                writer.set_property(k.clone(), v.clone());
            }

            let paths = self.file_paths.read().expect("poisoned");
            // sort for deterministic output (tests + diffability)
            let mut sorted: Vec<PathBuf> = paths
                .iter()
                .filter(|path| is_allowed(path))
                .cloned()
                .collect();
            sorted.sort_by_key(|path| (is_metadata(path), path.clone()));

            for path in sorted {
                let file_data = self
                    .mmap
                    .open_read(&path)
                    .map_err(|e| anyhow!("puffin open_read {path:?}: {e}"))?;

                let bytes = file_data
                    .read_bytes()
                    .map_err(|e| anyhow!("puffin read_bytes {path:?}: {e}"))?;

                let tag = path.to_string_lossy().into_owned();
                writer.add_blob(&bytes, BlobTypes::TantivySegmentV1, tag)?;
            }

            writer.add_blob(
                &hot_cache,
                BlobTypes::TantivyFooterV1,
                FOOTER_CACHE_BLOB_TAG.to_string(),
            )?;

            writer.finish()?;
        }

        io::Write::flush(&mut counted.inner).map_err(|e| anyhow!("puffin flush: {e}"))?;

        Ok(counted.written)
    }
}

struct CountingWriter<W> {
    inner: W,
    written: u64,
}

impl<W: io::Write> io::Write for CountingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.written += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn is_allowed(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str())
        && ALLOWED_FILE_EXT.contains(&ext)
    {
        return true;
    }

    path.to_str() == Some(META_JSON)
        || path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == META_JSON)
            .unwrap_or(false)
}

/// `.managed.json` 与 `meta.json`：读端打开归档就要的小文件，排在段文件之后。
fn is_metadata(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("json")
}

impl Clone for PuffinDirWriter {
    fn clone(&self) -> Self {
        Self {
            mmap: self.mmap.clone(),
            file_paths: self.file_paths.clone(),
            properties: self.properties.clone(),
            _tempdir: self._tempdir.clone(),
        }
    }
}

impl Directory for PuffinDirWriter {
    fn get_file_handle(
        &self,
        path: &Path,
    ) -> std::result::Result<Arc<dyn tantivy::directory::FileHandle>, OpenReadError> {
        self.mmap.get_file_handle(path)
    }

    fn delete(&self, path: &Path) -> Result<(), tantivy::directory::error::DeleteError> {
        self.mmap.delete(path)
    }

    fn exists(&self, path: &Path) -> std::result::Result<bool, OpenReadError> {
        self.mmap.exists(path)
    }

    fn open_write(
        &self,
        path: &Path,
    ) -> Result<tantivy::directory::WritePtr, tantivy::directory::error::OpenWriteError> {
        self.file_paths
            .write()
            .expect("poisoned")
            .insert(path.to_path_buf());

        self.mmap.open_write(path)
    }

    fn atomic_read(&self, path: &Path) -> std::result::Result<Vec<u8>, OpenReadError> {
        self.mmap.atomic_read(path)
    }

    fn atomic_write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        self.file_paths
            .write()
            .expect("poisoned")
            .insert(path.to_path_buf());

        self.mmap.atomic_write(path, data)
    }

    fn sync_directory(&self) -> io::Result<()> {
        self.mmap.sync_directory()
    }

    fn watch(&self, cb: WatchCallback) -> tantivy::Result<WatchHandle> {
        self.mmap.watch(cb)
    }

    fn acquire_lock(
        &self,
        lock: &tantivy::directory::Lock,
    ) -> std::result::Result<tantivy::directory::DirectoryLock, tantivy::directory::error::LockError>
    {
        self.mmap.acquire_lock(lock)
    }
}

impl HasLen for PuffinDirWriter {
    fn len(&self) -> usize {
        self.file_paths.read().expect("poisoned").len()
    }
}

#[cfg(test)]
mod tests {
    use tantivy::{
        Index,
        schema::{STRING, Schema},
    };

    use super::*;

    #[test]
    fn build_index_and_serialize_to_puffin() {
        let dir = PuffinDirWriter::new().unwrap();
        let mut sb = Schema::builder();
        sb.add_text_field("msg", STRING);
        let schema = sb.build();
        let index = Index::create(dir.clone(), schema, tantivy::IndexSettings::default()).unwrap();
        let mut writer: tantivy::IndexWriter = index.writer(50_000_000).unwrap();
        writer.commit().unwrap();
        drop(writer);
        drop(index);
        let bytes = dir.to_puffin_bytes().unwrap();
        assert!(bytes.len() > 16);
        assert_eq!(&bytes[..4], &crate::puffin::MAGIC[..]);
        assert_eq!(&bytes[bytes.len() - 4..], &crate::puffin::MAGIC[..]);
    }

    #[test]
    fn is_allowed_filters_non_tantivy_files() {
        assert!(is_allowed(Path::new("foo.term")));
        assert!(is_allowed(Path::new("a/b/meta.json")));
        assert!(is_allowed(Path::new("meta.json")));
        assert!(!is_allowed(Path::new("foo.tmp")));
        assert!(!is_allowed(Path::new("noise")));
    }
}
