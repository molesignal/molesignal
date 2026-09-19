// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{fmt, ops::Range, sync::Arc};

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::{StreamExt, TryFutureExt, stream::BoxStream};
use object_store::{
    Attributes, CopyOptions, GetOptions, GetResult, GetResultPayload, ListResult, MultipartUpload,
    ObjectMeta, ObjectStore, PutMultipartOptions, PutOptions, PutPayload, PutResult, RenameOptions,
    path::Path,
};

use super::{ObjectRegistry, block_cache::RangeBlockCache, metrics::ReadTimer};

pub(super) struct CachedObjectStore {
    origin: Arc<dyn ObjectStore>,
    registry: Arc<ObjectRegistry>,
    cache: Arc<RangeBlockCache>,
}

impl CachedObjectStore {
    pub(super) fn new(
        origin: Arc<dyn ObjectStore>,
        registry: Arc<ObjectRegistry>,
        cache: Arc<RangeBlockCache>,
    ) -> Self {
        Self {
            origin,
            registry,
            cache,
        }
    }
}

impl fmt::Debug for CachedObjectStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CachedObjectStore")
            .finish_non_exhaustive()
    }
}

impl fmt::Display for CachedObjectStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MoleSignalCachedObjectStore")
    }
}

#[async_trait]
#[deny(clippy::missing_trait_methods)]
impl ObjectStore for CachedObjectStore {
    async fn put_opts(
        &self,
        _location: &Path,
        _payload: PutPayload,
        _options: PutOptions,
    ) -> object_store::Result<PutResult> {
        Err(read_only("put"))
    }

    async fn put_multipart_opts(
        &self,
        _location: &Path,
        _options: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        Err(read_only("put_multipart"))
    }

    async fn get_opts(
        &self,
        location: &Path,
        options: GetOptions,
    ) -> object_store::Result<GetResult> {
        let Some(registered) = self.registry.get(location.as_ref()) else {
            return self.origin.get_opts(location, options).await;
        };
        if options.version.is_some()
            || options.if_modified_since.is_some()
            || options.if_unmodified_since.is_some()
        {
            return self.origin.get_opts(location, options).await;
        }
        let meta = object_meta(location.clone(), &registered.object);
        options.check_preconditions(&meta)?;
        if options.head {
            return Ok(GetResult {
                payload: GetResultPayload::Stream(futures::stream::empty().boxed()),
                meta,
                range: 0..0,
                attributes: Attributes::default(),
            });
        }
        let range = match options.range.as_ref() {
            Some(range) => range
                .as_range(registered.object.size_bytes)
                .map_err(|error| object_store::Error::Generic {
                    store: "CachedObjectStore",
                    source: Box::new(error),
                })?,
            None => 0..registered.object.size_bytes,
        };
        let timer = ReadTimer::start();
        let bytes = self
            .cache
            .read_range(&self.origin, &registered, range.clone())
            .await?;
        timer.finish(bytes.len());
        Ok(GetResult {
            payload: GetResultPayload::Stream(
                futures::stream::once(async move { Ok(bytes) }).boxed(),
            ),
            meta,
            range,
            attributes: Attributes::default(),
        })
    }

    async fn get_ranges(
        &self,
        location: &Path,
        ranges: &[Range<u64>],
    ) -> object_store::Result<Vec<Bytes>> {
        if self.registry.get(location.as_ref()).is_none() {
            return self.origin.get_ranges(location, ranges).await;
        }
        futures::future::try_join_all(ranges.iter().cloned().map(|range| {
            self.get_opts(location, GetOptions::new().with_range(Some(range)))
                .and_then(GetResult::bytes)
        }))
        .await
    }

    fn delete_stream(
        &self,
        locations: BoxStream<'static, object_store::Result<Path>>,
    ) -> BoxStream<'static, object_store::Result<Path>> {
        locations
            .map(|location| location.and_then(|_| Err(read_only("delete"))))
            .boxed()
    }

    fn list(&self, prefix: Option<&Path>) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        self.origin.list(prefix)
    }

    fn list_with_offset(
        &self,
        prefix: Option<&Path>,
        offset: &Path,
    ) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        self.origin.list_with_offset(prefix, offset)
    }

    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> object_store::Result<ListResult> {
        self.origin.list_with_delimiter(prefix).await
    }

    async fn copy_opts(
        &self,
        _from: &Path,
        _to: &Path,
        _options: CopyOptions,
    ) -> object_store::Result<()> {
        Err(read_only("copy"))
    }

    async fn rename_opts(
        &self,
        _from: &Path,
        _to: &Path,
        _options: RenameOptions,
    ) -> object_store::Result<()> {
        Err(read_only("rename"))
    }
}

fn object_meta(location: Path, object: &crate::domain::storage::StoredObject) -> ObjectMeta {
    ObjectMeta {
        location,
        last_modified: DateTime::<Utc>::from_timestamp(0, 0).expect("unix epoch is valid"),
        size: object.size_bytes,
        e_tag: object
            .etag
            .clone()
            .or_else(|| Some(object.checksum.0.clone())),
        version: None,
    }
}

fn read_only(operation: &str) -> object_store::Error {
    object_store::Error::NotImplemented {
        operation: operation.to_owned(),
        implementer: "CachedObjectStore".to_owned(),
    }
}
