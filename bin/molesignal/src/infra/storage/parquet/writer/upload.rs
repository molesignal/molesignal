// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! One immutable object written in pieces.
//!
//! Bytes are hashed and counted as they arrive. An object that stays smaller than one part is
//! written with a single create-if-absent PUT when it is finished; a larger one becomes a
//! multipart upload once the first part fills, with at most [`PARTS_IN_FLIGHT`] parts uploading
//! at a time. Memory therefore stays at a few parts however large the object grows.

use std::{path::Path as FilePath, sync::Arc};

use bytes::{Bytes, BytesMut};
use futures::{FutureExt, future::BoxFuture};
use object_store::{
    Error as OsError, ObjectStore, ObjectStoreExt, PutMode, PutMultipartOptions, PutOptions,
    PutPayload, WriteMultipart, path::Path,
};
use parquet::{arrow::async_writer::AsyncFileWriter, errors::ParquetError};
use tokio::io::AsyncReadExt;

use crate::{
    domain::storage::{ObjectChecksum, ObjectKey, StoredObject},
    shared::{Error, Result},
};

/// Size of one uploaded part. Object stores take parts of at least 5 MiB, and S3 allows 10 000
/// parts per upload, so one object can reach 160 GiB.
pub(crate) const PART_BYTES: usize = 16 * 1024 * 1024;

/// Parts uploading at once for one object.
const PARTS_IN_FLIGHT: usize = 2;

pub(crate) struct ArtifactUpload {
    store: Arc<dyn ObjectStore>,
    key: ObjectKey,
    location: Path,
    part_bytes: usize,
    hasher: blake3::Hasher,
    size: u64,
    /// Bytes waiting for the first part to fill; empty once the upload is multipart.
    pending: BytesMut,
    multipart: Option<WriteMultipart>,
    finished: Option<StoredObject>,
}

impl ArtifactUpload {
    pub(crate) fn new(store: Arc<dyn ObjectStore>, key: ObjectKey) -> Self {
        Self::with_part_bytes(store, key, PART_BYTES)
    }

    pub(crate) fn with_part_bytes(
        store: Arc<dyn ObjectStore>,
        key: ObjectKey,
        part_bytes: usize,
    ) -> Self {
        Self {
            location: Path::from(key.as_str()),
            store,
            key,
            part_bytes: part_bytes.max(1),
            hasher: blake3::Hasher::new(),
            size: 0,
            pending: BytesMut::new(),
            multipart: None,
            finished: None,
        }
    }

    /// Uploads a local file, reading it one part at a time.
    pub(crate) async fn file(
        store: Arc<dyn ObjectStore>,
        key: ObjectKey,
        file: &FilePath,
    ) -> Result<StoredObject> {
        let mut upload = Self::new(store, key);

        let mut source = tokio::fs::File::open(file)
            .await
            .map_err(|error| Error::internal(format!("open {}: {error}", file.display())))?;

        loop {
            let mut chunk = BytesMut::with_capacity(upload.part_bytes);

            while chunk.len() < upload.part_bytes {
                let read = source.read_buf(&mut chunk).await.map_err(|error| {
                    Error::internal(format!("read {}: {error}", file.display()))
                })?;

                if read == 0 {
                    break;
                }
            }

            if chunk.is_empty() {
                break;
            }

            if let Err(error) = upload.write(chunk.freeze()).await {
                upload.abort().await;
                return Err(error);
            }
        }

        match upload.finish().await {
            Ok(object) => Ok(object),
            Err(error) => {
                upload.abort().await;
                Err(error)
            }
        }
    }

    pub(crate) async fn write(&mut self, bytes: Bytes) -> Result<()> {
        self.hasher.update(&bytes);
        self.size += bytes.len() as u64;

        if let Some(multipart) = self.multipart.as_mut() {
            multipart.put(bytes);
            return wait(multipart).await;
        }

        self.pending.extend_from_slice(&bytes);

        if self.pending.len() < self.part_bytes {
            return Ok(());
        }

        // The first part filled: confirm the object is absent, as a create-if-absent PUT would,
        // and continue as a multipart upload.
        match self.store.head(&self.location).await {
            Ok(_) => {
                return Err(Error::internal(format!(
                    "object {} already exists",
                    self.key
                )));
            }
            Err(OsError::NotFound { .. }) => {}
            Err(error) => return Err(upload_error(&self.key, error)),
        }

        let upload = self
            .store
            .put_multipart_opts(&self.location, PutMultipartOptions::default())
            .await
            .map_err(|error| upload_error(&self.key, error))?;

        let mut multipart = WriteMultipart::new_with_chunk_size(upload, self.part_bytes);
        multipart.put(std::mem::take(&mut self.pending).freeze());
        let waited = wait(&mut multipart).await;
        self.multipart = Some(multipart);
        waited
    }

    /// Completes the object and returns what the Catalog records about it.
    pub(crate) async fn finish(&mut self) -> Result<StoredObject> {
        if let Some(object) = &self.finished {
            return Ok(object.clone());
        }

        let e_tag = match self.multipart.take() {
            Some(multipart) => {
                multipart
                    .finish()
                    .await
                    .map_err(|error| upload_error(&self.key, error))?
                    .e_tag
            }
            None => {
                let payload = PutPayload::from(std::mem::take(&mut self.pending).freeze());

                self.store
                    .put_opts(
                        &self.location,
                        payload,
                        PutOptions {
                            mode: PutMode::Create,
                            ..PutOptions::default()
                        },
                    )
                    .await
                    .map_err(|error| upload_error(&self.key, error))?
                    .e_tag
            }
        };

        let object = StoredObject {
            key: self.key.clone(),
            size_bytes: self.size,
            checksum: ObjectChecksum::from_string(format!(
                "b3:{}",
                self.hasher.finalize().to_hex()
            )),
            etag: e_tag,
        };

        self.finished = Some(object.clone());
        Ok(object)
    }

    /// Drops the parts uploaded so far. A finished object is left in place: deleting it is the
    /// caller's decision.
    pub(crate) async fn abort(&mut self) {
        if let Some(multipart) = self.multipart.take()
            && let Err(error) = multipart.abort().await
        {
            tracing::warn!(object_key = %self.key, %error, "abort multipart upload failed");
        }
    }
}

async fn wait(multipart: &mut WriteMultipart) -> Result<()> {
    multipart
        .wait_for_capacity(PARTS_IN_FLIGHT)
        .await
        .map_err(|error| Error::internal(format!("upload part: {error}")))
}

fn upload_error(key: &ObjectKey, error: OsError) -> Error {
    Error::internal(format!("upload {key}: {error}"))
}

impl AsyncFileWriter for ArtifactUpload {
    fn write(&mut self, bytes: Bytes) -> BoxFuture<'_, parquet::errors::Result<()>> {
        async move {
            ArtifactUpload::write(self, bytes)
                .await
                .map_err(|error| ParquetError::External(Box::new(error)))
        }
        .boxed()
    }

    fn complete(&mut self) -> BoxFuture<'_, parquet::errors::Result<()>> {
        async move {
            self.finish()
                .await
                .map(|_| ())
                .map_err(|error| ParquetError::External(Box::new(error)))
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use object_store::memory::InMemory;

    use super::*;

    fn bytes(len: usize) -> Bytes {
        Bytes::from(
            (0..len)
                .map(|index| (index % 251) as u8)
                .collect::<Vec<_>>(),
        )
    }

    async fn upload(
        part_bytes: usize,
        pieces: &[usize],
    ) -> (Arc<dyn ObjectStore>, Bytes, StoredObject) {
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let mut upload = ArtifactUpload::with_part_bytes(
            store.clone(),
            ObjectKey::from_string("objects/a"),
            part_bytes,
        );
        let mut written = BytesMut::new();

        for len in pieces {
            let piece = bytes(*len);
            written.extend_from_slice(&piece);
            upload.write(piece).await.unwrap();
        }

        let object = upload.finish().await.unwrap();
        (store, written.freeze(), object)
    }

    #[tokio::test]
    async fn small_objects_are_one_put_and_large_ones_are_multipart() {
        for (part_bytes, pieces) in [(1024, vec![10, 20]), (64, vec![50, 50, 100, 7])] {
            let (store, written, object) = upload(part_bytes, &pieces).await;
            let stored = store
                .get(&Path::from("objects/a"))
                .await
                .unwrap()
                .bytes()
                .await
                .unwrap();

            assert_eq!(stored, written);
            assert_eq!(object.size_bytes, written.len() as u64);
            assert_eq!(
                object.checksum.0,
                format!("b3:{}", blake3::hash(&written).to_hex())
            );
        }
    }

    #[tokio::test]
    async fn an_existing_object_is_not_overwritten() {
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        store
            .put(&Path::from("objects/a"), Bytes::from_static(b"old").into())
            .await
            .unwrap();

        for part_bytes in [1024, 8] {
            let mut upload = ArtifactUpload::with_part_bytes(
                store.clone(),
                ObjectKey::from_string("objects/a"),
                part_bytes,
            );

            let written = upload.write(bytes(16)).await;
            let finished = match written {
                Ok(()) => upload.finish().await.map(|_| ()),
                Err(error) => Err(error),
            };

            assert!(finished.is_err(), "part size {part_bytes}");
        }

        let stored = store
            .get(&Path::from("objects/a"))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_eq!(stored.as_ref(), b"old");
    }

    #[tokio::test]
    async fn files_upload_in_parts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("archive");
        let content = bytes(3 * 1024 + 5);
        std::fs::write(&path, &content).unwrap();

        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let object =
            ArtifactUpload::file(store.clone(), ObjectKey::from_string("objects/f"), &path)
                .await
                .unwrap();

        let stored = store
            .get(&Path::from("objects/f"))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();

        assert_eq!(stored, content);
        assert_eq!(object.size_bytes, content.len() as u64);
    }
}
