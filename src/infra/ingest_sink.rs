// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `IngestSink` 的 infra 端实装入口。
//!
//! 当前提供 [`MemoryIngestSink`]：把整批事件 push 到内存 `parking_lot::Mutex<Vec>`，
//! 用于跑通装配链路与端到端测试，不做 WAL / parquet flush。
//!
//! 真正的"WAL + 内存 buffer + 周期 flush parquet"链路在下一个 change 接入：
//! 需要把 [`WalWriter`](crate::infra::wal::WalWriter) + Arrow buffer + [`ParquetWriter`]
//! 串起来，并由 ingester role 启动 flush 任务。

use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::{
    domain::ingestion::{IngestBatch, IngestResult, IngestSink},
    shared::Result,
};

#[derive(Default)]
pub struct MemoryIngestSink {
    pub batches: Arc<Mutex<Vec<IngestBatch>>>,
}

impl MemoryIngestSink {
    pub fn new() -> Self {
        Self {
            batches: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 取出当前缓存的批次（用于测试与 flush 任务）。
    pub fn drain(&self) -> Vec<IngestBatch> {
        std::mem::take(&mut *self.batches.lock())
    }
}

#[async_trait]
impl IngestSink for MemoryIngestSink {
    async fn write(&self, batch: IngestBatch) -> Result<IngestResult> {
        let accepted = batch.events.len();
        self.batches.lock().push(batch);
        Ok(IngestResult {
            accepted,
            rejected: 0,
            errors: Vec::new(),
        })
    }
}
