// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 按字段分片写入的文件，每一行都落在文件那个分片里：分片字段上的等值条件（`=`、`IN`）只可能在
//! 值所在分片的文件里成立；按首字符分片的字段，以文字开头的 `LIKE` / `ILIKE` 只可能在前缀所在
//! 分片的文件里成立。两个分片字段时各按自己那部分分片号判断。文件按自己写入时的分片字段判断，
//! 没有分片的文件证明不了什么。

use crate::{
    domain::storage::{FileShard, QueryFile},
    infra::query::parser::{
        EqualitySet, LikePattern, LikeSet, Refuted, WhereTree, atom_bit, equality_atom, like_atom,
    },
};

/// 条件树里分片能判断的原子条件：等值条件与 `LIKE` / `ILIKE`。
#[derive(Default)]
pub(super) struct Atoms {
    equalities: Vec<(usize, EqualitySet)>,
    likes: Vec<(usize, LikeSet)>,
}

impl Atoms {
    pub(super) fn new(tree: &WhereTree) -> Self {
        let mut atoms = Self::default();

        for (atom, expr) in tree.atoms() {
            if let Some(set) = equality_atom(expr) {
                atoms.equalities.push((atom, set));
            } else if let Some(set) = like_atom(expr) {
                atoms.likes.push((atom, set));
            }
        }

        atoms
    }

    /// 这些原子的位。
    pub(super) fn mask(&self) -> Refuted {
        self.equalities
            .iter()
            .map(|(atom, _)| atom)
            .chain(self.likes.iter().map(|(atom, _)| atom))
            .fold(0, |mask, atom| mask | atom_bit(*atom))
    }

    /// `file` 的分片证明不成立的原子。
    pub(super) fn refuted(&self, file: &QueryFile) -> Refuted {
        let Some(shard) = &file.shard else {
            return 0;
        };

        let equalities = self
            .equalities
            .iter()
            .filter(|(_, set)| !shard.may_hold_any(&set.column, &set.values));

        let likes = self
            .likes
            .iter()
            .filter(|(_, set)| !may_match_any(shard, &set.column, &set.patterns));

        equalities
            .map(|(atom, _)| atom)
            .chain(likes.map(|(atom, _)| atom))
            .fold(0, |refuted, atom| refuted | atom_bit(*atom))
    }
}

/// 文件里可能有 `column` 匹配 `patterns` 之一的行：模式不以文字开头时哪个分片都可能。
fn may_match_any(shard: &FileShard, column: &str, patterns: &[LikePattern]) -> bool {
    patterns.iter().any(|pattern| {
        pattern
            .prefix()
            .is_none_or(|prefix| shard.may_hold_prefix(column, prefix, pattern.case_insensitive))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{
            storage::primary_dataset_type,
            stream::{FieldPartition, FieldPartitions, PartitionScheme, StreamType},
        },
        infra::query::refutations::Refutations,
        shared::{
            ids::Id,
            time::{TimeRange, TimestampMicros},
        },
    };

    fn file(id: &str, shard: Option<FileShard>) -> QueryFile {
        QueryFile {
            id: Id::from_string(id),
            org_id: Id::from_string("org"),
            stream: "logs".into(),
            stream_type: StreamType::LOGS,
            dataset_type: primary_dataset_type(StreamType::LOGS).unwrap(),
            object_key: format!("{id}.parquet"),
            checksum: None,
            etag: None,
            time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(1)),
            rows: 1,
            size_bytes: 1,
            min_values: serde_json::Map::new(),
            max_values: serde_json::Map::new(),
            value_sets: serde_json::Map::new(),
            address_ranges: serde_json::Map::new(),
            null_counts: serde_json::Map::new(),
            indexed_at_micros: None,
            shard,
        }
    }

    /// 文件 `id` 写着 `values`（按分片字段的顺序）落在的那个分片。
    fn sharded(id: &str, partitions: &FieldPartitions, values: &[&str]) -> QueryFile {
        file(
            id,
            Some(FileShard {
                partitions: partitions.clone(),
                shard: partitions.shard_of(values.iter().map(|value| Some(*value))),
            }),
        )
    }

    fn kept(files: &[QueryFile], sql: &str) -> Vec<String> {
        let tree = WhereTree::parse(sql);
        let atoms = Atoms::new(&tree);

        Refutations::new(tree)
            .retain(files.to_vec(), |file| atoms.refuted(file))
            .iter()
            .map(|file| file.id.as_str().to_string())
            .collect()
    }

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
    fn only_the_hash_shards_of_the_wanted_values_are_read() {
        let partitions = FieldPartitions::new(vec![hash("service", 8)]);

        let other = (0..)
            .map(|index| format!("service-{index}"))
            .find(|value| {
                partitions.shard_of([Some(value.as_str())])
                    != partitions.shard_of([Some("checkout")])
            })
            .unwrap();

        let files = vec![
            sharded("checkout", &partitions, &["checkout"]),
            sharded("other", &partitions, &[&other]),
            file("unsharded", None),
        ];

        assert_eq!(
            kept(&files, "SELECT * FROM logs WHERE service = 'checkout'"),
            ["checkout", "unsharded"]
        );
        assert_eq!(
            kept(&files, "SELECT * FROM logs WHERE level = 'error'"),
            ["checkout", "other", "unsharded"]
        );
        assert_eq!(
            kept(
                &files,
                "SELECT * FROM logs WHERE service = 'checkout' OR level = 'error'"
            ),
            ["checkout", "other", "unsharded"]
        );
        assert_eq!(
            kept(&files, "SELECT * FROM logs WHERE service LIKE 'check%'"),
            ["checkout", "other", "unsharded"]
        );
    }

    /// 首字符分片：等值与以文字开头的 `LIKE` 只读首字符那片；`ILIKE` 另读其余字符那片；模式以
    /// 通配符开头时都读。
    #[test]
    fn only_the_prefix_shards_a_value_or_pattern_can_start_in_are_read() {
        let partitions = FieldPartitions::new(vec![prefix("trace_id")]);

        let files = ["0", "a", "b", "-"]
            .into_iter()
            .map(|first| sharded(first, &partitions, &[first]))
            .collect::<Vec<_>>();

        assert_eq!(kept(&files, "SELECT * FROM t WHERE trace_id = 'a1'"), ["a"]);
        assert_eq!(
            kept(&files, "SELECT * FROM t WHERE trace_id IN ('0f', 'B2')"),
            ["0", "b"]
        );
        assert_eq!(
            kept(&files, "SELECT * FROM t WHERE trace_id LIKE 'A%'"),
            ["a"]
        );
        assert_eq!(
            kept(&files, "SELECT * FROM t WHERE trace_id ILIKE 'a%'"),
            ["a", "-"]
        );
        assert_eq!(
            kept(
                &files,
                "SELECT * FROM t WHERE trace_id LIKE '0%' OR trace_id LIKE 'b%'"
            ),
            ["0", "b"]
        );
        assert_eq!(
            kept(&files, "SELECT * FROM t WHERE trace_id LIKE '%a%'"),
            ["0", "a", "b", "-"]
        );
        assert!(
            kept(
                &files,
                "SELECT * FROM t WHERE trace_id LIKE 'a%' AND trace_id = 'b1'"
            )
            .is_empty()
        );
    }

    /// 两个分片字段：每个字段的条件按它自己那部分分片号判断，两个都带时一起裁。
    #[test]
    fn each_of_two_partition_fields_prunes_by_its_own_part_of_the_shard() {
        let service = hash("service", 2);
        let partitions = FieldPartitions::new(vec![service.clone(), prefix("pod")]);

        let left = "checkout";
        let right = (0..)
            .map(|index| format!("service-{index}"))
            .find(|value| service.shard_of(Some(value)) != service.shard_of(Some(left)))
            .unwrap();
        let right = right.as_str();

        let files = vec![
            sharded("left-a", &partitions, &[left, "api-1"]),
            sharded("left-w", &partitions, &[left, "web-1"]),
            sharded("right-a", &partitions, &[right, "api-2"]),
            sharded("right-w", &partitions, &[right, "web-2"]),
        ];

        assert_eq!(
            kept(&files, &format!("SELECT * FROM t WHERE service = '{left}'")),
            ["left-a", "left-w"]
        );
        assert_eq!(
            kept(&files, "SELECT * FROM t WHERE pod LIKE 'web%'"),
            ["left-w", "right-w"]
        );
        assert_eq!(
            kept(
                &files,
                &format!("SELECT * FROM t WHERE service = '{right}' AND pod = 'api-9'")
            ),
            ["right-a"]
        );
        assert_eq!(
            kept(
                &files,
                &format!("SELECT * FROM t WHERE service = '{right}' OR pod LIKE 'web%'")
            ),
            ["left-w", "right-a", "right-w"],
            "an OR across the two fields reads the shards of either branch"
        );
        assert_eq!(
            kept(&files, "SELECT * FROM t WHERE level = 'error'").len(),
            4
        );
    }
}
