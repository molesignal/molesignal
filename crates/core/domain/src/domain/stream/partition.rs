// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 按字段分片：按一两个文本字段的值把数据分进几个分片，每个分片的数据写进各自的文件。带分片字段
//! 等值条件（`=`、`IN`）的查询只读值所在分片的文件；按首字符分片的字段，以文字开头的 `LIKE` /
//! `ILIKE`（`'abc%'`）也只读前缀所在分片的文件。
//!
//! 两个字段时，文件落在两个字段的分片组合成的分片里，分片数相乘：所以最多两个字段，合起来不超过
//! [`MAX_PARTITION_SHARDS`] 片。每个文件记下自己写入时的分片字段（见
//! [`crate::domain::storage::ColumnStats::partitions`]）。查询按文件自己的记录判断，改了分片设置
//! 之后旧文件照样判断得对；合并只把分片字段相同的文件合在一起。

use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result};

/// 按哈希分片时分片数的上限。
const MAX_PARTITION_BUCKETS: u16 = 64;

/// 分片字段最多几个。
const MAX_PARTITION_FIELDS: usize = 2;

/// 各字段分片数之积的上限：一次刷盘，一个时间分区最多写出这么多个文件。
const MAX_PARTITION_SHARDS: u32 = 256;

/// 按首字符分片的分片数：数字、ASCII 字母（不分大小写）各一片，其余一片。
const PREFIX_SHARDS: u16 = 37;

/// 一个流的分片字段，按顺序。没有时不分片。
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FieldPartitions(Vec<FieldPartition>);

/// 按 `field` 的值分片。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FieldPartition {
    pub field: String,
    #[serde(flatten)]
    pub scheme: PartitionScheme,
}

/// 值怎样落进分片。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PartitionScheme {
    /// 值的 FNV-1a 哈希对分片数取余：分得匀，只帮得上等值条件。
    Hash { buckets: u16 },
    /// 值的首字符：数字与 ASCII 字母（不分大小写）各一片，其余一片，共 37 片。分得匀不匀看
    /// 数据，以文字开头的 `LIKE` 也帮得上。
    Prefix,
}

impl FieldPartitions {
    pub fn new(fields: Vec<FieldPartition>) -> Self {
        Self(fields)
    }

    pub fn fields(&self) -> &[FieldPartition] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn validate(&self) -> Result<()> {
        if self.0.len() > MAX_PARTITION_FIELDS {
            return Err(Error::invalid(format!(
                "a stream is partitioned by at most {MAX_PARTITION_FIELDS} fields"
            )));
        }

        for (index, partition) in self.0.iter().enumerate() {
            partition.validate()?;

            if self.0[..index]
                .iter()
                .any(|earlier| earlier.field == partition.field)
            {
                return Err(Error::invalid(format!(
                    "partition field `{}` is listed twice",
                    partition.field
                )));
            }
        }

        if self.shard_count() > MAX_PARTITION_SHARDS {
            return Err(Error::invalid(format!(
                "the shard counts of the partition fields multiply to more than \
                 {MAX_PARTITION_SHARDS}"
            )));
        }

        Ok(())
    }

    /// 分片总数：各字段分片数之积。
    pub fn shard_count(&self) -> u32 {
        self.0
            .iter()
            .map(|partition| u32::from(partition.shard_count()))
            .product()
    }

    /// 一行所在的分片：`values` 按字段顺序、每个字段一个，给出这一行各分片字段的值。分片号由各
    /// 字段的分片号按各自的分片数进位拼成，前面的字段在高位。
    pub fn shard_of<'a>(&self, values: impl IntoIterator<Item = Option<&'a str>>) -> u16 {
        let shard = self
            .0
            .iter()
            .zip(values)
            .fold(0_u32, |shard, (partition, value)| {
                shard * u32::from(partition.shard_count()) + u32::from(partition.shard_of(value))
            });

        u16::try_from(shard).unwrap_or(u16::MAX)
    }

    /// 分片 `shard` 里 `field` 的分片方式与分片号；`field` 不是分片字段时为 `None`。
    pub fn component(&self, shard: u16, field: &str) -> Option<(&FieldPartition, u16)> {
        let index = self
            .0
            .iter()
            .position(|partition| partition.field == field)?;
        let partition = &self.0[index];

        let stride = self.0[index + 1..]
            .iter()
            .map(|later| u32::from(later.shard_count()))
            .product::<u32>();

        let component = u32::from(shard) / stride % u32::from(partition.shard_count());

        Some((partition, component as u16))
    }
}

impl FieldPartition {
    fn validate(&self) -> Result<()> {
        if self.field.trim().is_empty() {
            return Err(Error::invalid("partition field cannot be empty"));
        }

        match self.scheme {
            PartitionScheme::Hash { buckets }
                if !(2..=MAX_PARTITION_BUCKETS).contains(&buckets) =>
            {
                Err(Error::invalid(format!(
                    "partition buckets must be between 2 and {MAX_PARTITION_BUCKETS}"
                )))
            }
            _ => Ok(()),
        }
    }

    /// 这个字段分出的分片数。
    pub fn shard_count(&self) -> u16 {
        match self.scheme {
            PartitionScheme::Hash { buckets } => buckets.max(1),
            PartitionScheme::Prefix => PREFIX_SHARDS,
        }
    }

    /// 值所在的分片，没有值（NULL 或空串）的行在分片 0。哈希是 FNV-1a，前缀按首字符的 ASCII
    /// 小写：都不随版本与平台变化，写进文件的分片号永远算得回来。
    pub fn shard_of(&self, value: Option<&str>) -> u16 {
        let Some(value) = value.filter(|value| !value.is_empty()) else {
            return 0;
        };

        match self.scheme {
            PartitionScheme::Hash { .. } => {
                let hash = value.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                    (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
                });

                (hash % u64::from(self.shard_count())) as u16
            }
            PartitionScheme::Prefix => value.chars().next().map_or(0, prefix_shard),
        }
    }

    /// 以 `prefix` 开头的值可能在的分片；哪个分片都可能时为 `None`。哈希分片看不出前缀。
    /// `case_insensitive`（`ILIKE`）时，ASCII 字母的大小写本就落在同一片，另加上其余字符那一片：
    /// 开尔文符号、长 s 这类非 ASCII 字符不计大小写时等于 ASCII 字母；首字符不是 ASCII 时哪片都
    /// 可能。
    pub fn shards_of_prefix(&self, prefix: &str, case_insensitive: bool) -> Option<Vec<u16>> {
        if !matches!(self.scheme, PartitionScheme::Prefix) {
            return None;
        }

        let first = prefix.chars().next()?;

        match (case_insensitive, first.is_ascii()) {
            (false, _) => Some(vec![prefix_shard(first)]),
            (true, true) => Some(vec![prefix_shard(first), 0]),
            (true, false) => None,
        }
    }
}

/// 首字符所在的前缀分片：`0`–`9` 是 1–10，`a`–`z`（不分大小写）是 11–36，其余是 0。
fn prefix_shard(first: char) -> u16 {
    match first.to_ascii_lowercase() {
        digit @ '0'..='9' => 1 + (digit as u16 - '0' as u16),
        letter @ 'a'..='z' => 11 + (letter as u16 - 'a' as u16),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(field: &str, buckets: u16) -> FieldPartition {
        FieldPartition {
            field: field.into(),
            scheme: PartitionScheme::Hash { buckets },
        }
    }

    fn prefix(field: &str) -> FieldPartition {
        FieldPartition {
            field: field.into(),
            scheme: PartitionScheme::Prefix,
        }
    }

    #[test]
    fn hash_buckets_are_stable_and_spread_values() {
        let partition = hash("service", 8);

        assert_eq!(
            partition.shard_of(Some("checkout")),
            partition.shard_of(Some("checkout"))
        );
        assert_eq!(partition.shard_of(None), 0);
        assert_eq!(partition.shard_of(Some("")), 0);

        let used = (0..200)
            .map(|value| partition.shard_of(Some(&format!("service-{value}"))))
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(used.len(), 8);
        assert!(used.iter().all(|bucket| *bucket < 8));
    }

    /// 写进文件的分片号必须一直算得回来：哈希不能变。
    #[test]
    fn the_hash_is_fnv_1a() {
        assert_eq!(
            hash("service", 1000).shard_of(Some("a")),
            (0xaf63dc4c8601ec8c_u64 % 1000) as u16
        );
    }

    #[test]
    fn prefix_shards_follow_the_first_character_regardless_of_ascii_case() {
        let partition = prefix("trace_id");

        assert_eq!(partition.shard_of(Some("0af7")), 1);
        assert_eq!(partition.shard_of(Some("9")), 10);
        assert_eq!(partition.shard_of(Some("abc")), 11);
        assert_eq!(partition.shard_of(Some("Abc")), 11);
        assert_eq!(partition.shard_of(Some("zeta")), 36);
        assert_eq!(partition.shard_of(Some("-x")), 0);
        assert_eq!(partition.shard_of(Some("订单")), 0);
        assert_eq!(partition.shard_of(Some("")), 0);
        assert_eq!(partition.shard_of(None), 0);
        assert!((0..128).all(|byte| prefix_shard(char::from(byte)) < PREFIX_SHARDS));
    }

    /// `LIKE` 的前缀只落在一片；`ILIKE` 另加其余字符那片；首字符不是 ASCII 的 `ILIKE` 与哈希分片
    /// 都排除不了任何一片。
    #[test]
    fn a_prefix_names_the_shards_its_values_can_be_in() {
        let partition = prefix("trace_id");

        assert_eq!(partition.shards_of_prefix("Ab", false), Some(vec![11]));
        assert_eq!(partition.shards_of_prefix("ab", true), Some(vec![11, 0]));
        assert_eq!(partition.shards_of_prefix("éa", false), Some(vec![0]));
        assert_eq!(partition.shards_of_prefix("éa", true), None);
        assert_eq!(partition.shards_of_prefix("", false), None);
        assert_eq!(hash("service", 8).shards_of_prefix("ab", false), None);
    }

    /// 两个字段的分片号拼成一个，每个字段的分片号都能从中拆回来。
    #[test]
    fn two_fields_combine_into_one_shard_and_split_back() {
        let partitions = FieldPartitions::new(vec![hash("service", 4), prefix("pod")]);

        assert_eq!(partitions.shard_count(), 148);

        for service in ["api", "web", "db", "checkout"] {
            for pod in [Some("web-1"), Some("0f"), Some("Ω"), None] {
                let shard = partitions.shard_of([Some(service), pod]);

                assert!(u32::from(shard) < partitions.shard_count());
                assert_eq!(
                    partitions.component(shard, "service").unwrap().1,
                    hash("service", 4).shard_of(Some(service))
                );
                assert_eq!(
                    partitions.component(shard, "pod").unwrap().1,
                    prefix("pod").shard_of(pod)
                );
                assert!(partitions.component(shard, "level").is_none());
            }
        }

        let one = FieldPartitions::new(vec![prefix("pod")]);
        assert_eq!(one.shard_of([Some("web-1")]), 33);
        assert_eq!(one.component(33, "pod").unwrap().1, 33);
    }

    #[test]
    fn the_settings_are_a_list_of_fields_each_tagged_with_its_scheme() {
        let partitions = FieldPartitions::new(vec![hash("service", 8), prefix("trace_id")]);
        let json = serde_json::json!([
            {"field": "service", "type": "hash", "buckets": 8},
            {"field": "trace_id", "type": "prefix"}
        ]);

        assert_eq!(serde_json::to_value(&partitions).unwrap(), json);
        assert_eq!(
            serde_json::from_value::<FieldPartitions>(json).unwrap(),
            partitions
        );
    }

    #[test]
    fn at_most_two_distinct_fields_of_at_most_256_shards_together() {
        let valid = |fields: Vec<FieldPartition>| FieldPartitions::new(fields).validate().is_ok();

        assert!(valid(vec![]));
        assert!(valid(vec![hash("service", 2)]));
        assert!(valid(vec![hash("service", 64)]));
        assert!(valid(vec![prefix("trace_id")]));
        assert!(valid(vec![prefix("pod"), hash("service", 4)]));
        assert!(valid(vec![hash("service", 16), hash("namespace", 16)]));

        assert!(!valid(vec![hash("service", 1)]));
        assert!(!valid(vec![hash("service", 65)]));
        assert!(!valid(vec![prefix(" ")]));
        assert!(!valid(vec![prefix("pod"), hash("service", 8)]));
        assert!(!valid(vec![prefix("pod"), prefix("host")]));
        assert!(!valid(vec![hash("service", 4), prefix("service")]));
        assert!(!valid(vec![hash("a", 2), hash("b", 2), hash("c", 2)]));
    }
}
