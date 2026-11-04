// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 用 Catalog 里每个文件的统计裁剪：字段的 min/max（zone map，建了索引的字段都有）、值不多的
//! `skip` 字段的值集合，以及 `ip` 字段的地址范围。
//!
//! 证明条件树（[`WhereTree`]）里落在一列上的原子条件不成立：比较、`IN`、`BETWEEN`、前缀
//! `LIKE` 与 `cidr_match`。别的函数、`NOT` 与证明不了的类型组合一律当作可能成立。统计只决定一个
//! Parquet 文件是否可能命中，最终行级语义仍由 DataFusion 执行。

use crate::{
    domain::{storage::QueryFile, stream::Schema},
    infra::query::parser::{Refuted, WhereTree, atom_bit},
};

mod condition;
mod evidence;

/// 条件树里能用文件统计判断的原子条件。
pub struct Atoms(Vec<(usize, condition::Condition)>);

impl Atoms {
    pub fn new(tree: &WhereTree, schema: &Schema) -> Self {
        Self(condition::extract(tree, schema))
    }

    /// 这些原子的位。
    pub fn mask(&self) -> Refuted {
        self.0
            .iter()
            .fold(0, |mask, (atom, _)| mask | atom_bit(*atom))
    }

    /// `file` 的统计证明不成立的原子。
    pub fn refuted(&self, file: &QueryFile) -> Refuted {
        self.0
            .iter()
            .filter(|(_, condition)| !evidence::may_match(condition, file))
            .fold(0, |refuted, (atom, _)| refuted | atom_bit(*atom))
    }
}

#[cfg(test)]
mod tests;
