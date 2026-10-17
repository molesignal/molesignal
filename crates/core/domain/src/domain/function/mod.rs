// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Function bounded context：VRL / JS transform function.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionLanguage {
    Vrl,
    Js,
    /// LLM 评估节点：`source` 即指令/prompt，运行时把事件 JSON 交给模型评估，
    /// 结果写回事件字段。由 bootstrap 注入的执行器实装（需配置 AI provider）。
    Llm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub language: FunctionLanguage,
    pub source: String,
    pub params_schema: serde_json::Value,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// What a function did with the event it ran on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionOutcome {
    /// The event goes on, changed or not.
    Kept,
    /// The function dropped the event on purpose (VRL `abort`). It goes no further, and that is
    /// not a failure.
    Dropped,
}

/// Executes one function against an event. Runtime adapters implement this port.
#[async_trait]
pub trait FunctionExecutor: Send + Sync {
    async fn run(
        &self,
        function: &Function,
        event: &mut serde_json::Value,
    ) -> Result<FunctionOutcome>;

    /// [`Self::run`] with metadata the function reads beside the event (VRL `%name`) and that never
    /// becomes part of it. A runtime without metadata ignores it.
    async fn run_with_metadata(
        &self,
        function: &Function,
        event: &mut serde_json::Value,
        metadata: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<FunctionOutcome> {
        let _ = metadata;
        self.run(function, event).await
    }
}

#[async_trait]
pub trait FunctionRepository: Send + Sync {
    async fn create(&self, f: Function) -> Result<Function>;
    async fn update(&self, f: Function) -> Result<Function>;
    async fn get_by_id(&self, id: &Id) -> Result<Function>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<Function>;
    async fn list(&self, org_id: &Id) -> Result<Vec<Function>>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;
}
