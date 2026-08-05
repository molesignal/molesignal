// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Pipeline runtime port + engine。
//!
//! 模型：
//! - [`FunctionExecutor`]：app 层 port，由 infra 实装（默认 `VrlFunctionExecutor`，包装
//!   [`crate::app::ingestion::pipeline`] 调用的 `infra::runtime::VrlRuntime`）。
//! - [`PipelineEngine::apply`]：拉 `(org, stream, stream_type)` 下 enabled pipelines；
//!   对每条 pipeline 的 steps 依次调 `executor.run_step(function, event_value)`。
//!
//! 失败策略：单步失败 → drop 该 event 进 `IngestResult.errors`，整 batch 不回滚；
//! 这与 schema 校验失败的处理一致。

use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    domain::{
        function::{Function, FunctionRepository},
        ingestion::{IngestBatch, IngestError},
        pipeline::PipelineRepository,
    },
    shared::Result,
};

/// Function 执行器抽象：由 infra 用 VRL/javascript runtime 实装。
#[async_trait]
pub trait FunctionExecutor: Send + Sync {
    /// 执行单个 function 对 event JSON 做就地变换。
    /// 解释器层面 compile 缓存由 impl 自己负责。
    async fn run(&self, function: &Function, event: &mut serde_json::Value) -> Result<()>;
}

/// 永远 noop 的执行器，dev / 无 vrl runtime 时占位。
pub struct NoopFunctionExecutor;

#[async_trait]
impl FunctionExecutor for NoopFunctionExecutor {
    async fn run(&self, _function: &Function, _event: &mut serde_json::Value) -> Result<()> {
        Ok(())
    }
}

pub struct PipelineEngine {
    pipelines: Arc<dyn PipelineRepository>,
    functions: Arc<dyn FunctionRepository>,
    executor: Arc<dyn FunctionExecutor>,
}

impl PipelineEngine {
    pub fn new(
        pipelines: Arc<dyn PipelineRepository>,
        functions: Arc<dyn FunctionRepository>,
        executor: Arc<dyn FunctionExecutor>,
    ) -> Self {
        Self {
            pipelines,
            functions,
            executor,
        }
    }

    /// 对 batch 内每条 event 跑该 (org, stream, stream_type) 上的全部 enabled pipeline。
    /// 返回单步失败的事件 index + reason；caller 决定是否把它们从 batch 剔除。
    #[tracing::instrument(
        name = "pipeline.ingest.apply",
        skip_all,
        fields(
            otel.kind = "internal",
            molesignal.pipeline.event_count = batch.events.len(),
            molesignal.stream.type = ?batch.stream_type
        )
    )]
    pub async fn apply(&self, batch: &mut IngestBatch) -> Result<Vec<IngestError>> {
        let pipelines = self
            .pipelines
            .list_for_stream(&batch.org_id, &batch.stream, batch.stream_type)
            .await?;
        if pipelines.is_empty() {
            return Ok(Vec::new());
        }
        // 把所有 step 引用到的 function 一次性 fetch（小规模 N 步，逐个调用）
        let mut errors: Vec<IngestError> = Vec::new();
        for (idx, ev) in batch.events.iter_mut().enumerate() {
            let mut value: serde_json::Value = serde_json::to_value(&ev.fields).unwrap_or_default();
            'this_event: for pipeline in &pipelines {
                for step in &pipeline.steps {
                    let f = match self.functions.get(&batch.org_id, &step.function_id).await {
                        Ok(f) => f,
                        Err(e) => {
                            errors.push(IngestError {
                                index: idx,
                                reason: format!("pipeline {} step missing fn: {e}", pipeline.name),
                            });
                            break 'this_event;
                        }
                    };
                    if let Err(e) = self.executor.run(&f, &mut value).await {
                        errors.push(IngestError {
                            index: idx,
                            reason: format!(
                                "pipeline {} step `{}` failed: {e}",
                                pipeline.name, f.name
                            ),
                        });
                        break 'this_event;
                    }
                }
            }
            // 把 value 转回 RawEvent.fields（fields 是 `serde_json::Map`，直接 swap）
            if let serde_json::Value::Object(obj) = value {
                ev.fields = obj;
            }
        }
        Ok(errors)
    }
}
