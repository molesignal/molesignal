// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 开放类型 ID：signal、dataset、artifact、index、WAL codec。
//!
//! 这些维度会随功能演进新增取值，因此不用 Rust enum 或 PostgreSQL ENUM，
//! 而是经过校验的字符串 Newtype + 运行期注册（见 [`super::registry`]）。
//! 会改变系统正确性的状态机（Segment / Artifact 状态等）仍然是封闭 enum。
//!
//! 命名约定：`builtin.` 前缀留给内置类型，如 `builtin.logs`、`builtin.parquet`、
//! `builtin.metrics.rollup`。ID 一旦发布不可改名；格式版本单独管理。

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::shared::{Error, Result};

/// 校验开放类型 ID：小写 ASCII，仅 `a-z 0-9 . - _`，`.` 分段且段非空，
/// 段首必须是字母，总长 3..=96。
fn validate_type_id(value: &str) -> Result<()> {
    if value.len() < 3 || value.len() > 96 {
        return Err(Error::invalid(format!(
            "type id `{value}` length must be within 3..=96"
        )));
    }
    for segment in value.split('.') {
        if segment.is_empty() {
            return Err(Error::invalid(format!(
                "type id `{value}` has an empty dot-separated segment"
            )));
        }
        let mut chars = segment.chars();
        let head = chars.next().expect("segment is non-empty");
        if !head.is_ascii_lowercase() {
            return Err(Error::invalid(format!(
                "type id `{value}` segment `{segment}` must start with a-z"
            )));
        }
        if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') {
            return Err(Error::invalid(format!(
                "type id `{value}` segment `{segment}` may only contain a-z 0-9 - _"
            )));
        }
    }
    Ok(())
}

macro_rules! type_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self> {
                let value = value.into();
                validate_type_id(&value)?;
                Ok(Self(value))
            }

            /// 内置常量入口：编译期字面量，格式错误属于编程错误，直接 panic。
            pub fn builtin(value: &'static str) -> Self {
                Self::new(value).expect("builtin type id must be valid")
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl std::str::FromStr for $name {
            type Err = Error;

            fn from_str(value: &str) -> Result<Self> {
                Self::new(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

/// 逻辑流的开放信号类型（logs / metrics / traces / …）。
///
/// 该 ID 在请求、缓存键和运行时热路径中大量按值传递，因此以内联定长字节保存；
/// 仍然遵守与其它开放类型相同的 96 字节上限，不是封闭枚举。
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StreamTypeId {
    len: u8,
    bytes: [u8; 96],
}

impl StreamTypeId {
    pub const LOGS: Self = Self::builtin(builtin::STREAM_LOGS);
    pub const METRICS: Self = Self::builtin(builtin::STREAM_METRICS);
    pub const TRACES: Self = Self::builtin(builtin::STREAM_TRACES);
    pub const PROFILES: Self = Self::builtin(builtin::STREAM_PROFILES);
    pub const EXTEND: Self = Self::builtin(builtin::STREAM_EXTEND);

    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_type_id(&value)?;
        Ok(Self::copy_from_str(&value))
    }

    pub const fn builtin(value: &'static str) -> Self {
        assert!(value.len() <= 96, "builtin type id exceeds 96 bytes");
        let source = value.as_bytes();
        let mut bytes = [0; 96];
        let mut index = 0;
        while index < source.len() {
            bytes[index] = source[index];
            index += 1;
        }
        Self {
            len: source.len() as u8,
            bytes,
        }
    }

    fn copy_from_str(value: &str) -> Self {
        let mut bytes = [0; 96];
        bytes[..value.len()].copy_from_slice(value.as_bytes());
        Self {
            len: value.len() as u8,
            bytes,
        }
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..usize::from(self.len)])
            .expect("validated stream type id is UTF-8")
    }

    /// Stable public/protocol slug for built-ins; extension IDs remain namespaced.
    pub fn external_name(&self) -> &str {
        match self.as_str() {
            builtin::STREAM_LOGS => "logs",
            builtin::STREAM_METRICS => "metrics",
            builtin::STREAM_TRACES => "traces",
            builtin::STREAM_PROFILES => "profiles",
            builtin::STREAM_EXTEND => "extend",
            other => other,
        }
    }

    pub fn from_external_name(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        Ok(match value.as_str() {
            "logs" => Self::LOGS,
            "metrics" => Self::METRICS,
            "traces" => Self::TRACES,
            "profiles" => Self::PROFILES,
            "extend" => Self::EXTEND,
            _ => Self::new(value)?,
        })
    }

    pub fn allowed_as_pipeline_target(self) -> bool {
        self != Self::EXTEND && self != Self::PROFILES
    }
}

impl fmt::Debug for StreamTypeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("StreamTypeId")
            .field(&self.as_str())
            .finish()
    }
}

impl fmt::Display for StreamTypeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for StreamTypeId {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        Self::new(value)
    }
}

impl Serialize for StreamTypeId {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.external_name())
    }
}

impl<'de> Deserialize<'de> for StreamTypeId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_external_name(value).map_err(serde::de::Error::custom)
    }
}

type_id! {
    /// 同一逻辑流下物理数据集的用途（records / samples / rollup / …）。
    DatasetTypeId
}

type_id! {
    /// 单个物理文件的格式（parquet / tantivy / bloom / …）。
    ArtifactTypeId
}

type_id! {
    /// 索引器类型；决定由哪个 indexer 构建、哪个 reader 打开。
    IndexTypeId
}

type_id! {
    /// WAL payload 编码格式；新数据类型只需注册新的 codec + decoder。
    WalCodecId
}

/// 内置类型 ID 字面量。字符串一旦发布不可改。
pub mod builtin {
    // Stream types
    pub const STREAM_LOGS: &str = "builtin.logs";
    pub const STREAM_METRICS: &str = "builtin.metrics";
    pub const STREAM_TRACES: &str = "builtin.traces";
    pub const STREAM_PROFILES: &str = "builtin.profiles";
    /// extend table（静态 KV）：不落盘、无物理数据集。
    pub const STREAM_EXTEND: &str = "builtin.extend";

    // Dataset types
    pub const DATASET_LOG_RECORDS: &str = "builtin.logs.records";
    pub const DATASET_RUM_SESSION_SUMMARY: &str = "builtin.logs.rum_session_summary";
    pub const DATASET_RUM_ACTION_SUMMARY: &str = "builtin.logs.rum_action_summary";
    pub const DATASET_RUM_ERROR_SUMMARY: &str = "builtin.logs.rum_error_summary";
    pub const DATASET_METRIC_SAMPLES: &str = "builtin.metrics.samples";
    pub const DATASET_METRIC_ROLLUP: &str = "builtin.metrics.rollup";
    pub const DATASET_METRIC_CATALOG: &str = "builtin.metrics.catalog";
    pub const DATASET_TRACE_SPANS: &str = "builtin.traces.spans";
    pub const DATASET_TRACE_SUMMARY: &str = "builtin.traces.summary";
    pub const DATASET_PROFILE_SAMPLES: &str = "builtin.profiles.samples";

    // Artifact types
    pub const ARTIFACT_PARQUET: &str = "builtin.parquet";
    pub const ARTIFACT_TANTIVY: &str = "builtin.tantivy";
    pub const ARTIFACT_PARTITION_MANIFEST: &str = "builtin.partition_manifest";

    // Index types
    pub const INDEX_TANTIVY: &str = "builtin.tantivy";

    // WAL codecs：现行 intake 批次编码（bincode 行批）。
    pub const WAL_CODEC_ROW_BATCH: &str = "builtin.row_batch";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_namespaced_lowercase_ids() {
        for ok in [
            builtin::STREAM_LOGS,
            builtin::DATASET_METRIC_ROLLUP,
            builtin::ARTIFACT_PARQUET,
            "vendor.custom_signal-v2",
        ] {
            assert!(StreamTypeId::new(ok).is_ok(), "{ok} should be valid");
        }
    }

    #[test]
    fn rejects_malformed_ids() {
        for bad in [
            "",
            "ab",
            "Builtin.logs",
            "builtin..logs",
            ".logs",
            "logs.",
            "builtin.日志",
            "builtin.1logs",
            "builtin.lo gs",
        ] {
            assert!(
                StreamTypeId::new(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn deserialize_validates() {
        assert!(serde_json::from_str::<DatasetTypeId>("\"builtin.logs.records\"").is_ok());
        assert!(serde_json::from_str::<DatasetTypeId>("\"BAD ID\"").is_err());
    }

    #[test]
    fn builtin_ids_are_all_valid() {
        for id in [
            builtin::STREAM_LOGS,
            builtin::STREAM_METRICS,
            builtin::STREAM_TRACES,
            builtin::STREAM_PROFILES,
            builtin::STREAM_EXTEND,
            builtin::DATASET_LOG_RECORDS,
            builtin::DATASET_RUM_SESSION_SUMMARY,
            builtin::DATASET_RUM_ACTION_SUMMARY,
            builtin::DATASET_RUM_ERROR_SUMMARY,
            builtin::DATASET_METRIC_SAMPLES,
            builtin::DATASET_METRIC_ROLLUP,
            builtin::DATASET_METRIC_CATALOG,
            builtin::DATASET_TRACE_SPANS,
            builtin::DATASET_TRACE_SUMMARY,
            builtin::DATASET_PROFILE_SAMPLES,
            builtin::ARTIFACT_PARQUET,
            builtin::ARTIFACT_TANTIVY,
            builtin::ARTIFACT_PARTITION_MANIFEST,
            builtin::INDEX_TANTIVY,
            builtin::WAL_CODEC_ROW_BATCH,
        ] {
            assert!(validate_type_id(id).is_ok(), "{id} must be valid");
        }
    }
}
