// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Extend table in-memory store。
//!
//! - `(org, table_name)` → `HashMap<key, Value>`
//! - 启动期/事件触发时由 [`ExtendTable::rebuild`] 从 extend_kv 表全量重读
//! - VRL 内置 `lookup(table, key)` → 走 [`ExtendTable::lookup`]
//!
//! 写入并发：`Arc<RwLock<...>>`。读多写少；重建用 swap-in 整张表的方式避免读阻塞。

use std::{collections::HashMap, sync::Arc};

use parking_lot::RwLock;
use serde_json::Value;

use crate::shared::ids::Id;

pub mod repository;

type Tables = HashMap<(Id, String), HashMap<String, Value>>;

#[derive(Default)]
pub struct ExtendTable {
    inner: Arc<RwLock<Tables>>,
}

impl ExtendTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn lookup(&self, org: &Id, table: &str, key: &str) -> Option<Value> {
        let g = self.inner.read();
        g.get(&(org.clone(), table.to_string()))
            .and_then(|m| m.get(key).cloned())
    }

    /// 全量 swap-in 一张表（重建）。调用方一次性把单张表的所有 row 给进来。
    pub fn replace_table(&self, org: &Id, table: &str, rows: HashMap<String, Value>) {
        let mut g = self.inner.write();
        g.insert((org.clone(), table.to_string()), rows);
    }

    pub fn drop_table(&self, org: &Id, table: &str) {
        let mut g = self.inner.write();
        g.remove(&(org.clone(), table.to_string()));
    }

    pub fn table_size(&self, org: &Id, table: &str) -> usize {
        let g = self.inner.read();
        g.get(&(org.clone(), table.to_string()))
            .map(|m| m.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_lookup_drop() {
        let et = ExtendTable::new();
        let org = Id("o1".into());
        let mut rows = HashMap::new();
        rows.insert("svc-a".to_string(), Value::String("team-alpha".into()));
        rows.insert("svc-b".to_string(), Value::String("team-beta".into()));
        et.replace_table(&org, "svc_owner", rows);
        assert_eq!(et.table_size(&org, "svc_owner"), 2);
        assert_eq!(
            et.lookup(&org, "svc_owner", "svc-a").unwrap().as_str(),
            Some("team-alpha")
        );
        et.drop_table(&org, "svc_owner");
        assert!(et.lookup(&org, "svc_owner", "svc-a").is_none());
    }
}
