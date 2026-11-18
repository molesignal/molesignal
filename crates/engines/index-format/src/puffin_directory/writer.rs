// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Puffin-backed write directory：内部 `MmapDirectory` (tempdir) 给 tantivy 用，
//! 序列化时把提交的索引用到的每个文件作为一个 Puffin blob 追加。

use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use anyhow::{Result, anyhow};
use hashbrown::HashMap;
use tantivy::{
    Index,
    directory::{Directory, MmapDirectory, WatchCallback, WatchHandle, error::OpenReadError},
    index::SegmentComponent,
};

use super::{FOOTER_CACHE_BLOB_TAG, MANAGED_JSON, META_JSON};
use crate::puffin::{BlobTypes, writer::PuffinBytesWriter};

#[derive(Debug)]
pub struct PuffinDirWriter {
    mmap: Arc<MmapDirectory>,
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

    /// 序列化 puffin 文件 bytes：归档里是 `meta.json` 提交的索引，只有它的各段用到的文件。
    /// 写入顺序：先是 tantivy 段文件，再是 `.managed.json` 与 `meta.json`，最后是
    /// [`BlobTypes::TantivyFooterV1`] 的 segment-open [`hot_cache`](super::hot_cache)。读端打开
    /// 归档时需要的小 blob 都紧挨 footer，一次末尾 range read 就能连同 footer 一起拿到。
    ///
    /// 写这个目录的 `IndexWriter` 要先结束：还在进行的段合并会改写 `meta.json`、删掉被合并的段。
    pub fn to_puffin_bytes(&self) -> Result<Vec<u8>> {
        let mut out: Vec<u8> = Vec::new();
        self.write_puffin(&mut out)?;
        Ok(out)
    }

    /// 同 [`Self::to_puffin_bytes`]，但写进 `out`（例如本地文件），返回写出的字节数。
    pub fn write_puffin<W: io::Write>(&self, out: W) -> Result<u64> {
        let files = committed_files(&self.mmap)?;
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

            for path in files {
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

/// 提交的索引用到的文件，按写进归档的顺序：各段的文件按文件名排，然后是读端打开归档就要的
/// `.managed.json` 与 `meta.json`。段合并后删掉的文件不在其中；提交里的段缺了文件，读它时报错。
fn committed_files(files: &MmapDirectory) -> Result<Vec<PathBuf>> {
    let index = Index::open(files.clone()).map_err(|e| anyhow!("puffin open index: {e}"))?;

    let segments = index
        .searchable_segment_metas()
        .map_err(|e| anyhow!("puffin committed segments: {e}"))?;

    let mut paths: Vec<PathBuf> = segments
        .iter()
        .flat_map(|segment| {
            SegmentComponent::iterator()
                .filter(|component| {
                    **component != SegmentComponent::Delete || segment.has_deletes()
                })
                .map(|component| segment.relative_path(*component))
        })
        .collect();

    paths.sort();

    // Tantivy 写 `meta.json` 时把它记进 `.managed.json`，有前者就有后者。
    paths.extend([PathBuf::from(MANAGED_JSON), PathBuf::from(META_JSON)]);
    Ok(paths)
}

impl Clone for PuffinDirWriter {
    fn clone(&self) -> Self {
        Self {
            mmap: self.mmap.clone(),
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
        self.mmap.open_write(path)
    }

    fn atomic_read(&self, path: &Path) -> std::result::Result<Vec<u8>, OpenReadError> {
        self.mmap.atomic_read(path)
    }

    fn atomic_write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use object_store::{ObjectStore, ObjectStoreExt, memory::InMemory, path::Path as ObjectPath};
    use tantivy::{
        Index, IndexWriter, ReloadPolicy, TantivyDocument, Term,
        merge_policy::NoMergePolicy,
        query::{Query, TermQuery},
        schema::{FAST, IndexRecordOption, STRING, Schema},
    };

    use super::*;
    use crate::puffin_directory::PuffinDirReader;

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

    /// 三次提交各写一段 100 个文档（每 4 个有 1 个 `error`，`at` 依次是 0 到 299），再把三段
    /// 合并成一段：合并结束时的垃圾回收经这个目录删掉三段的文件。返回合并出的段的文件。
    fn merged_index() -> (PuffinDirWriter, Vec<PathBuf>) {
        let dir = PuffinDirWriter::new().unwrap();
        let mut schema = Schema::builder();
        let level = schema.add_text_field("level", STRING);
        let at = schema.add_i64_field("at", FAST);

        let index = Index::create(
            dir.clone(),
            schema.build(),
            tantivy::IndexSettings::default(),
        )
        .unwrap();

        let mut writer: IndexWriter = index.writer_with_num_threads(1, 15_000_000).unwrap();
        writer.set_merge_policy(Box::new(NoMergePolicy));

        for segment in 0..3 {
            for row in 0..100 {
                let mut doc = TantivyDocument::default();
                doc.add_text(level, if row % 4 == 0 { "error" } else { "info" });
                doc.add_i64(at, segment * 100 + row);
                writer.add_document(doc).unwrap();
            }

            writer.commit().unwrap();
        }

        let segments = index.searchable_segment_ids().unwrap();
        assert_eq!(segments.len(), 3);

        let merged = writer.merge(&segments).wait().unwrap().unwrap();
        writer.wait_merging_threads().unwrap();

        let files = SegmentComponent::iterator()
            .filter(|component| **component != SegmentComponent::Delete)
            .map(|component| merged.relative_path(*component))
            .collect();

        (dir, files)
    }

    /// 归档里每个 blob 的名字，`level = error` 的文档数，各文档 `at` 的值（升序）。
    fn read_back(reader: PuffinDirReader) -> (BTreeSet<String>, usize, Vec<i64>) {
        let index = Index::open(reader.clone()).unwrap();
        let schema = index.schema();

        let names = reader
            .footer(schema.clone())
            .puffin_meta
            .blobs
            .iter()
            .filter_map(|blob| blob.properties.get("blob_tag").cloned())
            .collect();

        let searcher = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .unwrap()
            .searcher();

        let level = schema.get_field("level").unwrap();

        let errors = TermQuery::new(
            Term::from_field_text(level, "error"),
            IndexRecordOption::Basic,
        )
        .count(&searcher)
        .unwrap();

        let mut times: Vec<i64> = searcher
            .segment_readers()
            .iter()
            .flat_map(|segment| {
                let column = segment.fast_fields().i64("at").unwrap();
                (0..segment.max_doc()).filter_map(move |doc| column.first(doc))
            })
            .collect();

        times.sort_unstable();
        (names, errors, times)
    }

    /// 合并删掉的文件不进归档；重开归档，文档与快字段都还在。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_merged_index_archives_only_the_merged_segment() {
        let (dir, merged) = merged_index();
        let bytes = dir.to_puffin_bytes().unwrap();
        let size = bytes.len() as u64;
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let location = ObjectPath::from("merged.ttv");
        store.put(&location, bytes.into()).await.unwrap();

        let reader = PuffinDirReader::from_object_store(store, location, size)
            .await
            .unwrap();

        let (names, errors, times) = tokio::task::spawn_blocking(move || read_back(reader))
            .await
            .unwrap();

        let expected = merged
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .chain([MANAGED_JSON, META_JSON, FOOTER_CACHE_BLOB_TAG].map(String::from))
            .collect::<BTreeSet<_>>();

        assert_eq!(names, expected);
        assert_eq!(errors, 75);
        assert_eq!(times, (0..300).collect::<Vec<_>>());
    }

    /// 提交里的段缺了文件时，归档报错，而不是少一个文件。
    #[test]
    fn a_committed_file_that_is_gone_fails_the_archive() {
        let (dir, merged) = merged_index();
        let fast = merged
            .iter()
            .find(|path| path.extension().is_some_and(|ext| ext == "fast"))
            .unwrap();

        dir.delete(fast).unwrap();

        let error = format!("{:#}", dir.to_puffin_bytes().unwrap_err());
        assert!(error.contains(&*fast.to_string_lossy()), "{error}");
    }
}
