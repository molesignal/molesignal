// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! An in-memory object store whose WAL writes can be made to fail or to take a while.

use std::{
    fmt,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use futures::stream::BoxStream;
use object_store::{
    CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    PutMultipartOptions, PutOptions, PutPayload, PutResult, Result, memory::InMemory, path::Path,
};

#[derive(Debug, Default)]
pub(super) struct FaultyStore {
    inner: InMemory,
    /// WAL writes still to fail.
    failing_wal_puts: AtomicUsize,
    /// How long each WAL write takes.
    wal_put_delay: Mutex<Duration>,
    /// WAL writes asked for, failed ones included.
    wal_puts: AtomicUsize,
}

impl FaultyStore {
    /// Fails the next `count` WAL writes.
    pub(super) fn fail_wal_puts(&self, count: usize) {
        self.failing_wal_puts.store(count, Ordering::SeqCst);
    }

    pub(super) fn delay_wal_puts(&self, delay: Duration) {
        *self.wal_put_delay.lock().unwrap() = delay;
    }

    pub(super) fn wal_puts(&self) -> usize {
        self.wal_puts.load(Ordering::SeqCst)
    }
}

impl fmt::Display for FaultyStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "FaultyStore")
    }
}

#[async_trait::async_trait]
impl ObjectStore for FaultyStore {
    async fn put_opts(
        &self,
        location: &Path,
        payload: PutPayload,
        options: PutOptions,
    ) -> Result<PutResult> {
        if location.as_ref().starts_with("wal/") {
            self.wal_puts.fetch_add(1, Ordering::SeqCst);
            let delay = *self.wal_put_delay.lock().unwrap();

            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }

            let failed = self
                .failing_wal_puts
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                    left.checked_sub(1)
                })
                .is_ok();

            if failed {
                return Err(object_store::Error::Generic {
                    store: "faulty",
                    source: "injected WAL write failure".into(),
                });
            }
        }

        self.inner.put_opts(location, payload, options).await
    }

    async fn put_multipart_opts(
        &self,
        location: &Path,
        options: PutMultipartOptions,
    ) -> Result<Box<dyn MultipartUpload>> {
        self.inner.put_multipart_opts(location, options).await
    }

    async fn get_opts(&self, location: &Path, options: GetOptions) -> Result<GetResult> {
        self.inner.get_opts(location, options).await
    }

    fn delete_stream(
        &self,
        locations: BoxStream<'static, Result<Path>>,
    ) -> BoxStream<'static, Result<Path>> {
        self.inner.delete_stream(locations)
    }

    fn list(&self, prefix: Option<&Path>) -> BoxStream<'static, Result<ObjectMeta>> {
        self.inner.list(prefix)
    }

    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> Result<ListResult> {
        self.inner.list_with_delimiter(prefix).await
    }

    async fn copy_opts(&self, from: &Path, to: &Path, options: CopyOptions) -> Result<()> {
        self.inner.copy_opts(from, to, options).await
    }
}
