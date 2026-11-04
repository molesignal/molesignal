// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 扩展表的内存副本，给 VRL 的 `lookup(table, key)` 查。
//!
//! - 按 `(组织, 表)` 在用到时从 `extend_kv` 整表读进来（[`ExtendTable::prepare`]），没用到的表
//!   不占内存。
//! - 读进来的表留 [`TTL`]：别的节点改的行最多晚这么久查得到；本节点改了表
//!   （[`ExtendTable::forget`]）下一次用到就重读。
//! - 查询（[`ExtendTable::lookup`]）只读内存，VRL 执行时同步调用。

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::RwLock;
use serde_json::Value;

use crate::shared::{Result, ids::Id};

pub mod repository;

use repository::ExtendKvRepository;

/// 读进来的表在内存里留多久。
const TTL: Duration = Duration::from_secs(30);

struct Loaded {
    at: Instant,
    rows: Arc<HashMap<String, Value>>,
}

pub struct ExtendTable {
    store: Arc<dyn ExtendKvRepository>,
    loaded: RwLock<HashMap<(Id, String), Loaded>>,
}

impl ExtendTable {
    pub fn new(store: Arc<dyn ExtendKvRepository>) -> Self {
        Self {
            store,
            loaded: RwLock::new(HashMap::new()),
        }
    }

    /// 把组织的这几张表读进来；已经读过、还没过期的不重读。没有的表读成空表。
    pub async fn prepare(&self, org: &Id, tables: &[String]) -> Result<()> {
        for table in tables {
            let fresh = self
                .loaded
                .read()
                .get(&(org.clone(), table.clone()))
                .is_some_and(|loaded| loaded.at.elapsed() < TTL);

            if fresh {
                continue;
            }

            let rows = self
                .store
                .list_table(org, table)
                .await?
                .into_iter()
                .map(|row| (row.key, row.value_json))
                .collect();

            self.loaded.write().insert(
                (org.clone(), table.clone()),
                Loaded {
                    at: Instant::now(),
                    rows: Arc::new(rows),
                },
            );
        }

        Ok(())
    }

    /// 组织的表 `table` 里键为 `key` 的那一行；表还没读进来时也是 `None`。
    pub fn lookup(&self, org: &Id, table: &str, key: &str) -> Option<Value> {
        self.loaded
            .read()
            .get(&(org.clone(), table.to_string()))
            .and_then(|loaded| loaded.rows.get(key).cloned())
    }

    /// 本节点改了组织的这张表：下一次用到时重读。
    pub fn forget(&self, org: &Id, table: &str) {
        self.loaded
            .write()
            .remove(&(org.clone(), table.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use serde_json::json;

    use super::{
        repository::{ExtendRow, ExtendTableDefinition, ExtendTableSummary, ExtendValueField},
        *,
    };
    use crate::shared::time::TimestampMicros;

    /// Rows of one table, counting how often they are read.
    #[derive(Default)]
    struct Store {
        rows: RwLock<Vec<(String, Value)>>,
        reads: AtomicUsize,
    }

    #[async_trait]
    impl ExtendKvRepository for Store {
        async fn create_table(
            &self,
            table: ExtendTableDefinition,
        ) -> Result<ExtendTableDefinition> {
            Ok(table)
        }

        async fn update_table_fields(
            &self,
            _: &Id,
            _: &str,
            _: &[ExtendValueField],
            _: TimestampMicros,
            _: bool,
        ) -> Result<bool> {
            Ok(true)
        }

        async fn delete_table(&self, _: &Id, _: &str) -> Result<()> {
            Ok(())
        }

        async fn upsert(&self, _: ExtendRow) -> Result<()> {
            Ok(())
        }

        async fn delete(&self, _: &Id, _: &str, _: &str) -> Result<()> {
            Ok(())
        }

        async fn list_table(&self, org: &Id, table: &str) -> Result<Vec<ExtendRow>> {
            self.reads.fetch_add(1, Ordering::SeqCst);

            Ok(self
                .rows
                .read()
                .iter()
                .map(|(key, value)| ExtendRow {
                    id: Id(key.clone()),
                    org_id: org.clone(),
                    table_name: table.to_string(),
                    key: key.clone(),
                    value_json: value.clone(),
                    updated_at: TimestampMicros(0),
                })
                .collect())
        }

        async fn list_tables(&self, _: &Id) -> Result<Vec<ExtendTableSummary>> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn a_table_is_read_once_and_again_after_it_is_forgotten() {
        let store = Arc::new(Store::default());
        store
            .rows
            .write()
            .push(("svc-a".into(), json!({ "team": "alpha" })));
        let tables = ExtendTable::new(store.clone());
        let org = Id("o1".into());
        let owners = vec!["svc_owner".to_string()];

        assert!(tables.lookup(&org, "svc_owner", "svc-a").is_none());

        tables.prepare(&org, &owners).await.unwrap();
        tables.prepare(&org, &owners).await.unwrap();

        assert_eq!(
            tables.lookup(&org, "svc_owner", "svc-a"),
            Some(json!({ "team": "alpha" }))
        );
        assert_eq!(store.reads.load(Ordering::SeqCst), 1);
        // Another organization's table of the same name is its own.
        assert!(
            tables
                .lookup(&Id("o2".into()), "svc_owner", "svc-a")
                .is_none()
        );

        store.rows.write().clear();
        tables.forget(&org, "svc_owner");
        tables.prepare(&org, &owners).await.unwrap();

        assert!(tables.lookup(&org, "svc_owner", "svc-a").is_none());
        assert_eq!(store.reads.load(Ordering::SeqCst), 2);
    }
}
