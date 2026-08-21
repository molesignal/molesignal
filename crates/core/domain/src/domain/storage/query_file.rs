// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Catalog-native query projection.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{DatasetTypeId, ObjectChecksum, ObjectKey, StoredObject};
use crate::{
    domain::stream::StreamType,
    shared::{Result, ids::Id, time::TimeRange},
};

/// Immutable primary Artifact projected for query planning.
///
/// This is deliberately read-only: writes, compaction and retention mutate [`super::FileCatalog`]
/// transactions instead of a parallel file-metadata table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryFile {
    pub id: Id,
    pub org_id: Id,
    pub stream: String,
    pub stream_type: StreamType,
    pub dataset_type: DatasetTypeId,
    /// Opaque Catalog object key. Callers must not infer identity or sibling artifacts from it.
    pub object_key: String,
    /// Catalog checksum used to bind range-cache entries to immutable object contents.
    #[serde(default)]
    pub checksum: Option<ObjectChecksum>,
    /// Origin version validator used for conditional range reads when available.
    #[serde(default)]
    pub etag: Option<String>,
    pub time_range: TimeRange,
    pub rows: u64,
    pub size_bytes: u64,
    pub min_values: serde_json::Map<String, serde_json::Value>,
    pub max_values: serde_json::Map<String, serde_json::Value>,
}

impl QueryFile {
    /// Reconstruct the immutable Catalog object identity when the projection carries a checksum.
    pub fn stored_object(&self) -> Option<StoredObject> {
        Some(StoredObject {
            key: ObjectKey::from_string(self.object_key.clone()),
            size_bytes: self.size_bytes,
            checksum: self.checksum.clone()?,
            etag: self.etag.clone(),
        })
    }
}

/// Read-only source for query projections. The normal implementation is a repeatable-read
/// FileCatalog snapshot merged with immutable manifests and live buffer generations.
#[async_trait]
pub trait QueryFileSource: Send + Sync {
    async fn find(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        time_range: TimeRange,
    ) -> Result<Vec<QueryFile>>;

    async fn find_dataset(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        dataset_type: DatasetTypeId,
        time_range: TimeRange,
    ) -> Result<Vec<QueryFile>> {
        Ok(self
            .find(org_id, stream, stream_type, time_range)
            .await?
            .into_iter()
            .filter(|file| file.dataset_type == dataset_type)
            .collect())
    }
}
