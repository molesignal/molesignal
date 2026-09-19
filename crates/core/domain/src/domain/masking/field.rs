// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 查询返回边界的字段遮掩策略与端口。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    domain::{
        metrics::PrometheusExemplarQueryResult,
        query::{QueryRequest, QueryResult},
        stream::StreamType,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldMaskingAlgorithm {
    Full {
        #[serde(default = "default_replacement")]
        replacement: String,
    },
    /// 遮掩字符区间 `[start, end)`。
    Range {
        start: usize,
        end: usize,
        #[serde(default = "default_replacement")]
        replacement: String,
    },
    /// 保留首尾字符，遮掩中间部分。
    Inner {
        prefix_chars: usize,
        suffix_chars: usize,
        #[serde(default = "default_replacement")]
        replacement: String,
    },
    /// 保留字符区间 `[start, end)`，遮掩区间外的内容。
    Outer {
        start: usize,
        end: usize,
        #[serde(default = "default_replacement")]
        replacement: String,
    },
    /// 组织隔离、确定性的 HMAC-SHA-256 十六进制摘要。
    Hash,
}

fn default_replacement() -> String {
    "******".into()
}

impl Default for FieldMaskingAlgorithm {
    fn default() -> Self {
        Self::Full {
            replacement: default_replacement(),
        }
    }
}

impl FieldMaskingAlgorithm {
    pub fn validate(&self) -> Result<()> {
        let (range, replacement) = match self {
            Self::Full { replacement } => (None, Some(replacement)),
            Self::Range {
                start,
                end,
                replacement,
            }
            | Self::Outer {
                start,
                end,
                replacement,
            } => (Some((*start, *end)), Some(replacement)),
            Self::Inner { replacement, .. } => (None, Some(replacement)),
            Self::Hash => (None, None),
        };
        if let Some((start, end)) = range
            && start >= end
        {
            return Err(crate::shared::Error::invalid(
                "masking range start must be less than end",
            ));
        }
        if replacement.is_some_and(|value| value.len() > 1024) {
            return Err(crate::shared::Error::invalid(
                "masking replacement must not exceed 1024 bytes",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldMaskingOverride {
    pub field: String,
    /// `None` 表示流级显式关闭遮掩；规则不存在表示继承全局配置。
    pub algorithm: Option<FieldMaskingAlgorithm>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldMaskingRule {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub priority: i32,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 字段名精确匹配或 `*`/`?` glob。
    pub field_pattern: String,
    #[serde(default)]
    pub stream_pattern: Option<String>,
    #[serde(default)]
    pub stream_type: Option<StreamType>,
    pub algorithm: FieldMaskingAlgorithm,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldMaskingSource {
    None,
    Global,
    Stream,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveFieldMaskingEntry {
    pub field: String,
    pub masked: bool,
    pub source: FieldMaskingSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub algorithm: Option<FieldMaskingAlgorithm>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_name: Option<String>,
    /// 无论流级是否覆盖，均返回首条命中的全局规则，供配置界面展示继承来源。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inherited_algorithm: Option<FieldMaskingAlgorithm>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inherited_rule_id: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inherited_rule_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveFieldMasking {
    pub stream_id: Id,
    pub fields: Vec<EffectiveFieldMaskingEntry>,
}

#[async_trait]
pub trait FieldMaskingRuleRepository: Send + Sync {
    async fn list(&self, org_id: &Id) -> Result<Vec<FieldMaskingRule>>;
    async fn create(&self, rule: FieldMaskingRule) -> Result<FieldMaskingRule>;
    async fn update(&self, rule: FieldMaskingRule) -> Result<FieldMaskingRule>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;
    async fn reorder(&self, org_id: &Id, ids: &[Id], now: TimestampMicros) -> Result<()>;
}

#[async_trait]
pub trait FieldMaskingProvider: Send + Sync {
    async fn effective_for_stream(
        &self,
        org_id: &Id,
        stream_id: &Id,
    ) -> Result<EffectiveFieldMasking>;

    async fn mask_result(&self, request: &QueryRequest, result: &mut QueryResult) -> Result<()>;

    async fn mask_exemplars(
        &self,
        request: &QueryRequest,
        result: &mut PrometheusExemplarQueryResult,
    ) -> Result<()>;
}
