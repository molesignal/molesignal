// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Pipeline runtime port + engine。
//!
//! 模型：
//! - [`FunctionExecutor`]：app 层 port，由 infra 的 `ChainedFunctionExecutor` 实装，
//!   按函数语言分发到 VRL、JavaScript 或可选的 LLM runtime。
//! - [`PipelineEngine::apply`]：拉 `(org, stream, stream_type)` 下 enabled pipelines；
//!   对每条 pipeline 的 steps 依次调 `executor.run_step(function, event_value)`。
//!
//! 失败策略：单步失败 → drop 该 event 进 `IntakeResult.errors`，整 batch 不回滚；
//! 这与 schema 校验失败的处理一致。步骤有意丢弃的 event（VRL `abort`）同样离开 batch，但不是错误。

use std::sync::Arc;

use async_trait::async_trait;

pub use crate::domain::function::FunctionExecutor;
use crate::{
    domain::{
        function::{Function, FunctionOutcome, FunctionRepository},
        intake::{IntakeBatch, IntakeError},
        pipeline::PipelineRepository,
    },
    shared::Result,
};

/// 永远 noop 的执行器，dev / 无 vrl runtime 时占位。
pub struct NoopFunctionExecutor;

#[async_trait]
impl FunctionExecutor for NoopFunctionExecutor {
    async fn run(
        &self,
        _function: &Function,
        _event: &mut serde_json::Value,
    ) -> Result<FunctionOutcome> {
        Ok(FunctionOutcome::Kept)
    }
}

/// 一个 batch 过完 stream 上的 pipeline 后要离开 batch 的 event：步骤失败的（错误），和步骤
/// 有意丢弃的（不是错误）。两者都按 batch 内的 index 记。
#[derive(Debug, Default)]
pub struct PipelineApplied {
    pub errors: Vec<IntakeError>,
    pub dropped: Vec<usize>,
}

impl PipelineApplied {
    /// 把失败和被丢弃的 event 从 batch 里拿掉，`sizes`（与 batch 按 index 对齐）同步拿掉对应的项。
    pub fn remove_from(&self, batch: &mut IntakeBatch, sizes: &mut Vec<u64>) {
        let removed: std::collections::HashSet<usize> = self
            .errors
            .iter()
            .map(|error| error.index)
            .chain(self.dropped.iter().copied())
            .collect();

        if removed.is_empty() {
            return;
        }

        let mut index = 0;

        sizes.retain(|_| {
            let keep = !removed.contains(&index);
            index += 1;
            keep
        });

        let mut index = 0;

        batch.events.retain(|_| {
            let keep = !removed.contains(&index);
            index += 1;
            keep
        });
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
    /// 返回单步失败的事件 index + reason，以及被有意丢弃的事件 index；caller 把它们从 batch 剔除。
    #[tracing::instrument(
        name = "pipeline.intake.apply",
        skip_all,
        fields(
            otel.kind = "internal",
            molesignal.pipeline.event_count = batch.events.len(),
            molesignal.stream.type = ?batch.stream_type
        )
    )]
    pub async fn apply(&self, batch: &mut IntakeBatch) -> Result<PipelineApplied> {
        let pipelines = self
            .pipelines
            .list_for_stream(&batch.org_id, &batch.stream, batch.stream_type)
            .await?;

        if pipelines.is_empty() {
            return Ok(PipelineApplied::default());
        }

        // 把所有 step 引用到的 function 一次性 fetch（小规模 N 步，逐个调用）
        let mut errors: Vec<IntakeError> = Vec::new();
        let mut dropped: Vec<usize> = Vec::new();

        for (idx, ev) in batch.events.iter_mut().enumerate() {
            let mut value: serde_json::Value = serde_json::to_value(&ev.fields).unwrap_or_default();

            'this_event: for pipeline in &pipelines {
                for step in &pipeline.steps {
                    let f = match self.functions.get(&batch.org_id, &step.function_id).await {
                        Ok(f) => f,
                        Err(e) => {
                            errors.push(IntakeError {
                                index: idx,
                                reason: format!("pipeline {} step missing fn: {e}", pipeline.name),
                            });

                            break 'this_event;
                        }
                    };

                    match self.executor.run(&f, &mut value).await {
                        Ok(FunctionOutcome::Kept) => {}
                        Ok(FunctionOutcome::Dropped) => {
                            dropped.push(idx);
                            break 'this_event;
                        }
                        Err(e) => {
                            errors.push(IntakeError {
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
            }

            // 把 value 转回 RawEvent.fields（fields 是 `serde_json::Map`，直接 swap）
            if let serde_json::Value::Object(obj) = value {
                ev.fields = obj;
            }
        }

        Ok(PipelineApplied { errors, dropped })
    }
}
