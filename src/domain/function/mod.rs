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

#[async_trait]
pub trait FunctionRepository: Send + Sync {
    async fn create(&self, f: Function) -> Result<Function>;
    async fn update(&self, f: Function) -> Result<Function>;
    async fn get_by_id(&self, id: &Id) -> Result<Function>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<Function>;
    async fn list(&self, org_id: &Id) -> Result<Vec<Function>>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;
}
