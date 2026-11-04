// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 每个候选文件里被索引证明不成立的原子条件（[`WhereTree`] 的叶子），按 object key 记下。
//!
//! 剪枝分几步：分片、文件级过滤器、文件统计、Tantivy。每一步只证明自己看得懂的原子条件，证明
//! 累积起来；一个文件的条件树在这些原子都不成立时已不可能成立，它就不读。所以一支靠一种索引、
//! 另一支靠另一种索引的 `OR` 也能剔除文件。

use std::collections::HashMap;

use crate::{
    domain::storage::QueryFile,
    infra::query::parser::{Refuted, WhereTree},
};

#[derive(Debug, Default)]
pub struct Refutations {
    tree: WhereTree,
    refuted: HashMap<String, Refuted>,
}

impl Refutations {
    pub fn new(tree: WhereTree) -> Self {
        Self {
            tree,
            refuted: HashMap::new(),
        }
    }

    pub fn tree(&self) -> &WhereTree {
        &self.tree
    }

    /// `file` 里已证明不成立的原子。
    pub fn of(&self, file: &QueryFile) -> Refuted {
        self.refuted.get(&file.object_key).copied().unwrap_or(0)
    }

    /// `atoms` 里的原子全被证明不成立时，有文件可能因此不读：别的原子证明不了，条件树就可能
    /// 成立不了。
    pub fn could_drop(&self, atoms: Refuted) -> bool {
        atoms != 0 && !self.tree.may_hold(atoms)
    }

    /// 记下 `file` 里又有 `refuted` 这些原子不成立；条件树在它里面已不可能成立时为 `false`。
    pub fn record(&mut self, file: &QueryFile, refuted: Refuted) -> bool {
        let known = self.refuted.entry(file.object_key.clone()).or_default();
        *known |= refuted;

        self.tree.may_hold(*known)
    }

    /// `files` 里，`refute` 再证明一些原子不成立之后，条件树仍可能成立的那些。
    pub fn retain(
        &mut self,
        files: Vec<QueryFile>,
        refute: impl Fn(&QueryFile) -> Refuted,
    ) -> Vec<QueryFile> {
        files
            .into_iter()
            .filter(|file| match refute(file) {
                0 => true,
                refuted => self.record(file, refuted),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{storage::primary_dataset_type, stream::StreamType},
        infra::query::parser::atom_bit,
        shared::{
            ids::Id,
            time::{TimeRange, TimestampMicros},
        },
    };

    fn file(key: &str) -> QueryFile {
        QueryFile {
            id: Id::from_string(key),
            org_id: Id::from_string("org"),
            stream: "logs".into(),
            stream_type: StreamType::LOGS,
            dataset_type: primary_dataset_type(StreamType::LOGS).unwrap(),
            object_key: key.into(),
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
            shard: None,
        }
    }

    /// 两支各由一步证明：哪一步单独都剔除不了文件，合起来才行。
    #[test]
    fn the_branches_of_an_or_may_be_refuted_by_different_steps() {
        let tree = WhereTree::parse("SELECT * FROM logs WHERE level = 'error' OR status >= 500");
        let mut refutations = Refutations::new(tree);
        let files = vec![file("a"), file("b")];

        let files = refutations.retain(files, |_| atom_bit(0));
        assert_eq!(files.len(), 2);

        let files = refutations.retain(files, |file| match file.object_key.as_str() {
            "a" => atom_bit(1),
            _ => 0,
        });
        assert_eq!(
            files
                .iter()
                .map(|file| file.object_key.as_str())
                .collect::<Vec<_>>(),
            ["b"]
        );
        assert!(refutations.could_drop(atom_bit(0) | atom_bit(1)));
        assert!(!refutations.could_drop(atom_bit(0)));
    }
}
