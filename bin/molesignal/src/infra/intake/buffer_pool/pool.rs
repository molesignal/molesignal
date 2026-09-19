// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use dashmap::{DashMap, mapref::entry::Entry};
use tokio::sync::Mutex;

use super::{
    BufferKey, BufferedRecordBatch, RecordBuilder,
    memory::{MemoryBudget, MemoryReservation},
};
use crate::{
    domain::{
        storage::PhysicalDatasetId,
        stream::{StreamDefinition, StreamType},
    },
    infra::intake::{dataset_resolver::ResolvedDataset, physical_schema},
    shared::{Error, Result, ids::Id},
};

/// Immutable dataset metadata plus the one mutex that orders WAL append/push against rotation.
pub struct DatasetBuffer {
    pub dataset: Arc<ResolvedDataset>,
    pub organization_id: Id,
    pub logical_stream_id: Id,
    pub stream_name: String,
    pub stream_type: StreamType,
    records: Mutex<RecordBuilder>,
}

impl DatasetBuffer {
    pub fn records(&self) -> &Mutex<RecordBuilder> {
        &self.records
    }

    pub async fn query_snapshot(&self) -> Result<Vec<BufferedRecordBatch>> {
        self.records
            .lock()
            .await
            .query_snapshot()
            .map_err(|error| Error::internal(format!("buffer query snapshot: {error}")))
    }
}

/// 跨 stream 的 buffer 池，同时拥有整个 intake 进程的内存预算。
pub struct BufferPool {
    buffers: DashMap<BufferKey, Arc<DatasetBuffer>>,
    memory: Arc<MemoryBudget>,
}

impl Default for BufferPool {
    fn default() -> Self {
        Self::new()
    }
}

impl BufferPool {
    /// 测试与兼容构造器；生产装配应使用 [`Self::with_memory_limit_bytes`]。
    pub fn new() -> Self {
        Self::with_memory_limit_bytes(usize::MAX)
    }

    pub fn with_memory_limit_bytes(max_bytes: usize) -> Self {
        Self {
            buffers: DashMap::new(),
            memory: MemoryBudget::new(max_bytes),
        }
    }

    pub fn try_reserve(&self, stream_type: StreamType, bytes: usize) -> Result<MemoryReservation> {
        self.memory.try_reserve(stream_type, bytes)
    }

    pub fn force_reserve(&self, bytes: usize) -> Result<MemoryReservation> {
        self.memory.force_reserve(bytes)
    }

    pub fn release_memory(&self, bytes: usize) {
        self.memory.release(bytes);
    }

    pub fn reserved_bytes(&self) -> usize {
        self.memory.reserved_bytes()
    }

    pub fn get_or_create_dataset(
        &self,
        stream: &StreamDefinition,
        dataset: Arc<ResolvedDataset>,
    ) -> Result<Arc<DatasetBuffer>> {
        if dataset.dataset.organization_id != stream.org_id
            || dataset.dataset.logical_stream_id != stream.id
        {
            return Err(Error::invalid(format!(
                "physical dataset {} does not belong to stream {}",
                dataset.dataset.id, stream.id
            )));
        }
        let key = dataset.dataset.id.clone();
        match self.buffers.entry(key.clone()) {
            Entry::Occupied(entry) => {
                if entry.get().logical_stream_id != stream.id {
                    return Err(Error::internal(format!(
                        "buffer dataset {} is already bound to another logical stream",
                        key
                    )));
                }
                Ok(entry.get().clone())
            }
            Entry::Vacant(entry) => {
                let physical_stream =
                    physical_schema::project(stream, &dataset.dataset.dataset_type);
                Ok(entry
                    .insert(Arc::new(DatasetBuffer {
                        dataset,
                        organization_id: stream.org_id.clone(),
                        logical_stream_id: stream.id.clone(),
                        stream_name: stream.name.clone(),
                        stream_type: stream.stream_type,
                        records: Mutex::new(RecordBuilder::new(&physical_stream)),
                    }))
                    .clone())
            }
        }
    }

    /// 列出当前所有 `(key, buffer)` 快照，供 flush scheduler 遍历。
    pub fn snapshot_keys(&self) -> Vec<BufferKey> {
        self.buffers
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    pub fn get(&self, key: &BufferKey) -> Option<Arc<DatasetBuffer>> {
        self.buffers.get(key).map(|value| value.clone())
    }

    pub async fn snapshot_dataset(
        &self,
        dataset_id: &PhysicalDatasetId,
    ) -> Result<Vec<BufferedRecordBatch>> {
        match self.get(dataset_id) {
            Some(buffer) => buffer.query_snapshot().await,
            None => Ok(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::stream::StreamType;

    #[test]
    fn reservation_is_atomic_and_drop_releases_capacity() {
        let pool = BufferPool::with_memory_limit_bytes(10);
        let first = pool.try_reserve(StreamType::LOGS, 7).unwrap();
        assert_eq!(pool.reserved_bytes(), 7);
        assert!(pool.try_reserve(StreamType::METRICS, 4).is_err());
        assert_eq!(pool.reserved_bytes(), 7);
        drop(first);
        assert_eq!(pool.reserved_bytes(), 0);
        assert!(pool.try_reserve(StreamType::METRICS, 10).is_ok());
    }

    #[test]
    fn committed_and_replay_reservations_need_explicit_release() {
        let pool = BufferPool::with_memory_limit_bytes(5);
        let accounted = pool.try_reserve(StreamType::LOGS, 5).unwrap().commit();
        assert_eq!(accounted, 5);
        assert_eq!(pool.reserved_bytes(), 5);

        let replayed = pool.force_reserve(3).unwrap().commit();
        assert_eq!(pool.reserved_bytes(), 8);
        pool.release_memory(accounted + replayed);
        assert_eq!(pool.reserved_bytes(), 0);
    }
}
