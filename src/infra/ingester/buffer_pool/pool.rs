// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::Mutex;

use super::{
    BufferKey, RecordBuilder,
    memory::{MemoryBudget, MemoryReservation},
};
use crate::{
    domain::{
        storage::PhysicalDatasetKind,
        stream::{StreamDefinition, StreamType},
    },
    infra::ingester::physical_schema,
    shared::Result,
};

/// 跨 stream 的 buffer 池，同时拥有整个 ingester 进程的内存预算。
pub struct BufferPool {
    buffers: DashMap<BufferKey, Arc<Mutex<RecordBuilder>>>,
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

    pub fn get_or_create(&self, stream: &StreamDefinition) -> Arc<Mutex<RecordBuilder>> {
        self.get_or_create_dataset(stream, PhysicalDatasetKind::Raw)
    }

    pub fn get_or_create_dataset(
        &self,
        stream: &StreamDefinition,
        dataset_kind: PhysicalDatasetKind,
    ) -> Arc<Mutex<RecordBuilder>> {
        let key: BufferKey = (
            stream.org_id.clone(),
            stream.stream_type,
            stream.name.clone(),
            dataset_kind,
        );
        if let Some(buffer) = self.buffers.get(&key) {
            return buffer.clone();
        }
        let physical_stream = physical_schema::project(stream, dataset_kind);
        let buffer = Arc::new(Mutex::new(RecordBuilder::new(&physical_stream)));
        self.buffers.insert(key, buffer.clone());
        buffer
    }

    /// 列出当前所有 `(key, buffer)` 快照，供 flush scheduler 遍历。
    pub fn snapshot_keys(&self) -> Vec<BufferKey> {
        self.buffers
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    pub fn get(&self, key: &BufferKey) -> Option<Arc<Mutex<RecordBuilder>>> {
        self.buffers.get(key).map(|value| value.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::stream::StreamType;

    #[test]
    fn reservation_is_atomic_and_drop_releases_capacity() {
        let pool = BufferPool::with_memory_limit_bytes(10);
        let first = pool.try_reserve(StreamType::Logs, 7).unwrap();
        assert_eq!(pool.reserved_bytes(), 7);
        assert!(pool.try_reserve(StreamType::Metrics, 4).is_err());
        assert_eq!(pool.reserved_bytes(), 7);
        drop(first);
        assert_eq!(pool.reserved_bytes(), 0);
        assert!(pool.try_reserve(StreamType::Metrics, 10).is_ok());
    }

    #[test]
    fn committed_and_replay_reservations_need_explicit_release() {
        let pool = BufferPool::with_memory_limit_bytes(5);
        let accounted = pool.try_reserve(StreamType::Logs, 5).unwrap().commit();
        assert_eq!(accounted, 5);
        assert_eq!(pool.reserved_bytes(), 5);

        let replayed = pool.force_reserve(3).unwrap().commit();
        assert_eq!(pool.reserved_bytes(), 8);
        pool.release_memory(accounted + replayed);
        assert_eq!(pool.reserved_bytes(), 0);
    }
}
