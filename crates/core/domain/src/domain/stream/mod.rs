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
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

mod address;
mod partition;

pub use address::{address_key, address_number, cidr_range, parse_address_key};
pub use partition::{FieldPartition, FieldPartitions, PartitionScheme};

/// MoleSignal 自身遥测使用的精确保留流名。仅该名字保留，其他 `_` 前缀不受影响。
pub const MOLESIGNAL_SYSTEM_STREAM: &str = "_molesignal";

/// 所有公共 continuous-profile 入口固定使用的 metadata stream。
pub const DEFAULT_PROFILE_STREAM: &str = "default";

pub fn is_reserved_system_stream(name: &str) -> bool {
    name == MOLESIGNAL_SYSTEM_STREAM
}

/// 组织的审计记录：每条记下的审计事件也写进这个流。读它要有 `audit.read`。
pub const AUDIT_STREAM: &str = "_audit";

/// 只由平台写的流：平台自身遥测（平台组织的 `_molesignal`），以及每个组织的拨测运行结果、审计记录、
/// 智能体的模型调用与评分。它们和别的流一样可读（`_audit` 要 `audit.read`），但任何写入协议、任何
/// 流水线都写不进去。
pub const PLATFORM_STREAMS: &[&str] = &[
    MOLESIGNAL_SYSTEM_STREAM,
    "_synthetic_results",
    AUDIT_STREAM,
    "_agent_model_traces",
    "_agent_score_traces",
];

pub fn is_platform_stream(name: &str) -> bool {
    PLATFORM_STREAMS.contains(&name)
}

/// 写入协议和流水线写流之前的检查：平台的流（[`is_platform_stream`]）不让写。
pub fn refuse_platform_stream(name: &str) -> Result<()> {
    if is_platform_stream(name) {
        return Err(Error::forbidden(format!(
            "`{name}` is written by the platform only"
        )));
    }

    Ok(())
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
        return Err(Error::invalid(format!(
            "stream name `{name}` may only contain letters, numbers, '_', '-' and '.'"
        )));
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
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
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
    /// Parquet Bloom filter，另有文件级过滤器：`=` / `IN` 在打开文件前就能跳过它（文本字段）。
    Bloom,
    /// 文件级 min/max zone map 做比较谓词裁剪；值不多时另记值集合。
    Skip,
    /// 文件级三元组过滤器：`LIKE` / `ILIKE` / `MATCH` 的子串（文本字段）。
    Ngram,
    /// 文件级地址范围：`cidr_match` 网段与 `=` 地址（文本字段里的 IP 地址）。
    Ip,
    /// JSON 字段第一层的键与值：`json_text(字段, 'key') = '值'`、`IN` 与 `IS NOT NULL`（JSON
    /// 字段）。
    KeyValue,
}

impl StreamIndexType {
    /// `data_type` 的字段能建这种索引：`full_text`、`ngram`、`ip` 只用于文本字段，`key_value` 只
    /// 用于 JSON 字段。
    pub fn suits(self, data_type: FieldType) -> bool {
        match self {
            Self::FullText | Self::Ngram | Self::Ip => data_type == FieldType::Utf8,
            Self::KeyValue => data_type == FieldType::Json,
            Self::None | Self::Exact | Self::Bloom | Self::Skip => true,
        }
    }

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
            "dataType": "utf8",
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
        assert_eq!(serialized["indexType"], "bloom");
        assert_eq!(serialized["indexed"], true);
        assert_eq!(serialized["exact"], false);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldIndexRule {
    pub field: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub index_type: StreamIndexType,
}

/// 新字段按名字定索引：写入时第一次出现的字段，名字匹配 `pattern`、类型合适时建 `index_type`。
/// `pattern` 里 `*` 是任意一段文字（可以为空），`?` 是任意一个字符，别的字符按原样比较。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexPattern {
    pub pattern: String,
    pub index_type: StreamIndexType,
}

impl IndexPattern {
    pub fn matches(&self, field: &str) -> bool {
        glob_matches(&self.pattern, field)
    }
}

/// `pattern` 匹配整个 `text`：`*` 是任意一段文字（可以为空），`?` 是任意一个字符。
fn glob_matches(pattern: &str, text: &str) -> bool {
    let pattern = pattern.chars().collect::<Vec<_>>();
    let text = text.chars().collect::<Vec<_>>();
    let (mut at, mut read) = (0, 0);

    // 最近一个 `*` 的位置，与它当时开始匹配的文字位置：后面对不上时让它多吞一个字符再试。
    let mut star = None;

    while read < text.len() {
        match pattern.get(at) {
            Some('*') => {
                star = Some((at, read));
                at += 1;
            }
            Some(&character) if character == '?' || character == text[read] => {
                at += 1;
                read += 1;
            }
            _ => {
                let Some((star_at, star_read)) = star else {
                    return false;
                };

                star = Some((star_at, star_read + 1));
                at = star_at + 1;
                read = star_read + 1;
            }
        }
    }

    pattern[at..].iter().all(|character| *character == '*')
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamSettings {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub index_rules: Vec<FieldIndexRule>,
    /// 新字段按名字定索引，按次序第一条匹配的生效，见 [`Self::new_field_index`]。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub index_patterns: Vec<IndexPattern>,
    /// 一次查询最多覆盖多少小时：时间范围更长的查询被拒绝，免得一次误选扫遍几十天的数据。
    /// `None` 不限。
    #[serde(default)]
    pub max_query_range_hours: Option<u32>,
    /// 是否允许被查询。`false` 时该 stream 照常 intake 与保留，但查询/搜索端拒绝访问，
    /// 并从查询选择器中隐藏。用于「源 stream 仅作入口、数据经 pipeline 分流到下游
    /// stream」的场景：源 stream 不应被直接查询（避免重复计数 / 暴露未分流的原始数据）。
    /// 默认 `true`（保持既有 stream 可查询）。
    #[serde(default = "default_true")]
    pub queryable: bool,
    /// 流级字段遮掩覆盖；同字段存在条目时优先于全局规则。
    #[serde(default)]
    pub field_masking: Vec<FieldMaskingOverride>,
    /// 日志与链路：一天结束一小时后，这一天的文件加起来不超过合并目标大小时合成一个文件，
    /// 跨多天的查询要打开的文件少得多。保留期随之按天生效：这样的文件在一整天都过期后才删除。
    /// 默认开启。
    #[serde(default = "default_true")]
    pub daily_merge: bool,
    /// 指标：早于若干天的样本按时间桶聚合，见 [`Downsample`]。默认不降采样。
    #[serde(default)]
    pub downsample: Downsample,
    /// 日志与链路：按一两个字段的值把新写入的数据分进几个分片，见 [`FieldPartitions`]。默认
    /// 不分。
    #[serde(default, skip_serializing_if = "FieldPartitions::is_empty")]
    pub partitions: FieldPartitions,
    /// 最近一次改已有字段索引的时刻（微秒），由服务端在保存设置时记下。在这之前建索引的段（见
    /// [`crate::domain::storage::ColumnStats::indexed_at_micros`]）由后台按新设置重建。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_changed_at: Option<i64>,
}

impl Default for StreamSettings {
    fn default() -> Self {
        Self {
            description: None,
            index_rules: Vec::new(),
            index_patterns: Vec::new(),
            max_query_range_hours: None,
            queryable: true,
            field_masking: Vec::new(),
            daily_merge: true,
            downsample: Downsample::default(),
            partitions: FieldPartitions::default(),
            index_changed_at: None,
        }
    }
}

impl StreamSettings {
    /// 写入时第一次出现的字段 `name`（类型 `data_type`）建什么索引：同名的字段规则，其次是第一条
    /// 名字匹配、类型合适的 [`IndexPattern`]。都没有时为 `None`，由数据流类型的默认规则决定。
    pub fn new_field_index(&self, name: &str, data_type: FieldType) -> Option<StreamIndexType> {
        if let Some(rule) = self.index_rules.iter().find(|rule| rule.field == name) {
            return match rule.enabled {
                true => Some(rule.index_type).filter(|index_type| index_type.suits(data_type)),
                false => Some(StreamIndexType::None),
            };
        }

        self.index_patterns
            .iter()
            .find(|pattern| pattern.matches(name) && pattern.index_type.suits(data_type))
            .map(|pattern| pattern.index_type)
    }

    /// `range` 超过 [`Self::max_query_range_hours`] 时返回这个上限。
    pub fn exceeded_query_range(&self, range: TimeRange) -> Option<u32> {
        let hours = self.max_query_range_hours.filter(|hours| *hours > 0)?;
        let span = range.end.0.saturating_sub(range.start.0);

        (span > i64::from(hours).saturating_mul(3_600_000_000)).then_some(hours)
    }
}

/// 指标流的降采样：早于 `after_days` 天的原始样本按 `interval_secs` 秒的时间桶聚合进 rollup，
/// 原始样本随之删除，长时间范围的查询要读的样本少得多。`after_days` 为 0 时不降采样。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Downsample {
    #[serde(default)]
    pub after_days: u32,
    #[serde(default = "default_downsample_interval_secs")]
    pub interval_secs: u32,
}

/// 降采样默认的时间桶：一小时。
pub const DEFAULT_DOWNSAMPLE_INTERVAL_SECS: u32 = 3_600;

fn default_downsample_interval_secs() -> u32 {
    DEFAULT_DOWNSAMPLE_INTERVAL_SECS
}

impl Default for Downsample {
    fn default() -> Self {
        Self {
            after_days: 0,
            interval_secs: DEFAULT_DOWNSAMPLE_INTERVAL_SECS,
        }
    }
}

impl Downsample {
    pub fn enabled(&self) -> bool {
        self.after_days > 0 && self.interval_secs > 0
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    #[test]
    fn index_patterns_match_whole_names_with_wildcards() {
        let pattern = |pattern: &str| IndexPattern {
            pattern: pattern.into(),
            index_type: StreamIndexType::Exact,
        };

        for (pattern_text, name, expected) in [
            ("*_id", "user_id", true),
            ("*_id", "_id", true),
            ("*_id", "id", false),
            ("*_id", "user_id.raw", false),
            ("http.*", "http.method", true),
            ("http.*", "http", false),
            ("?d", "id", true),
            ("?d", "uid", false),
            ("a*b*c", "aXbYc", true),
            ("a*b*c", "acb", false),
            ("*", "anything", true),
            ("level", "level", true),
            ("level", "Level", false),
        ] {
            assert_eq!(
                pattern(pattern_text).matches(name),
                expected,
                "{pattern_text} ~ {name}"
            );
        }
    }

    /// 同名的字段规则先于模式；模式按次序，第一条名字匹配、类型合适的生效。
    #[test]
    fn a_new_field_takes_its_rule_then_the_first_pattern_that_suits_it() {
        let settings = StreamSettings {
            index_rules: vec![
                FieldIndexRule {
                    field: "session_ip".into(),
                    enabled: false,
                    index_type: StreamIndexType::Ip,
                },
                FieldIndexRule {
                    field: "order_id".into(),
                    enabled: true,
                    index_type: StreamIndexType::Bloom,
                },
            ],
            index_patterns: vec![
                IndexPattern {
                    pattern: "*_ip".into(),
                    index_type: StreamIndexType::Ip,
                },
                IndexPattern {
                    pattern: "*_i?".into(),
                    index_type: StreamIndexType::Exact,
                },
            ],
            ..StreamSettings::default()
        };

        assert_eq!(
            settings.new_field_index("client_ip", FieldType::Utf8),
            Some(StreamIndexType::Ip)
        );
        assert_eq!(
            settings.new_field_index("count_ip", FieldType::Int64),
            Some(StreamIndexType::Exact),
            "ip does not suit an integer, the next pattern does"
        );
        assert_eq!(
            settings.new_field_index("session_ip", FieldType::Utf8),
            Some(StreamIndexType::None)
        );
        assert_eq!(
            settings.new_field_index("order_id", FieldType::Utf8),
            Some(StreamIndexType::Bloom)
        );
        assert_eq!(settings.new_field_index("message", FieldType::Utf8), None);
    }

    #[test]
    fn a_query_range_longer_than_the_stream_allows_is_reported() {
        let day = 24 * 3_600_000_000_i64;
        let range = |end| TimeRange::new(TimestampMicros(0), TimestampMicros(end));

        let settings = StreamSettings {
            max_query_range_hours: Some(24),
            ..StreamSettings::default()
        };

        assert_eq!(settings.exceeded_query_range(range(day)), None);
        assert_eq!(settings.exceeded_query_range(range(day + 1)), Some(24));
        assert_eq!(
            StreamSettings::default().exceeded_query_range(range(i64::MAX)),
            None
        );
    }

    /// Settings saved with keys that are gone still read, with the new ones at their defaults.
    #[test]
    fn settings_saved_with_removed_keys_still_read() {
        let settings: StreamSettings = serde_json::from_value(serde_json::json!({
            "flattenLevel": 3,
            "keepConditions": [{ "name": "errors", "expression": "level = 'error'" }],
            "queryable": false,
        }))
        .expect("read settings");

        assert!(!settings.queryable);
        assert!(settings.daily_merge);
        assert_eq!(settings.downsample, Downsample::default());
        assert!(!settings.downsample.enabled());
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
