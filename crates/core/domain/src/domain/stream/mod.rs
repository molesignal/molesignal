// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 流上下文：StreamDefinition + Schema + Retention。
//!
//! 一个 "stream" 是同一来源/同一 schema 的数据流，是租户内最细粒度的数据划分单元，
//! 既是写入路径上的分桶单位，也是查询时的"表"。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    domain::{masking::FieldMaskingOverride, storage::StreamTypeId},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// MoleSignal 自身遥测使用的精确保留流名。仅该名字保留，其他 `_` 前缀不受影响。
pub const MOLESIGNAL_SYSTEM_STREAM: &str = "_molesignal";
/// 所有公共 continuous-profile 入口固定使用的 metadata stream。
pub const DEFAULT_PROFILE_STREAM: &str = "default";

pub fn is_reserved_system_stream(name: &str) -> bool {
    name == MOLESIGNAL_SYSTEM_STREAM
}

/// Validate the path-safe stream identifier shared by management and every intake protocol.
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

/// Public domain name retained for API readability; the underlying type is the open,
/// namespaced [`StreamTypeId`], not a closed enum.
pub type StreamType = StreamTypeId;

/// Receives committed stream mutations so runtime projections can invalidate stale state.
pub trait StreamMutationObserver: Send + Sync {
    fn stream_changed(&self, org_id: &Id, name: &str, stream_type: StreamType);
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
    /// 规范化后的字段索引类型。`None` 表示该 schema 来自旧版本，需要由
    /// [`Self::effective_index_type`] 从 `indexed/exact` 兼容推导；`Some(None)` 才表示
    /// API 明确关闭索引。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_type: Option<StreamIndexType>,
    /// 旧 schema/API 的兼容投影：新写入保持为 `index_type != none`。
    pub indexed: bool,
    /// 字段级静态加密：为 true 时该字段在写入 parquet 前用 `CipherRootKey` 加密，
    /// 列以密文（Utf8）落盘；查询端用 `decrypt(col)` UDF 还原明文。默认 false。
    #[serde(default)]
    pub encrypted: bool,
    /// 旧 schema/API 的兼容投影：新写入仅在 `index_type == exact` 时为 `true`。
    #[serde(default)]
    pub exact: bool,
}

impl FieldDef {
    /// 返回实际索引类型，并兼容尚未持久化 `index_type` 的历史 schema。
    ///
    /// 历史非文本 `indexed && !exact` 字段没有 Tantivy TEXT 索引，只生成 min/max 元数据，
    /// 因而兼容映射为 `skip`；历史文本字段保持原有 `full_text` 行为。
    pub fn effective_index_type(&self) -> StreamIndexType {
        match self.index_type {
            Some(index_type) => index_type,
            None if !self.indexed => StreamIndexType::None,
            None if self.exact => StreamIndexType::Exact,
            None if matches!(self.data_type, FieldType::Utf8 | FieldType::Json) => {
                StreamIndexType::FullText
            }
            None => StreamIndexType::Skip,
        }
    }

    /// 原子更新规范索引类型与两个旧兼容字段，避免三者出现互相矛盾的组合。
    pub fn configure_index(&mut self, enabled: bool, index_type: StreamIndexType) {
        let index_type = if enabled {
            index_type
        } else {
            StreamIndexType::None
        };
        self.index_type = Some(index_type);
        self.indexed = index_type != StreamIndexType::None;
        self.exact = index_type == StreamIndexType::Exact;
    }
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamIndexType {
    /// 不创建字段级辅助索引。
    #[default]
    None,
    /// 精确匹配：文本使用未分词倒排索引，并启用 Parquet Bloom。
    Exact,
    /// 全文匹配：文本使用分词倒排索引。
    FullText,
    /// 仅启用 Parquet Bloom filter。
    Bloom,
    /// 使用文件级 min/max zone map 做比较谓词裁剪。
    Skip,
}

impl StreamIndexType {
    /// 兼容只提交 `indexed=true`、未提交 `index_type` 的旧创建请求。
    pub fn legacy_default(data_type: FieldType) -> Self {
        if matches!(data_type, FieldType::Utf8 | FieldType::Json) {
            Self::FullText
        } else {
            Self::Skip
        }
    }
}

#[cfg(test)]
mod index_type_tests {
    use super::*;

    fn field(data_type: FieldType, indexed: bool, exact: bool) -> FieldDef {
        FieldDef {
            name: "value".into(),
            data_type,
            nullable: true,
            index_type: None,
            indexed,
            encrypted: false,
            exact,
        }
    }

    #[test]
    fn legacy_schema_maps_to_the_previous_effective_indexes() {
        assert_eq!(
            field(FieldType::Utf8, true, false).effective_index_type(),
            StreamIndexType::FullText
        );
        assert_eq!(
            field(FieldType::Int64, true, false).effective_index_type(),
            StreamIndexType::Skip
        );
        assert_eq!(
            field(FieldType::Utf8, true, true).effective_index_type(),
            StreamIndexType::Exact
        );
        assert_eq!(
            field(FieldType::Utf8, false, false).effective_index_type(),
            StreamIndexType::None
        );
    }

    #[test]
    fn explicit_index_type_is_not_collapsed_to_indexed_and_exact() {
        let mut value = field(FieldType::Utf8, false, false);
        value.configure_index(true, StreamIndexType::Bloom);
        assert_eq!(value.effective_index_type(), StreamIndexType::Bloom);
        assert!(value.indexed);
        assert!(!value.exact);

        value.configure_index(true, StreamIndexType::Skip);
        assert_eq!(value.effective_index_type(), StreamIndexType::Skip);

        value.configure_index(false, StreamIndexType::FullText);
        assert_eq!(value.effective_index_type(), StreamIndexType::None);
        assert!(!value.indexed);
    }

    #[test]
    fn serde_reads_legacy_schema_and_persists_the_explicit_type() {
        let mut value: FieldDef = serde_json::from_value(serde_json::json!({
            "name": "message",
            "data_type": "utf8",
            "nullable": true,
            "indexed": true,
            "encrypted": false,
            "exact": false
        }))
        .expect("legacy field schema");
        assert_eq!(value.index_type, None);
        assert_eq!(value.effective_index_type(), StreamIndexType::FullText);

        value.configure_index(true, StreamIndexType::Bloom);
        let serialized = serde_json::to_value(value).expect("serialize field schema");
        assert_eq!(serialized["index_type"], "bloom");
        assert_eq!(serialized["indexed"], true);
        assert_eq!(serialized["exact"], false);
    }
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
    /// 是否允许被查询。`false` 时该 stream 照常 intake 与保留，但查询/搜索端拒绝访问，
    /// 并从查询选择器中隐藏。用于「源 stream 仅作入口、数据经 pipeline 分流到下游
    /// stream」的场景：源 stream 不应被直接查询（避免重复计数 / 暴露未分流的原始数据）。
    /// 默认 `true`（保持既有 stream 可查询）。
    #[serde(default = "default_true")]
    pub queryable: bool,
    /// 流级字段遮掩覆盖；同字段存在条目时优先于全局规则。
    #[serde(default)]
    pub field_masking: Vec<FieldMaskingOverride>,
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
            field_masking: Vec::new(),
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
    /// 可信内部 intake 的 schema 演化入口。公共 API 不得调用。
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
