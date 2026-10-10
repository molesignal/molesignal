// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    fmt,
    sync::{Arc, Mutex},
};

use futures::stream::BoxStream;
use object_store::{
    CopyOptions, Error as OsError, ListResult, ObjectStoreExt, UploadPart, memory::InMemory,
};

use super::*;
use crate::config::ObjectStoreSettings;

const KIB: usize = 1024;
const MIB_USIZE: usize = 1024 * 1024;

#[derive(Debug, Default)]
struct Calls {
    puts: Vec<usize>,
    /// 每次 multipart 上传的分片大小（按调用顺序）。
    uploads: Vec<Vec<usize>>,
    aborts: usize,
    /// 每次 `get_opts` 的范围，以及请求是否带了 `if_match`。
    gets: Vec<(Option<GetRange>, bool)>,
}

/// 记录所有写入与读取形态的内存对象存储；`fail_part` 让对应序号的分片上传失败。
#[derive(Debug)]
struct Recording {
    inner: InMemory,
    calls: Arc<Mutex<Calls>>,
    fail_part: Option<usize>,
}

impl fmt::Display for Recording {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Recording")
    }
}

#[async_trait::async_trait]
impl ObjectStore for Recording {
    async fn put_opts(
        &self,
        location: &ObjPath,
        payload: PutPayload,
        options: PutOptions,
    ) -> ObjectStoreResult<PutResult> {
        self.calls
            .lock()
            .unwrap()
            .puts
            .push(payload.content_length());

        self.inner.put_opts(location, payload, options).await
    }

    async fn put_multipart_opts(
        &self,
        location: &ObjPath,
        options: PutMultipartOptions,
    ) -> ObjectStoreResult<Box<dyn MultipartUpload>> {
        let inner = self.inner.put_multipart_opts(location, options).await?;

        let index = {
            let mut calls = self.calls.lock().unwrap();
            calls.uploads.push(Vec::new());
            calls.uploads.len() - 1
        };

        Ok(Box::new(RecordingUpload {
            inner,
            calls: self.calls.clone(),
            index,
            fail_part: self.fail_part,
        }))
    }

    async fn get_opts(
        &self,
        location: &ObjPath,
        options: GetOptions,
    ) -> ObjectStoreResult<GetResult> {
        self.calls
            .lock()
            .unwrap()
            .gets
            .push((options.range.clone(), options.if_match.is_some()));

        self.inner.get_opts(location, options).await
    }

    fn delete_stream(
        &self,
        locations: BoxStream<'static, ObjectStoreResult<ObjPath>>,
    ) -> BoxStream<'static, ObjectStoreResult<ObjPath>> {
        self.inner.delete_stream(locations)
    }

    fn list(&self, prefix: Option<&ObjPath>) -> BoxStream<'static, ObjectStoreResult<ObjectMeta>> {
        self.inner.list(prefix)
    }

    async fn list_with_delimiter(&self, prefix: Option<&ObjPath>) -> ObjectStoreResult<ListResult> {
        self.inner.list_with_delimiter(prefix).await
    }

    async fn copy_opts(
        &self,
        from: &ObjPath,
        to: &ObjPath,
        options: CopyOptions,
    ) -> ObjectStoreResult<()> {
        self.inner.copy_opts(from, to, options).await
    }
}

#[derive(Debug)]
struct RecordingUpload {
    inner: Box<dyn MultipartUpload>,
    calls: Arc<Mutex<Calls>>,
    index: usize,
    fail_part: Option<usize>,
}

#[async_trait::async_trait]
impl MultipartUpload for RecordingUpload {
    fn put_part(&mut self, data: PutPayload) -> UploadPart {
        let number = {
            let mut calls = self.calls.lock().unwrap();
            let parts = &mut calls.uploads[self.index];
            parts.push(data.content_length());
            parts.len()
        };

        if self.fail_part == Some(number) {
            return Box::pin(async {
                Err(OsError::Generic {
                    store: "recording",
                    source: "injected failure".to_string().into(),
                })
            });
        }

        self.inner.put_part(data)
    }

    async fn complete(&mut self) -> ObjectStoreResult<PutResult> {
        self.inner.complete().await
    }

    async fn abort(&mut self) -> ObjectStoreResult<()> {
        self.calls.lock().unwrap().aborts += 1;
        self.inner.abort().await
    }
}

fn settings() -> ObjectStoreSettings {
    ObjectStoreSettings {
        multipart_threshold_mb: 1,
        multipart_part_size_mb: 1,
        range_threshold_mb: 1,
        range_chunk_mb: 1,
        ..ObjectStoreSettings::default()
    }
}

fn wrapped(
    settings: ObjectStoreSettings,
    fail_part: Option<usize>,
) -> (Arc<ProductionObjectStore>, Arc<Mutex<Calls>>) {
    let calls = Arc::new(Mutex::new(Calls::default()));

    let store = ProductionObjectStore::wrap(
        Arc::new(Recording {
            inner: InMemory::new(),
            calls: calls.clone(),
            fail_part,
        }),
        settings,
    );

    (store, calls)
}

fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|index| (index % 251) as u8).collect()
}

#[tokio::test(flavor = "current_thread")]
async fn payload_at_threshold_uploads_in_parts() {
    let (store, calls) = wrapped(settings(), None);
    let data = pattern(2 * MIB_USIZE + 512 * KIB);
    // 分片边界落在原有 `Bytes` 块中间。
    let payload: PutPayload = data.chunks(700 * KIB).map(Bytes::copy_from_slice).collect();
    let path = ObjPath::from("t/large.parquet");

    store.put(&path, payload).await.unwrap();

    {
        let calls = calls.lock().unwrap();
        assert!(calls.puts.is_empty(), "single PUT must not be used");
        assert_eq!(
            calls.uploads,
            vec![vec![MIB_USIZE, MIB_USIZE, 512 * KIB]],
            "parts follow multipart_part_size_mb"
        );
    }
    let stored = store.get(&path).await.unwrap().bytes().await.unwrap();
    assert_eq!(stored.as_ref(), data.as_slice());
}

#[tokio::test(flavor = "current_thread")]
async fn payload_below_threshold_is_a_single_put() {
    let (store, calls) = wrapped(settings(), None);
    let data = pattern(MIB_USIZE - 1);

    store
        .put(
            &ObjPath::from("t/small.bin"),
            PutPayload::from(data.clone()),
        )
        .await
        .unwrap();

    let calls = calls.lock().unwrap();
    assert_eq!(calls.puts, vec![data.len()]);
    assert!(calls.uploads.is_empty());
}

fn create() -> PutOptions {
    PutOptions {
        mode: PutMode::Create,
        ..PutOptions::default()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn create_if_absent_uploads_in_parts_and_refuses_an_existing_object() {
    let (store, calls) = wrapped(settings(), None);
    let path = ObjPath::from("t/segment.parquet");
    let data = pattern(2 * MIB_USIZE);

    store
        .put_opts(&path, PutPayload::from(data.clone()), create())
        .await
        .unwrap();
    let second = store
        .put_opts(&path, PutPayload::from(data.clone()), create())
        .await;

    assert!(matches!(second, Err(OsError::AlreadyExists { .. })));
    {
        let calls = calls.lock().unwrap();
        assert!(calls.puts.is_empty());
        assert_eq!(
            calls.uploads,
            vec![vec![MIB_USIZE, MIB_USIZE]],
            "the second attempt stops at the existence check"
        );
    }
    let stored = store.get(&path).await.unwrap().bytes().await.unwrap();
    assert_eq!(stored.as_ref(), data.as_slice());
}

#[tokio::test(flavor = "current_thread")]
async fn manifests_and_versioned_updates_stay_single_requests() {
    let (store, calls) = wrapped(settings(), None);
    let manifest = ObjPath::from("manifests/v1/org/logs/dataset/p-0-00/1.parquet");
    store
        .put_opts(
            &manifest,
            PutPayload::from(pattern(2 * MIB_USIZE)),
            create(),
        )
        .await
        .unwrap();
    let versioned = ObjPath::from("t/state.json");
    store
        .put(&versioned, PutPayload::from(pattern(10)))
        .await
        .unwrap();
    let current = store.head(&versioned).await.unwrap();
    store
        .put_opts(
            &versioned,
            PutPayload::from(pattern(2 * MIB_USIZE)),
            PutOptions {
                mode: PutMode::Update(object_store::UpdateVersion {
                    e_tag: current.e_tag,
                    version: current.version,
                }),
                ..PutOptions::default()
            },
        )
        .await
        .unwrap();

    let calls = calls.lock().unwrap();
    assert_eq!(calls.puts.len(), 3);
    assert!(calls.uploads.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn failed_part_aborts_the_upload_and_surfaces_the_error() {
    let (store, calls) = wrapped(settings(), Some(2));
    let path = ObjPath::from("t/broken.parquet");

    let result = store
        .put(&path, PutPayload::from(pattern(3 * MIB_USIZE)))
        .await;

    assert!(result.is_err());
    assert_eq!(calls.lock().unwrap().aborts, 1);
    assert!(matches!(
        store.head(&path).await,
        Err(OsError::NotFound { .. })
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn range_at_threshold_is_read_in_chunks() {
    let (store, calls) = wrapped(settings(), None);
    let data = pattern(4 * MIB_USIZE);
    let path = ObjPath::from("t/object.bin");
    store
        .put(&path, PutPayload::from(data.clone()))
        .await
        .unwrap();
    calls.lock().unwrap().gets.clear();

    let start = 100_u64;
    let end = start + 2 * MIB_USIZE as u64 + 512 * KIB as u64;
    let bytes = store.get_range(&path, start..end).await.unwrap();

    assert_eq!(bytes.as_ref(), &data[start as usize..end as usize]);
    let mut gets = calls.lock().unwrap().gets.clone();
    assert_eq!(gets.len(), 3, "2.5 MiB at 1 MiB per chunk");
    // 第一块不带 `if_match`，其余块带第一块的 ETag；到达顺序不固定。
    assert!(
        !gets[0].1,
        "the first chunk carries the caller's conditions"
    );
    assert!(gets[1..].iter().all(|(_, if_match)| *if_match));
    gets.sort_by_key(|(range, _)| match range {
        Some(GetRange::Bounded(range)) => range.start,
        _ => u64::MAX,
    });
    let ranges: Vec<_> = gets
        .into_iter()
        .map(|(range, _)| match range {
            Some(GetRange::Bounded(range)) => range,
            other => panic!("unexpected range {other:?}"),
        })
        .collect();
    assert_eq!(
        ranges,
        vec![
            start..start + MIB_USIZE as u64,
            start + MIB_USIZE as u64..start + 2 * MIB_USIZE as u64,
            start + 2 * MIB_USIZE as u64..end,
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn range_below_threshold_is_a_single_request() {
    let (store, calls) = wrapped(settings(), None);
    let data = pattern(2 * MIB_USIZE);
    let path = ObjPath::from("t/object.bin");
    store
        .put(&path, PutPayload::from(data.clone()))
        .await
        .unwrap();
    calls.lock().unwrap().gets.clear();

    let bytes = store
        .get_range(&path, 10..10 + 512 * KIB as u64)
        .await
        .unwrap();

    assert_eq!(bytes.as_ref(), &data[10..10 + 512 * KIB]);
    assert_eq!(calls.lock().unwrap().gets.len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn chunked_range_is_clamped_to_the_object_end() {
    let (store, _) = wrapped(settings(), None);
    let data = pattern(MIB_USIZE + 512 * KIB);
    let path = ObjPath::from("t/short.bin");
    store
        .put(&path, PutPayload::from(data.clone()))
        .await
        .unwrap();

    let result = store
        .get_opts(
            &path,
            GetOptions::new().with_range(Some(0..4 * MIB_USIZE as u64)),
        )
        .await
        .unwrap();

    assert_eq!(result.range, 0..data.len() as u64);
    assert_eq!(result.bytes().await.unwrap().as_ref(), data.as_slice());
}

#[tokio::test(flavor = "current_thread")]
async fn get_ranges_splits_only_the_large_ranges() {
    let (store, calls) = wrapped(settings(), None);
    let data = pattern(5 * MIB_USIZE);
    let path = ObjPath::from("t/object.bin");
    store
        .put(&path, PutPayload::from(data.clone()))
        .await
        .unwrap();
    calls.lock().unwrap().gets.clear();

    let ranges = vec![
        3 * MIB_USIZE as u64..4 * MIB_USIZE as u64 + 300 * KIB as u64,
        0..64,
        10_000..10_100,
    ];
    let values = store.get_ranges(&path, &ranges).await.unwrap();

    for (range, value) in ranges.iter().zip(&values) {
        assert_eq!(
            value.as_ref(),
            &data[range.start as usize..range.end as usize]
        );
    }
    let gets = calls.lock().unwrap().gets.clone();
    assert!(!gets.is_empty());
    assert!(
        gets.iter().all(|(range, _)| match range {
            Some(GetRange::Bounded(range)) => range.end - range.start <= MIB_USIZE as u64,
            _ => false,
        }),
        "no backend request exceeds range_chunk_mb: {gets:?}"
    );
}

#[test]
fn split_payload_keeps_every_byte_in_order() {
    let data = pattern(25);
    let payload: PutPayload = [&data[..7], &data[7..8], &data[8..20], &data[20..]]
        .into_iter()
        .map(Bytes::copy_from_slice)
        .collect();

    let parts = split_payload(&payload, 10);

    let lengths: Vec<_> = parts.iter().map(PutPayload::content_length).collect();
    assert_eq!(lengths, vec![10, 10, 5]);
    let joined: Vec<u8> = parts
        .iter()
        .flat_map(|part| part.iter().flat_map(|bytes| bytes.iter().copied()))
        .collect();
    assert_eq!(joined, data);
}

#[test]
fn chunk_ranges_cover_the_span_exactly() {
    assert_eq!(
        chunk_ranges(5..26, 10).collect::<Vec<_>>(),
        vec![5..15, 15..25, 25..26]
    );
    assert_eq!(chunk_ranges(5..15, 10).collect::<Vec<_>>(), vec![5..15]);
    assert!(chunk_ranges(9..9, 10).next().is_none());
}

#[test]
fn part_size_grows_to_stay_within_the_part_limit() {
    let store = ProductionObjectStore::wrap(Arc::new(InMemory::new()), settings());
    assert_eq!(store.part_size(3 * MIB), MIB as usize);
    let huge = 20_000 * MIB;
    assert!(huge.div_ceil(store.part_size(huge) as u64) <= MAX_PARTS);
}
