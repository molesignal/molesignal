// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Catalog-native query projection.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{DatasetTypeId, ObjectChecksum, ObjectKey, StoredObject};
use crate::{
    domain::stream::{FieldPartitions, StreamType},
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
    /// 见 [`super::ColumnStats::value_sets`]。
    #[serde(default)]
    pub value_sets: serde_json::Map<String, serde_json::Value>,
    /// 见 [`super::ColumnStats::address_ranges`]。
    #[serde(default)]
    pub address_ranges: serde_json::Map<String, serde_json::Value>,
    /// 见 [`super::ColumnStats::null_counts`]。
    #[serde(default)]
    pub null_counts: serde_json::Map<String, serde_json::Value>,
    /// 见 [`super::ColumnStats::indexed_at_micros`]。
    #[serde(default)]
    pub indexed_at_micros: Option<i64>,
    /// 文件按字段分片写入时，它的分片方式与分片。
    #[serde(default)]
    pub shard: Option<FileShard>,
}

/// 按字段分片写入的文件：文件里每一行都落在 `shard` 这个分片里。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileShard {
    pub partitions: FieldPartitions,
    pub shard: u16,
}

impl FileShard {
    /// 文件里可能有 `field` 取 `values` 之一的行；`field` 不是分片字段时都可能。
    pub fn may_hold_any(&self, field: &str, values: &[String]) -> bool {
        self.partitions
            .component(self.shard, field)
            .is_none_or(|(partition, shard)| {
                values
                    .iter()
                    .any(|value| partition.shard_of(Some(value)) == shard)
            })
    }

    /// 文件里可能有 `field` 以 `prefix` 开头的行，`case_insensitive` 时不计大小写；`field` 不是
    /// 分片字段时都可能。
    pub fn may_hold_prefix(&self, field: &str, prefix: &str, case_insensitive: bool) -> bool {
        self.partitions
            .component(self.shard, field)
            .is_none_or(|(partition, shard)| {
                partition
                    .shards_of_prefix(prefix, case_insensitive)
                    .is_none_or(|shards| shards.contains(&shard))
            })
    }
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
