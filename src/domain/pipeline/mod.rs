// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Pipeline bounded context：ingest-time 多步 function 链。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    domain::stream::StreamType,
    shared::{Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStep {
    pub function_id: Id,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    /// 绑定的目标 stream（org_id + stream_name + stream_type 的 hash，作为 enabled 唯一约束）
    pub stream_name: String,
    pub stream_type: StreamType,
    pub steps: Vec<PipelineStep>,
    pub enabled: bool,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait PipelineRepository: Send + Sync {
    async fn create(&self, p: Pipeline) -> Result<Pipeline>;
    async fn update(&self, p: Pipeline) -> Result<Pipeline>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<Pipeline>;
    async fn list(&self, org_id: &Id) -> Result<Vec<Pipeline>>;
    async fn list_for_stream(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
    ) -> Result<Vec<Pipeline>>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;
}
