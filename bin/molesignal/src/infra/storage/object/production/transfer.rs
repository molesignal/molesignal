// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 大对象传输：达到 `[object_store]` 阈值的写入改用 multipart 上传，达到阈值的范围读取
//! 拆成固定大小的块并发读取。
//!
//! 两条路径都由多个后端请求组成。每个请求各自占用并发许可并受 `op_timeout_secs` 约束；
//! 外层只负责整体重试（multipart 上传失败时放弃已上传的分片，从头重试）。单次传输最多占用
//! 一半的并发许可，给其它请求留出余量。

use std::ops::Range;

use bytes::Bytes;
use futures::{
    StreamExt, future,
    stream::{self, FuturesUnordered},
};
use object_store::{
    Attributes, Error as OsError, GetOptions, GetRange, GetResult, GetResultPayload,
    MultipartUpload, ObjectMeta, ObjectStore, ObjectStoreExt, PutMode, PutMultipartOptions,
    PutOptions, PutPayload, PutResult, Result as ObjectStoreResult, path::Path as ObjPath,
};

use super::{Budget, ProductionObjectStore, telemetry::timeout_error};
use crate::infra::storage::layout::StorageLayout;

#[cfg(test)]
mod tests;

const MIB: u64 = 1024 * 1024;

/// 单次 multipart 上传最多 10 000 个分片（S3 上限）。
const MAX_PARTS: u64 = 10_000;

/// 一块范围读取的结果。
struct Chunk {
    bytes: Bytes,
    meta: ObjectMeta,
    attributes: Attributes,
}

impl ProductionObjectStore {
    fn multipart_threshold(&self) -> u64 {
        u64::from(self.settings.multipart_threshold_mb.max(1)) * MIB
    }

    /// 分片大小；对象很大时放大，使分片数不超过后端上限。
    fn part_size(&self, total: u64) -> usize {
        let configured = u64::from(self.settings.multipart_part_size_mb.max(1)) * MIB;
        usize::try_from(configured.max(total.div_ceil(MAX_PARTS))).unwrap_or(usize::MAX)
    }

    fn range_threshold(&self) -> u64 {
        u64::from(self.settings.range_threshold_mb.max(1)) * MIB
    }

    fn range_chunk(&self) -> u64 {
        u64::from(self.settings.range_chunk_mb.max(1)) * MIB
    }

    fn transfer_parallelism(&self) -> usize {
        (self.settings.max_concurrency.max(1) as usize).div_ceil(2)
    }

    /// 达到阈值的写入走 multipart。数据文件与索引 Artifact 以 create-if-absent 写入，上传前
    /// 先确认对象不存在；manifest 的版本协议依赖单次条件写，带版本条件的写入同样只能是单个请求。
    pub(super) fn wants_multipart(
        &self,
        location: &ObjPath,
        payload: &PutPayload,
        options: &PutOptions,
    ) -> bool {
        payload.content_length() as u64 >= self.multipart_threshold()
            && match options.mode {
                PutMode::Overwrite => true,
                PutMode::Create => !StorageLayout::is_manifest_key(location.as_ref()),
                PutMode::Update(_) => false,
            }
    }

    /// create-if-absent 的 multipart 写入：对象已存在时与单次条件写一样返回 `AlreadyExists`。
    /// 对象 key 由单一写入者生成，因此检查与上传之间不会出现另一个写入者。
    async fn ensure_absent(&self, location: &ObjPath) -> ObjectStoreResult<()> {
        match self
            .run("head", Some(location), || self.inner.head(location), |_| 0)
            .await
        {
            Ok(_) => Err(OsError::AlreadyExists {
                path: location.to_string(),
                source: "object already exists".to_owned().into(),
            }),
            Err(OsError::NotFound { .. }) => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub(super) async fn put_in_parts(
        &self,
        location: &ObjPath,
        payload: PutPayload,
        options: PutOptions,
    ) -> ObjectStoreResult<PutResult> {
        if matches!(options.mode, PutMode::Create) {
            self.ensure_absent(location).await?;
        }

        let part_size = self.part_size(payload.content_length() as u64);

        let options = PutMultipartOptions {
            tags: options.tags,
            attributes: options.attributes,
            extensions: options.extensions,
        };

        self.run_with(
            "put_multipart",
            Some(location),
            Budget::Composite,
            || self.upload_parts(location, &payload, part_size, options.clone()),
            // 字节数由 `multipart_complete` 统计。
            |_| 0,
        )
        .await
    }

    async fn upload_parts(
        &self,
        location: &ObjPath,
        payload: &PutPayload,
        part_size: usize,
        options: PutMultipartOptions,
    ) -> ObjectStoreResult<PutResult> {
        let mut upload = self.put_multipart_opts(location, options).await?;

        match self.send_parts(upload.as_mut(), payload, part_size).await {
            Ok(result) => Ok(result),
            Err(error) => {
                // 尽力放弃：后端会自行回收长期未完成的上传。
                let _ = upload.abort().await;
                Err(error)
            }
        }
    }

    async fn send_parts(
        &self,
        upload: &mut dyn MultipartUpload,
        payload: &PutPayload,
        part_size: usize,
    ) -> ObjectStoreResult<PutResult> {
        let parallelism = self.transfer_parallelism();
        let timeout = self.op_timeout();
        let backend = self.backend;
        let mut in_flight = FuturesUnordered::new();

        for part in split_payload(payload, part_size) {
            while in_flight.len() >= parallelism {
                if let Some(result) = in_flight.next().await {
                    result?;
                }
            }

            let permit = self.acquire_permit().await?;
            let sending = upload.put_part(part);

            in_flight.push(async move {
                let _permit = permit;

                tokio::time::timeout(timeout, sending)
                    .await
                    .unwrap_or_else(|_| Err(timeout_error(backend, "multipart_part")))
            });
        }

        while let Some(result) = in_flight.next().await {
            result?;
        }

        let _permit = self.acquire_permit().await?;

        tokio::time::timeout(timeout, upload.complete())
            .await
            .unwrap_or_else(|_| Err(timeout_error(backend, "multipart_complete")))
    }

    /// 带界限的范围读取达到阈值时返回该范围；`head`、`Offset`、`Suffix` 读取原样透传。
    pub(super) fn chunked_range(&self, options: &GetOptions) -> Option<Range<u64>> {
        if options.head {
            return None;
        }

        match options.range.as_ref()? {
            GetRange::Bounded(range)
                if range.end > range.start && range.end - range.start >= self.range_threshold() =>
            {
                Some(range.clone())
            }
            _ => None,
        }
    }

    /// 第一块沿用调用方的条件并带回对象元数据；其余块带上第一块的 ETag，保证来自同一版本，
    /// 然后按顺序并发读取。范围越过对象末尾时截到对象末尾。
    pub(super) async fn get_range_in_chunks(
        &self,
        location: &ObjPath,
        options: GetOptions,
        range: Range<u64>,
    ) -> ObjectStoreResult<GetResult> {
        let chunk_size = self.range_chunk();
        let first_end = range.start.saturating_add(chunk_size).min(range.end);

        let first = self
            .read_chunk(
                location,
                GetOptions {
                    range: Some(GetRange::Bounded(range.start..first_end)),
                    ..options
                },
            )
            .await?;

        let Chunk {
            bytes: first_bytes,
            meta,
            attributes,
        } = first;

        let resume = range.start + first_bytes.len() as u64;
        let end = range.end.min(meta.size);

        let store = self.clone();
        let location = location.clone();
        let etag = meta.e_tag.clone();

        let rest = stream::iter(chunk_ranges(resume..end, chunk_size))
            .map(move |chunk| {
                let store = store.clone();
                let location = location.clone();

                let options = GetOptions {
                    if_match: etag.clone(),
                    range: Some(GetRange::Bounded(chunk)),
                    ..GetOptions::default()
                };

                async move {
                    store
                        .read_chunk(&location, options)
                        .await
                        .map(|chunk| chunk.bytes)
                }
            })
            .buffered(self.transfer_parallelism());

        let payload = stream::once(future::ready(Ok(first_bytes)))
            .chain(rest)
            .boxed();

        Ok(GetResult {
            payload: GetResultPayload::Stream(payload),
            meta,
            range: range.start..end,
            attributes,
        })
    }

    async fn read_chunk(
        &self,
        location: &ObjPath,
        options: GetOptions,
    ) -> ObjectStoreResult<Chunk> {
        self.run(
            "get_range",
            Some(location),
            || {
                let options = options.clone();

                async move {
                    let result = self.inner.get_opts(location, options).await?;
                    let (meta, attributes) = (result.meta.clone(), result.attributes.clone());
                    let bytes = result.bytes().await?;

                    Ok(Chunk {
                        bytes,
                        meta,
                        attributes,
                    })
                }
            },
            |chunk| chunk.bytes.len() as u64,
        )
        .await
    }

    /// 多个范围：达到阈值的范围各自分块并发读取，其余范围仍合并为一次后端调用。
    pub(super) async fn get_ranges_split(
        &self,
        location: &ObjPath,
        ranges: &[Range<u64>],
    ) -> ObjectStoreResult<Vec<Bytes>> {
        let threshold = self.range_threshold();
        let is_large =
            |range: &Range<u64>| range.end > range.start && range.end - range.start >= threshold;

        if !ranges.iter().any(is_large) {
            return self.get_ranges_whole(location, ranges).await;
        }

        let (large, small): (Vec<_>, Vec<_>) = ranges
            .iter()
            .cloned()
            .enumerate()
            .partition(|(_, range)| is_large(range));

        let small_ranges: Vec<_> = small.iter().map(|(_, range)| range.clone()).collect();

        let (small_bytes, large_bytes) = futures::try_join!(
            async {
                if small_ranges.is_empty() {
                    Ok(Vec::new())
                } else {
                    self.get_ranges_whole(location, &small_ranges).await
                }
            },
            future::try_join_all(large.iter().map(|(_, range)| async move {
                self.get_range_in_chunks(location, GetOptions::default(), range.clone())
                    .await?
                    .bytes()
                    .await
            })),
        )?;

        let mut out = vec![Bytes::new(); ranges.len()];

        for ((index, _), bytes) in small.into_iter().zip(small_bytes) {
            out[index] = bytes;
        }

        for ((index, _), bytes) in large.into_iter().zip(large_bytes) {
            out[index] = bytes;
        }

        Ok(out)
    }

    async fn get_ranges_whole(
        &self,
        location: &ObjPath,
        ranges: &[Range<u64>],
    ) -> ObjectStoreResult<Vec<Bytes>> {
        let ranges = ranges.to_vec();

        self.run(
            "get_ranges",
            Some(location),
            || self.inner.get_ranges(location, &ranges),
            |values| values.iter().map(|value| value.len() as u64).sum(),
        )
        .await
    }
}

/// 把 `span` 切成不超过 `size` 的连续块。
fn chunk_ranges(span: Range<u64>, size: u64) -> impl Iterator<Item = Range<u64>> {
    let end = span.end;
    let step = usize::try_from(size.max(1)).unwrap_or(usize::MAX);
    span.step_by(step)
        .map(move |start| start..start.saturating_add(size).min(end))
}

/// 按 `part_size` 切分 payload；复用原有的 `Bytes`，不复制数据。
fn split_payload(payload: &PutPayload, part_size: usize) -> Vec<PutPayload> {
    let mut parts = Vec::new();
    let mut current: Vec<Bytes> = Vec::new();
    let mut current_len = 0_usize;

    for chunk in payload.iter() {
        let mut rest = chunk.clone();

        while !rest.is_empty() {
            let take = (part_size - current_len).min(rest.len());
            current.push(rest.split_to(take));
            current_len += take;

            if current_len == part_size {
                parts.push(current.drain(..).collect());
                current_len = 0;
            }
        }
    }

    if !current.is_empty() {
        parts.push(current.drain(..).collect());
    }

    parts
}
