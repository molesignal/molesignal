// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 流上下文：StreamDefinition + Schema + Retention。
//!
//! 一个 "stream" 是同一来源/同一 schema 的数据流，是租户内最细粒度的数据划分单元，
//! 既是写入路径上的分桶单位，也是查询时的"表"。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

/// MoleSignal 自身遥测使用的精确保留流名。仅该名字保留，其他 `_` 前缀不受影响。
pub const MOLESIGNAL_SYSTEM_STREAM: &str = "_molesignal";
/// 所有公共 continuous-profile 入口固定使用的 metadata stream。
pub const DEFAULT_PROFILE_STREAM: &str = "default";

pub fn is_reserved_system_stream(name: &str) -> bool {
    name == MOLESIGNAL_SYSTEM_STREAM
}

/// Validate the path-safe stream identifier shared by management and every ingest protocol.
pub fn validate_stream_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 255 {
        return Err(Error::invalid("stream name must be 1..255 characters"));
    }
    if matches!(name, "." | "..") {
        return Err(Error::invalid("stream name must not be a path segment"));
    }
    if !name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
    {
        return Err(Error::invalid(
            "stream name may only contain letters, numbers, '_', '-' and '.'",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamType {
    Logs,
    Metrics,
    Traces,
    /// Continuous Profiling（持续性能分析）：一组带权调用栈样本。
    /// 归一化后原始 pprof 旁路 zstd 归档到 object store，元数据行进本流；
    /// 不作为 pipeline target（栈语义不适合通用 transform）。
    Profiles,
    /// extend table（静态 KV）。
    /// 不参与 parquet 落盘 + 不能作为 pipeline target；
    /// ingester 把行内容直接 fan-out 到 [`crate::domain::stream`] 外的内存表（infra: `ExtendTable`）。
    Extend,
}

impl StreamType {
    pub const fn as_str(self) -> &'static str {
        match self {
            StreamType::Logs => "logs",
            StreamType::Metrics => "metrics",
            StreamType::Traces => "traces",
            StreamType::Profiles => "profiles",
            StreamType::Extend => "extend",
        }
    }

    /// extend stream 禁止作为 pipeline target。
    pub fn allowed_as_pipeline_target(self) -> bool {
        !matches!(self, StreamType::Extend | StreamType::Profiles)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamDefinition {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub stream_type: StreamType,
    pub schema: Schema,
    pub retention: Option<Retention>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schema {
    pub fields: Vec<FieldDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDef {
    pub name: String,
    pub data_type: FieldType,
    pub nullable: bool,
    pub indexed: bool,
    /// 字段级静态加密：为 true 时该字段在写入 parquet 前用 `CipherRootKey` 加密，
    /// 列以密文（Utf8）落盘；查询端用 `decrypt(col)` UDF 还原明文。默认 false。
    #[serde(default)]
    pub encrypted: bool,
    /// 精确索引：`indexed && exact` 时该字段建**未分词**的 tantivy `STRING` 索引（整值一个
    /// term），供 `col = 'literal'` 等值裁剪；`indexed && !exact` 走分词 `TEXT` 索引供
    /// `MATCH()` 全文。二者不可兼得（一个 tantivy 字段只能其一）。`exact` 无 `indexed`
    /// 无意义（不建任何索引）。默认 false，存量 schema 反序列化即保持既有 TEXT 行为。
    #[serde(default)]
    pub exact: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Bool,
    Int64,
    Float64,
    Utf8,
    Timestamp,
    Json,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Retention {
    pub days: u32,
}

impl StreamDefinition {
    pub fn effective_retention_days(&self, fallback_days: u32) -> u32 {
        self.retention
            .map(|retention| retention.days)
            .filter(|days| *days > 0)
            .unwrap_or_else(|| fallback_days.max(1))
    }
}

#[cfg(test)]
mod stream_name_tests {
    use super::validate_stream_name;

    #[test]
    fn stream_name_accepts_every_supported_path_safe_separator() {
        for name in ["app_logs", "checkout-api", "service.v2", "_molesignal"] {
            validate_stream_name(name).expect(name);
        }
    }

    #[test]
    fn stream_name_rejects_path_traversal_and_empty_values() {
        for name in ["", ".", "..", "../secrets", "a/b", "a\\b", "white space"] {
            assert!(validate_stream_name(name).is_err(), "{name}");
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamIndexType {
    #[default]
    None,
    Exact,
    FullText,
    Bloom,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldIndexRule {
    pub field: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub index_type: StreamIndexType,
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub sdr_patterns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamCondition {
    pub name: String,
    pub expression: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Optional retention override for records matching this condition.
    /// `None` keeps the stream's default retention.
    #[serde(default)]
    pub retention_days: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamSettings {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub index_rules: Vec<FieldIndexRule>,
    #[serde(default)]
    pub retention_filter: Option<String>,
    #[serde(default)]
    pub keep_conditions: Vec<StreamCondition>,
    #[serde(default)]
    pub max_query_range_hours: Option<u32>,
    #[serde(default)]
    pub flatten_level: Option<u8>,
    #[serde(default)]
    pub use_stream_stats_for_partitioning: bool,
    #[serde(default)]
    pub store_original_data: bool,
    #[serde(default = "default_true")]
    pub enable_distinct_values: bool,
    /// 是否允许被查询。`false` 时该 stream 照常 ingest 与保留，但查询/搜索端拒绝访问，
    /// 并从查询选择器中隐藏。用于「源 stream 仅作入口、数据经 pipeline 分流到下游
    /// stream」的场景：源 stream 不应被直接查询（避免重复计数 / 暴露未分流的原始数据）。
    /// 默认 `true`（保持既有 stream 可查询）。
    #[serde(default = "default_true")]
    pub queryable: bool,
}

impl Default for StreamSettings {
    fn default() -> Self {
        Self {
            description: None,
            index_rules: Vec::new(),
            retention_filter: None,
            keep_conditions: Vec::new(),
            max_query_range_hours: None,
            flatten_level: None,
            use_stream_stats_for_partitioning: false,
            store_original_data: false,
            enable_distinct_values: true,
            queryable: true,
        }
    }
}

fn default_true() -> bool {
    true
}

#[async_trait]
pub trait StreamRepository: Send + Sync {
    async fn create(&self, def: StreamDefinition) -> Result<StreamDefinition>;
    async fn update_schema(&self, id: &Id, schema: Schema) -> Result<()>;
    /// 可信内部 ingest 的 schema 演化入口。公共 API 不得调用。
    async fn update_schema_internal(&self, id: &Id, schema: Schema) -> Result<()> {
        self.update_schema(id, schema).await
    }
    async fn update_retention(&self, _id: &Id, _retention: Option<Retention>) -> Result<()> {
        Err(Error::internal(
            "stream repository does not support retention updates",
        ))
    }
    async fn get(
        &self,
        org_id: &Id,
        name: &str,
        stream_type: StreamType,
    ) -> Result<StreamDefinition>;
    async fn get_by_id(&self, _id: &Id) -> Result<StreamDefinition> {
        Err(Error::internal(
            "stream repository does not support get_by_id",
        ))
    }
    async fn list(&self, org_id: &Id) -> Result<Vec<StreamDefinition>>;
    async fn get_settings(&self, _id: &Id) -> Result<StreamSettings> {
        Ok(StreamSettings::default())
    }
    async fn update_settings(&self, _id: &Id, _settings: StreamSettings) -> Result<StreamSettings> {
        Err(Error::internal(
            "stream repository does not support settings updates",
        ))
    }
    async fn delete(&self, id: &Id) -> Result<()>;
}
