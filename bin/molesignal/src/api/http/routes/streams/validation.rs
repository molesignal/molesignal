// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 数据流设置的校验：保留天数、字段索引规则、查询时间范围上限与指标的降采样。

use std::collections::HashSet;

use crate::{
    domain::{
        iam::MAX_STREAM_RETENTION_DAYS,
        stream::{FieldType, Schema, StreamIndexType, StreamSettings, StreamType},
    },
    shared::{Error, Result},
};

/// 新字段的索引模式最多几条、每条最长几个字符。
const MAX_INDEX_PATTERNS: usize = 32;
const MAX_INDEX_PATTERN_CHARS: usize = 128;

/// 一天的秒数：降采样的时间桶须整除它，桶才与天对齐。
const DAY_SECS: u32 = 86_400;
/// 降采样时间桶的下限，单位秒。
const MIN_DOWNSAMPLE_INTERVAL_SECS: u32 = 60;
/// 查询时间范围上限的最大值，单位小时：十年。
const MAX_QUERY_RANGE_HOURS: u32 = MAX_STREAM_RETENTION_DAYS * 24;

pub(super) fn validate_days(days: u32) -> Result<()> {
    if days == 0 || days > MAX_STREAM_RETENTION_DAYS {
        return Err(Error::invalid(format!(
            "retention_days must be between 1 and {MAX_STREAM_RETENTION_DAYS}"
        )));
    }

    Ok(())
}

/// 只用于文本的索引类型（`full_text`、`ngram`、`ip`）仅限 string（utf8）字段（spec
/// stream-index-config），`key_value` 仅限 json 字段。create 与 update_settings 对这样的新配置
/// 返回 400；存量 json full_text 配置不受影响（写侧 builder 保持 `Utf8 | Json`，仅拦新提交）。
pub(super) fn validate_index_data_type(
    field_name: &str,
    data_type: FieldType,
    index_type: StreamIndexType,
) -> Result<()> {
    if index_type.suits(data_type) {
        return Ok(());
    }

    let (name, kind) = match index_type {
        StreamIndexType::KeyValue => ("key_value", "json"),
        StreamIndexType::FullText => ("full_text", "string (utf8)"),
        StreamIndexType::Ngram => ("ngram", "string (utf8)"),
        _ => ("ip", "string (utf8)"),
    };

    Err(Error::invalid(format!(
        "{name} index type is only supported on {kind} fields; \
         field `{field_name}` has type `{data_type:?}`"
    )))
}

pub(super) fn validate_settings(settings: &StreamSettings, stream_type: StreamType) -> Result<()> {
    let mut indexed_fields = HashSet::new();

    for rule in &settings.index_rules {
        if rule.field.trim().is_empty() {
            return Err(Error::invalid("index rule field cannot be empty"));
        }

        if !indexed_fields.insert(rule.field.as_str()) {
            return Err(Error::invalid(format!(
                "duplicate index rule for field `{}`",
                rule.field
            )));
        }
    }

    validate_index_patterns(settings)?;

    if settings
        .max_query_range_hours
        .is_some_and(|hours| hours == 0 || hours > MAX_QUERY_RANGE_HOURS)
    {
        return Err(Error::invalid(format!(
            "max_query_range_hours must be between 1 and {MAX_QUERY_RANGE_HOURS}"
        )));
    }

    if !settings.partitions.is_empty() {
        settings.partitions.validate()?;

        if !matches!(stream_type, StreamType::LOGS | StreamType::TRACES) {
            return Err(Error::invalid(
                "field partitioning applies to logs and traces streams only",
            ));
        }
    }

    validate_downsample(settings, stream_type)
}

/// 新字段的索引模式：不空、不重复、不太长也不太多。模式按次序生效，类型不合适的字段跳过它，
/// 所以不按类型校验。
fn validate_index_patterns(settings: &StreamSettings) -> Result<()> {
    if settings.index_patterns.len() > MAX_INDEX_PATTERNS {
        return Err(Error::invalid(format!(
            "at most {MAX_INDEX_PATTERNS} index patterns are allowed"
        )));
    }

    let mut seen = HashSet::new();

    for pattern in &settings.index_patterns {
        let text = pattern.pattern.as_str();

        if text.trim().is_empty() || text.chars().count() > MAX_INDEX_PATTERN_CHARS {
            return Err(Error::invalid(format!(
                "index pattern must have 1 to {MAX_INDEX_PATTERN_CHARS} characters"
            )));
        }

        if !seen.insert(text) {
            return Err(Error::invalid(format!("duplicate index pattern `{text}`")));
        }
    }

    Ok(())
}

/// 分片字段已在 schema 里时，必须是没加密的文本字段：分片按字段的明文文本算。还没出现的字段
/// 可以先配置，写入时它若不是文本，那批数据不分片。
pub(super) fn validate_partition_fields(settings: &StreamSettings, schema: &Schema) -> Result<()> {
    for partition in settings.partitions.fields() {
        if let Some(field) = schema
            .fields
            .iter()
            .find(|field| field.name == partition.field)
            .filter(|field| field.data_type != FieldType::Utf8 || field.encrypted)
        {
            return Err(Error::invalid(format!(
                "partition field `{}` must be an unencrypted string (utf8) field",
                field.name
            )));
        }
    }

    Ok(())
}

/// 降采样只用于指标流；样本保留的天数不超过保留天数的上限，时间桶至少一分钟、整除一天。
fn validate_downsample(settings: &StreamSettings, stream_type: StreamType) -> Result<()> {
    let downsample = settings.downsample;

    if !downsample.enabled() {
        return Ok(());
    }

    if stream_type != StreamType::METRICS {
        return Err(Error::invalid(
            "downsampling applies to metrics streams only",
        ));
    }

    if downsample.after_days > MAX_STREAM_RETENTION_DAYS {
        return Err(Error::invalid(format!(
            "downsample.after_days must be between 0 and {MAX_STREAM_RETENTION_DAYS}"
        )));
    }

    let interval = downsample.interval_secs;

    if !(MIN_DOWNSAMPLE_INTERVAL_SECS..=DAY_SECS).contains(&interval)
        || !DAY_SECS.is_multiple_of(interval)
    {
        return Err(Error::invalid(
            "downsample.interval_secs must be at least 60 seconds and divide a day evenly",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::stream::{Downsample, FieldPartition, FieldPartitions, PartitionScheme};

    fn downsampled(after_days: u32, interval_secs: u32) -> StreamSettings {
        StreamSettings {
            downsample: Downsample {
                after_days,
                interval_secs,
            },
            ..StreamSettings::default()
        }
    }

    #[test]
    fn downsampling_is_for_metrics_with_buckets_that_divide_a_day() {
        for interval in [60, 300, 900, 3_600, 86_400] {
            assert!(
                validate_settings(&downsampled(7, interval), StreamType::METRICS).is_ok(),
                "{interval}"
            );
        }

        for interval in [30, 7_000, 172_800] {
            assert!(
                validate_settings(&downsampled(7, interval), StreamType::METRICS).is_err(),
                "{interval}"
            );
        }

        assert!(validate_settings(&downsampled(7, 3_600), StreamType::LOGS).is_err());
        assert!(validate_settings(&downsampled(0, 3_600), StreamType::LOGS).is_ok());
        assert!(validate_settings(&downsampled(4_000, 3_600), StreamType::METRICS).is_err());
    }

    #[test]
    fn a_query_range_limit_is_at_least_an_hour() {
        let limited = |hours| StreamSettings {
            max_query_range_hours: Some(hours),
            ..StreamSettings::default()
        };

        assert!(validate_settings(&limited(24), StreamType::LOGS).is_ok());
        assert!(validate_settings(&limited(0), StreamType::LOGS).is_err());
        assert!(validate_settings(&limited(MAX_QUERY_RANGE_HOURS + 1), StreamType::LOGS).is_err());
    }

    fn partitioned(field: &str, buckets: u16) -> StreamSettings {
        partitioned_by(field, PartitionScheme::Hash { buckets })
    }

    fn partitioned_by(field: &str, scheme: PartitionScheme) -> StreamSettings {
        StreamSettings {
            partitions: FieldPartitions::new(vec![FieldPartition {
                field: field.into(),
                scheme,
            }]),
            ..StreamSettings::default()
        }
    }

    #[test]
    fn field_partitions_are_for_logs_and_traces_with_a_sane_bucket_count() {
        assert!(validate_settings(&partitioned("service", 8), StreamType::LOGS).is_ok());
        assert!(validate_settings(&partitioned("service", 8), StreamType::TRACES).is_ok());
        assert!(validate_settings(&partitioned("service", 8), StreamType::METRICS).is_err());
        assert!(validate_settings(&partitioned("service", 1), StreamType::LOGS).is_err());
        assert!(validate_settings(&partitioned("", 8), StreamType::LOGS).is_err());

        let prefix = partitioned_by("trace_id", PartitionScheme::Prefix);
        assert!(validate_settings(&prefix, StreamType::TRACES).is_ok());
        assert!(validate_settings(&prefix, StreamType::METRICS).is_err());

        let both = |buckets: u16| StreamSettings {
            partitions: FieldPartitions::new(vec![
                FieldPartition {
                    field: "service".into(),
                    scheme: PartitionScheme::Hash { buckets },
                },
                FieldPartition {
                    field: "pod".into(),
                    scheme: PartitionScheme::Prefix,
                },
            ]),
            ..StreamSettings::default()
        };

        assert!(validate_settings(&both(4), StreamType::LOGS).is_ok());
        assert!(validate_settings(&both(8), StreamType::LOGS).is_err());
    }

    #[test]
    fn a_partition_field_already_in_the_schema_must_be_plain_text() {
        let field =
            |name: &str, data_type: FieldType, encrypted: bool| crate::domain::stream::FieldDef {
                name: name.into(),
                data_type,
                nullable: true,
                index_type: None,
                indexed: false,
                encrypted,
                exact: false,
            };

        let schema = Schema {
            fields: vec![
                field("service", FieldType::Utf8, false),
                field("status", FieldType::Int64, false),
                field("user", FieldType::Utf8, true),
            ],
        };

        assert!(validate_partition_fields(&partitioned("service", 8), &schema).is_ok());
        assert!(validate_partition_fields(&partitioned("not_yet", 8), &schema).is_ok());
        assert!(validate_partition_fields(&partitioned("status", 8), &schema).is_err());
        assert!(validate_partition_fields(&partitioned("user", 8), &schema).is_err());
        assert!(validate_partition_fields(&StreamSettings::default(), &schema).is_ok());

        let second_encrypted = StreamSettings {
            partitions: FieldPartitions::new(vec![
                FieldPartition {
                    field: "service".into(),
                    scheme: PartitionScheme::Prefix,
                },
                FieldPartition {
                    field: "user".into(),
                    scheme: PartitionScheme::Hash { buckets: 4 },
                },
            ]),
            ..StreamSettings::default()
        };

        assert!(validate_partition_fields(&second_encrypted, &schema).is_err());
    }
}
