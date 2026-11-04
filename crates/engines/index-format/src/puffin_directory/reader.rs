// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Puffin-backed read directory：tantivy 的每次文件读先查归档自带的 [`HotCache`]，未命中才
//! 对单个 puffin blob 发 sub-range `get_range`；不会为一次小读下载整个 blob。

use std::{
    io,
    ops::Range,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use anyhow::Result;
use bytes::Bytes;
use hashbrown::HashMap;
use object_store::ObjectStore;
use tantivy::{
    HasLen,
    directory::{Directory, FileHandle, OwnedBytes, error::OpenReadError},
    schema::Schema,
};

use super::{
    FOOTER_CACHE_BLOB_TAG, HotCache, SYNC_ATOMIC_READ_TARGETS, TantivyFooter,
    empty_directory::get_empty_file_bytes,
};
use crate::puffin::{
    BlobMetadata, PuffinMeta,
    reader::{PuffinBytesReader, PuffinTail},
};

/// 打开归档时一次读的末尾字节数：新写的归档把 `.managed.json`、`meta.json` 与 hot cache（至多
/// 32 KiB）放在 footer 前，整段落在这里，打开只要 1 次 range read。
const TAIL_HINT_BYTES: u64 = 64 * 1024;

/// 用 `PuffinBytesReader` 提供 IO，把 `BlobMetadata` 按 tantivy 文件路径建立映射。
#[derive(Debug, Clone)]
pub struct PuffinDirReader {
    source: Arc<PuffinBytesReader>,
    blobs: Arc<HashMap<PathBuf, Arc<BlobMetadata>>>,
    /// 预物化的 sync `atomic_read` 目标字节（`meta.json` 与 `.managed.json`）。
    /// 构造 reader 时一次性 async 拉取，sync `Directory::atomic_read` 直接 map 查询；
    /// miss 返 `FileDoesNotExist`（由 Tantivy round-trip tests + IT 覆盖）。
    atomic_files: Arc<HashMap<PathBuf, Bytes>>,
    hot_cache: Arc<HotCache>,
    puffin_meta: Arc<PuffinMeta>,
    footer_payload: Bytes,
    /// 整个读进内存的 blob，见 [`Self::with_resident`]。
    resident: Arc<HashMap<PathBuf, Bytes>>,
}

impl PuffinDirReader {
    /// 从 object_store 读 footer 构造 reader：一次末尾 range read 拿 footer 与紧挨它的
    /// `meta.json`、`.managed.json`、hot cache；不在末尾那段里的（旧归档）各补 1 次 range read。
    pub async fn from_object_store(
        store: Arc<dyn ObjectStore>,
        location: object_store::path::Path,
        size: u64,
    ) -> Result<Self> {
        let source = PuffinBytesReader::new(store, location, size);
        let (meta, footer_payload, tail) = source.parse_footer_from_tail(TAIL_HINT_BYTES).await?;
        Self::from_parsed(source, meta, footer_payload, tail).await
    }

    /// 同 [`Self::from_object_store`]，但不需要事先知道对象大小：后缀范围读取的响应带回大小，
    /// 不发 HEAD。对象不存在时错误链里是 [`object_store::Error::NotFound`]。
    pub async fn from_object_store_tail(
        store: Arc<dyn ObjectStore>,
        location: object_store::path::Path,
    ) -> Result<Self> {
        let (source, meta, footer_payload, tail) =
            PuffinBytesReader::open_tail(store, location, TAIL_HINT_BYTES).await?;

        Self::from_parsed(source, meta, footer_payload, tail).await
    }

    async fn from_parsed(
        source: PuffinBytesReader,
        meta: PuffinMeta,
        footer_payload: Bytes,
        tail: PuffinTail,
    ) -> Result<Self> {
        let mut atomic_files = HashMap::new();
        let mut hot_cache = HotCache::default();

        for blob in &meta.blobs {
            let Some(tag) = blob.properties.get("blob_tag") else {
                continue;
            };

            if tag == FOOTER_CACHE_BLOB_TAG {
                hot_cache = HotCache::decode(&source.read_blob_from_tail(blob, &tail).await?);
            } else if SYNC_ATOMIC_READ_TARGETS.contains(&tag.as_str()) {
                let bytes = source.read_blob_from_tail(blob, &tail).await?;
                atomic_files.insert(PathBuf::from(tag), bytes);
            }
        }

        Ok(Self {
            blobs: Arc::new(blob_map(&meta)),
            source: Arc::new(source),
            atomic_files: Arc::new(atomic_files),
            hot_cache: Arc::new(hot_cache),
            puffin_meta: Arc::new(meta),
            footer_payload,
            resident: Arc::default(),
        })
    }

    /// 用已 cache 的 [`TantivyFooter`]（含 puffin_meta + 预物化 atomic_files + hot cache）构造
    /// reader，**零 IO**。Footer cache 命中路径专用。
    pub fn from_cached_footer(
        store: Arc<dyn ObjectStore>,
        location: object_store::path::Path,
        size: u64,
        footer: &TantivyFooter,
    ) -> Self {
        Self {
            source: Arc::new(PuffinBytesReader::new(store, location, size)),
            blobs: Arc::new(blob_map(&footer.puffin_meta)),
            atomic_files: footer.atomic_files.clone(),
            hot_cache: footer.hot_cache.clone(),
            puffin_meta: footer.puffin_meta.clone(),
            footer_payload: footer.footer_payload_bytes.clone(),
            resident: Arc::default(),
        }
    }

    /// 同一个归档的 reader，`paths` 指的 blob 已整个读进内存：一轮并发读取，之后 tantivy 对它们
    /// 的读不再访问对象存储。为一次计算而建，用完即弃，句柄缓存里的 reader 不常驻这些字节。
    /// 归档里没有的路径略过。
    pub async fn with_resident(&self, paths: &[PathBuf]) -> Result<Self> {
        let reads = paths
            .iter()
            .filter(|path| !self.resident.contains_key(*path))
            .filter_map(|path| Some((path.clone(), self.blobs.get(path)?.clone())))
            .map(|(path, meta)| async move {
                let bytes = self.source.read_blob_bytes(&meta, None).await?;
                anyhow::Ok((path, bytes))
            });

        let mut resident = (*self.resident).clone();
        resident.extend(futures::future::try_join_all(reads).await?);

        Ok(Self {
            resident: Arc::new(resident),
            ..self.clone()
        })
    }

    /// 归档里 `path` 那个文件的字节数；没有这个文件时为 `None`。
    pub fn blob_len(&self, path: &Path) -> Option<u64> {
        self.blobs.get(path).map(|blob| blob.length)
    }

    /// 归档写入时记下的属性。
    pub fn property(&self, key: &str) -> Option<&str> {
        self.puffin_meta.properties.get(key).map(String::as_str)
    }

    /// 本 reader 已读到的 footer 状态，供 footer cache 保存；不发 IO。
    pub fn footer(&self, schema: Schema) -> TantivyFooter {
        TantivyFooter {
            puffin_meta: self.puffin_meta.clone(),
            footer_payload_bytes: self.footer_payload.clone(),
            schema,
            atomic_files: self.atomic_files.clone(),
            object_size: self.source.size(),
            hot_cache: self.hot_cache.clone(),
        }
    }
}

fn blob_map(meta: &PuffinMeta) -> HashMap<PathBuf, Arc<BlobMetadata>> {
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

    blobs
}

/// Puffin blob 的 tantivy FileHandle 适配。
///
/// tantivy 在 search 路径上走 **sync** `read_bytes`。命中 hot cache 的读直接从内存返回；
/// 其余读只取被请求的那段字节，在常驻的 [`read_runtime`] 上执行后阻塞等待结果。
#[derive(Debug)]
struct PuffinSliceHandle {
    source: Arc<PuffinBytesReader>,
    meta: Arc<BlobMetadata>,
    path: PathBuf,
    hot_cache: Arc<HotCache>,
}

impl HasLen for PuffinSliceHandle {
    fn len(&self) -> usize {
        self.meta.length as usize
    }
}

impl PuffinSliceHandle {
    /// blob 内的相对范围，截到 blob 末尾，避免读进下一个 blob。
    fn blob_range(&self, byte_range: Range<usize>) -> Range<u64> {
        let end = (byte_range.end as u64).min(self.meta.length);
        let start = (byte_range.start as u64).min(end);
        start..end
    }

    fn read_blocking(&self, byte_range: Range<usize>) -> io::Result<Bytes> {
        let source = self.source.clone();
        let meta = self.meta.clone();
        let range = self.blob_range(byte_range);
        let (tx, rx) = std::sync::mpsc::channel();

        read_runtime().spawn(async move {
            let result = source
                .read_blob_bytes(&meta, Some(range))
                .await
                .map_err(|e| io::Error::other(format!("puffin range read: {e}")));

            let _ = tx.send(result);
        });

        rx.recv()
            .map_err(|e| io::Error::other(format!("puffin read channel: {e}")))?
    }
}

/// 执行 tantivy sync 读的常驻 runtime。读在这里跑而不是在调用方线程上临时建 runtime：
/// 对象存储客户端建立的连接、块缓存派生的任务都随它常驻，不会随一次读结束而失效。
fn read_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("puffin-read")
            .enable_all()
            .build()
            .expect("puffin read runtime")
    })
}

#[async_trait::async_trait]
impl FileHandle for PuffinSliceHandle {
    fn read_bytes(&self, byte_range: Range<usize>) -> io::Result<OwnedBytes> {
        if byte_range.is_empty() {
            return Ok(OwnedBytes::empty());
        }

        if let Some(bytes) = self.hot_cache.get(&self.path, &byte_range) {
            return Ok(OwnedBytes::new(bytes.to_vec()));
        }

        Ok(OwnedBytes::new(self.read_blocking(byte_range)?.to_vec()))
    }

    async fn read_bytes_async(&self, byte_range: Range<usize>) -> io::Result<OwnedBytes> {
        if byte_range.is_empty() {
            return Ok(OwnedBytes::empty());
        }

        if let Some(bytes) = self.hot_cache.get(&self.path, &byte_range) {
            return Ok(OwnedBytes::new(bytes.to_vec()));
        }

        let bytes = self
            .source
            .read_blob_bytes(&self.meta, Some(self.blob_range(byte_range)))
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
        if let Some(bytes) = self.resident.get(path) {
            return Ok(Arc::new(StaticBytesHandle {
                bytes: bytes.clone(),
            }));
        }

        if let Some(meta) = self.blobs.get(path) {
            return Ok(Arc::new(PuffinSliceHandle {
                source: self.source.clone(),
                meta: meta.clone(),
                path: path.to_path_buf(),
                hot_cache: self.hot_cache.clone(),
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

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        fmt,
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
    };

    use futures::stream::BoxStream;
    use object_store::{
        CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
        ObjectStoreExt, PutMultipartOptions, PutOptions, PutPayload, PutResult, memory::InMemory,
        path::Path as ObjectPath,
    };
    use tantivy::{
        Index, ReloadPolicy, TantivyDocument, Term,
        directory::Directory,
        query::{Query, TermQuery},
        schema::{IndexRecordOption, STRING, Schema, TEXT},
    };

    use super::*;
    use crate::{
        puffin::{BlobTypes, writer::PuffinBytesWriter},
        puffin_directory::{MANAGED_JSON, META_JSON, PuffinDirWriter},
    };

    /// Counts GET requests to the wrapped store.
    #[derive(Debug)]
    struct Counting {
        inner: InMemory,
        gets: AtomicU64,
    }

    impl fmt::Display for Counting {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("Counting")
        }
    }

    #[async_trait::async_trait]
    impl ObjectStore for Counting {
        async fn put_opts(
            &self,
            location: &ObjectPath,
            payload: PutPayload,
            options: PutOptions,
        ) -> object_store::Result<PutResult> {
            self.inner.put_opts(location, payload, options).await
        }

        async fn put_multipart_opts(
            &self,
            location: &ObjectPath,
            options: PutMultipartOptions,
        ) -> object_store::Result<Box<dyn MultipartUpload>> {
            self.inner.put_multipart_opts(location, options).await
        }

        async fn get_opts(
            &self,
            location: &ObjectPath,
            options: GetOptions,
        ) -> object_store::Result<GetResult> {
            self.gets.fetch_add(1, Ordering::SeqCst);
            self.inner.get_opts(location, options).await
        }

        fn delete_stream(
            &self,
            locations: BoxStream<'static, object_store::Result<ObjectPath>>,
        ) -> BoxStream<'static, object_store::Result<ObjectPath>> {
            self.inner.delete_stream(locations)
        }

        fn list(
            &self,
            prefix: Option<&ObjectPath>,
        ) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
            self.inner.list(prefix)
        }

        async fn list_with_delimiter(
            &self,
            prefix: Option<&ObjectPath>,
        ) -> object_store::Result<ListResult> {
            self.inner.list_with_delimiter(prefix).await
        }

        async fn copy_opts(
            &self,
            from: &ObjectPath,
            to: &ObjectPath,
            options: CopyOptions,
        ) -> object_store::Result<()> {
            self.inner.copy_opts(from, to, options).await
        }
    }

    /// An archive with a low-cardinality `path` field and a tokenized `message` field.
    fn archive() -> Vec<u8> {
        let dir = PuffinDirWriter::new().unwrap();
        let mut schema = Schema::builder();
        let path = schema.add_text_field("path", STRING);
        let message = schema.add_text_field("message", TEXT);
        let index = Index::create(
            dir.clone(),
            schema.build(),
            tantivy::IndexSettings::default(),
        )
        .unwrap();
        let mut writer: tantivy::IndexWriter = index.writer(50_000_000).unwrap();

        for row in 0..2_000 {
            let mut doc = TantivyDocument::default();
            doc.add_text(path, format!("/api/p{}", row % 20));
            doc.add_text(message, format!("request {row} served id{:x}", row * 7919));
            writer.add_document(doc).unwrap();
        }

        writer.commit().unwrap();
        drop(writer);
        drop(index);
        dir.to_puffin_bytes().unwrap()
    }

    /// The same blobs in the order and with the placeholder archives were written with before
    /// the hot cache existed.
    fn legacy_archive(current: &[u8]) -> Vec<u8> {
        let size = current.len() as u64;
        let payload_size = u32::from_le_bytes(
            current[current.len() - 12..current.len() - 8]
                .try_into()
                .unwrap(),
        ) as u64;
        let payload_start = (size - 12 - payload_size) as usize;
        let meta: PuffinMeta =
            serde_json::from_slice(&current[payload_start..current.len() - 12]).unwrap();

        let mut blobs: Vec<&BlobMetadata> = meta
            .blobs
            .iter()
            .filter(|blob| blob.properties.get("blob_tag").unwrap() != FOOTER_CACHE_BLOB_TAG)
            .collect();
        blobs.sort_by_key(|blob| blob.properties.get("blob_tag").unwrap().clone());

        let mut out = Vec::new();
        let mut writer = PuffinBytesWriter::new(&mut out);

        for blob in blobs {
            let range = blob.absolute_range(None);
            let bytes = &current[range.start as usize..range.end as usize];
            let tag = blob.properties.get("blob_tag").unwrap().clone();
            writer
                .add_blob(bytes, BlobTypes::TantivySegmentV1, tag)
                .unwrap();
        }

        writer
            .add_blob(
                b"__placeholder_footer_cache__",
                BlobTypes::TantivyFooterV1,
                FOOTER_CACHE_BLOB_TAG.to_string(),
            )
            .unwrap();
        writer.finish().unwrap();
        out
    }

    async fn stored(bytes: Vec<u8>) -> (Arc<Counting>, ObjectPath, u64) {
        let store = Arc::new(Counting {
            inner: InMemory::new(),
            gets: AtomicU64::new(0),
        });
        let location = ObjectPath::from("archive.ttv");
        let size = bytes.len() as u64;
        store.put(&location, bytes.into()).await.unwrap();
        (store, location, size)
    }

    fn count(reader: PuffinDirReader, field: &str, value: &str) -> usize {
        let index = Index::open(reader).unwrap();
        let field = index.schema().get_field(field).unwrap();
        let searcher = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .unwrap()
            .searcher();

        TermQuery::new(
            Term::from_field_text(field, value),
            IndexRecordOption::Basic,
        )
        .count(&searcher)
        .unwrap()
    }

    #[tokio::test]
    async fn materializes_both_tantivy_metadata_files() {
        let dir = PuffinDirWriter::new().unwrap();
        let mut schema = Schema::builder();
        schema.add_text_field("message", STRING);
        let index = Index::create(
            dir.clone(),
            schema.build(),
            tantivy::IndexSettings::default(),
        )
        .unwrap();
        drop(index);

        let bytes = dir.to_puffin_bytes().unwrap();
        let size = bytes.len() as u64;
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let location = ObjectPath::from("metadata.ttv");
        store.put(&location, bytes.into()).await.unwrap();

        let reader = PuffinDirReader::from_object_store(store, location, size)
            .await
            .unwrap();
        assert!(reader.atomic_files.contains_key(Path::new(META_JSON)));
        assert!(reader.atomic_files.contains_key(Path::new(MANAGED_JSON)));

        let managed = reader.atomic_read(Path::new(MANAGED_JSON)).unwrap();
        let _: HashSet<PathBuf> = serde_json::from_slice(&managed).unwrap();
        Index::open(reader).unwrap();
    }

    /// Opening reads the tail once; opening the segments and looking up a low-cardinality term
    /// then come entirely from the hot cache.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn new_archive_opens_and_counts_from_one_tail_read() {
        let (store, location, size) = stored(archive()).await;
        let reader = PuffinDirReader::from_object_store(store.clone(), location, size)
            .await
            .unwrap();

        assert_eq!(store.gets.load(Ordering::SeqCst), 1);
        assert!(!reader.hot_cache.is_empty());

        let matches = tokio::task::spawn_blocking(move || count(reader, "path", "/api/p3"))
            .await
            .unwrap();

        assert_eq!(matches, 100);
        assert_eq!(
            store.gets.load(Ordering::SeqCst),
            1,
            "segment open hit the store"
        );
    }

    /// Opening without the object size reads the tail once and learns the size from that answer.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tail_open_needs_neither_the_size_nor_a_head_request() {
        let (store, location, _size) = stored(archive()).await;
        let reader = PuffinDirReader::from_object_store_tail(store.clone(), location)
            .await
            .unwrap();

        assert_eq!(store.gets.load(Ordering::SeqCst), 1);

        let matches = tokio::task::spawn_blocking(move || count(reader, "path", "/api/p3"))
            .await
            .unwrap();

        assert_eq!(matches, 100);
        assert_eq!(store.gets.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn tail_open_reports_a_missing_archive_as_not_found() {
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let error = PuffinDirReader::from_object_store_tail(store, ObjectPath::from("missing.ttv"))
            .await
            .unwrap_err();

        assert!(error.chain().any(|cause| matches!(
            cause.downcast_ref::<object_store::Error>(),
            Some(object_store::Error::NotFound { .. })
        )));
    }

    /// A cached footer reopens the archive without any request.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cached_footer_reopens_without_requests() {
        let (store, location, size) = stored(archive()).await;
        let opened = PuffinDirReader::from_object_store(store.clone(), location.clone(), size)
            .await
            .unwrap();
        let schema = Index::open(opened.clone()).unwrap().schema();
        let footer = opened.footer(schema);
        let before = store.gets.load(Ordering::SeqCst);

        let reader = PuffinDirReader::from_cached_footer(store.clone(), location, size, &footer);
        let matches = tokio::task::spawn_blocking(move || count(reader, "path", "/api/p7"))
            .await
            .unwrap();

        assert_eq!(matches, 100);
        assert_eq!(store.gets.load(Ordering::SeqCst), before);
    }

    /// Resident blobs are read in one round up front; reads of them afterwards stay in memory and
    /// answer as the store would.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn resident_blobs_are_read_up_front_and_answer_the_same() {
        let (store, location, size) = stored(legacy_archive(&archive())).await;
        let reader = PuffinDirReader::from_object_store(store.clone(), location, size)
            .await
            .unwrap();

        let segment_files = reader
            .blobs
            .keys()
            .filter(|path| path.extension().is_some_and(|ext| ext != "json"))
            .cloned()
            .chain([PathBuf::from("absent.idx")])
            .collect::<Vec<_>>();

        let opened = store.gets.load(Ordering::SeqCst);
        let resident = reader.with_resident(&segment_files).await.unwrap();

        assert_eq!(
            store.gets.load(Ordering::SeqCst),
            opened + segment_files.len() as u64 - 1
        );

        let read = store.gets.load(Ordering::SeqCst);

        let matches = tokio::task::spawn_blocking(move || {
            (
                count(resident.clone(), "path", "/api/p3"),
                count(resident, "message", "served"),
            )
        })
        .await
        .unwrap();

        assert_eq!(matches, (100, 2_000));
        assert_eq!(store.gets.load(Ordering::SeqCst), read);
    }

    /// Archives from before the hot cache keep working; their reads go to the store by range.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn legacy_archive_answers_the_same_counts() {
        let (store, location, size) = stored(legacy_archive(&archive())).await;
        let reader = PuffinDirReader::from_object_store(store.clone(), location, size)
            .await
            .unwrap();

        assert!(reader.hot_cache.is_empty());

        let (path_matches, message_matches) = tokio::task::spawn_blocking(move || {
            (
                count(reader.clone(), "path", "/api/p3"),
                count(reader, "message", "served"),
            )
        })
        .await
        .unwrap();

        assert_eq!(path_matches, 100);
        assert_eq!(message_matches, 2_000);
    }
}
