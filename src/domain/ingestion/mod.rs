// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 采集上下文：原始事件、批次、写入结果。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    domain::{storage::PhysicalDatasetKind, stream::StreamType},
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// Stable ingestion identity used as the final keyset-pagination tie-breaker
/// for log-backed datasets. The trusted application layer overwrites any
/// client-provided value under this reserved field name.
pub const EVENT_ID_FIELD: &str = "_event_id";

/// 单条原始事件。日志/指标/trace 用相同的载体（区别在 `stream_type` 与字段约束）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawEvent {
    pub timestamp: TimestampMicros,
    pub fields: serde_json::Map<String, serde_json::Value>,
}

/// 一次写入提交的批次。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestBatch {
    pub batch_id: Id,
    pub org_id: Id,
    pub stream: String,
    pub stream_type: StreamType,
    pub events: Vec<RawEvent>,
    pub received_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestResult {
    pub accepted: usize,
    pub rejected: usize,
    pub errors: Vec<IngestError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestError {
    pub index: usize,
    pub reason: String,
}

/// 写入端口（端口-适配器中的"端口"）。
/// infra 层基于 WAL/buffer/parquet 实现。
#[async_trait]
pub trait IngestSink: Send + Sync {
    async fn write(&self, batch: IngestBatch) -> Result<IngestResult>;

    /// 生产 WAL/buffer sink 显式声明支持独立派生数据集。内存与测试 sink 默认关闭，
    /// 避免自动投影改变只验证原始批次的测试语义。
    fn supports_derived_datasets(&self) -> bool {
        false
    }

    /// 写入内部派生的物理数据集。外部采集入口只调用 [`Self::write`]，因此始终进入
    /// [`PhysicalDatasetKind::Raw`]；Trace/RUM/指标目录等应用服务可显式写独立摘要。
    async fn write_dataset(
        &self,
        dataset_kind: PhysicalDatasetKind,
        batch: IngestBatch,
    ) -> Result<IngestResult> {
        if dataset_kind != PhysicalDatasetKind::Raw {
            return Err(crate::shared::Error::invalid(format!(
                "ingest sink does not support physical dataset `{dataset_kind}`"
            )));
        }
        self.write(batch).await
    }
}

/// trace 观测端口：写入路径对 trace 批次旁路观察 span，派生服务间调用边。
///
/// 由 infra 实现（span 配对 + 分钟桶聚合），写入用例在 `stream_type == Traces`
/// 时旁路调用；为 None 时零开销。同步、不阻塞写入（纯内存累计）。
pub trait ServiceGraphObserver: Send + Sync {
    /// 观察一批 trace span 事件（字段约定见 OTLP/native trace 接入：`trace_id`、
    /// `span_id`、`parent_span_id`、`service.name`、`duration_ns`、`status_code`）。
    fn observe(&self, org_id: &Id, events: &[RawEvent]);
}

/// 写入前的领域服务：schema 推断、字段标准化、丢弃策略。
pub mod services {
    use super::*;

    /// 从批次中推断字段类型，用于 schema 演化。
    pub fn infer_schema_delta(
        _batch: &IngestBatch,
    ) -> Vec<(String, crate::domain::stream::FieldType)> {
        // 真实实现：遍历样本事件，按 json 类型映射到 FieldType
        Vec::new()
    }
}
