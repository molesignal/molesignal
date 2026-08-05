// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Per-org schema 缓存。
//!
//! ingest / query 路径需要快速查 `(org_id, stream_name, stream_type) → StreamDefinition`，
//! 否则每条事件 / 每条查询都跑一次 Pg query。本缓存：
//!
//! - `Arc<RwLock<HashMap<key, Arc<StreamDefinition>>>>`：读写比 ≫ 1，写仅在 schema
//!   变更时发生（CRUD + `update_schema` invalidation hook）。
//! - `invalidate_for(org_id, name, stream_type)`：精确撤销一条。
//! - `invalidate_org(org_id)`：批量撤销整个 org（org schema CRUD 批量改动用）。
//!
//! 不持有 repo 引用：缓存只做 KV，miss 时由 caller 调 repo 填回（典型模式：
//! `if let Some(s) = cache.get(...) { s } else { let s = repo.get(...).await?;
//!  cache.put(...); s }`）。

use std::{collections::HashMap, sync::Arc};

use parking_lot::RwLock;

use crate::{
    domain::stream::{StreamDefinition, StreamType},
    shared::ids::Id,
};

type Key = (String, String, StreamType);

#[derive(Default)]
pub struct OrgSchemaCache {
    inner: RwLock<HashMap<Key, Arc<StreamDefinition>>>,
}

impl OrgSchemaCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(
        &self,
        org_id: &Id,
        name: &str,
        stream_type: StreamType,
    ) -> Option<Arc<StreamDefinition>> {
        let key = (org_id.0.clone(), name.to_string(), stream_type);
        self.inner.read().get(&key).cloned()
    }

    pub fn put(&self, def: Arc<StreamDefinition>) {
        let key = (def.org_id.0.clone(), def.name.clone(), def.stream_type);
        self.inner.write().insert(key, def);
    }

    pub fn invalidate_for(&self, org_id: &Id, name: &str, stream_type: StreamType) {
        let key = (org_id.0.clone(), name.to_string(), stream_type);
        self.inner.write().remove(&key);
    }

    pub fn invalidate_org(&self, org_id: &Id) {
        let mut guard = self.inner.write();
        guard.retain(|(o, _, _), _| o != &org_id.0);
    }

    /// 仅供测试/诊断：当前缓存大小。
    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::stream::{Retention, Schema},
        shared::time::TimestampMicros,
    };

    fn mk(org: &str, name: &str, t: StreamType) -> Arc<StreamDefinition> {
        Arc::new(StreamDefinition {
            id: Id::new(),
            org_id: Id(org.into()),
            name: name.into(),
            stream_type: t,
            schema: Schema { fields: vec![] },
            retention: Some(Retention { days: 7 }),
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        })
    }

    #[test]
    fn put_get_roundtrips() {
        let c = OrgSchemaCache::new();
        let s = mk("orgA", "logs", StreamType::Logs);
        c.put(s.clone());
        let got = c.get(&Id("orgA".into()), "logs", StreamType::Logs).unwrap();
        assert_eq!(got.id.0, s.id.0);
    }

    #[test]
    fn invalidate_for_removes_one() {
        let c = OrgSchemaCache::new();
        c.put(mk("orgA", "logs", StreamType::Logs));
        c.put(mk("orgA", "metrics", StreamType::Metrics));
        c.invalidate_for(&Id("orgA".into()), "logs", StreamType::Logs);
        assert!(
            c.get(&Id("orgA".into()), "logs", StreamType::Logs)
                .is_none()
        );
        assert!(
            c.get(&Id("orgA".into()), "metrics", StreamType::Metrics)
                .is_some()
        );
    }

    #[test]
    fn invalidate_org_clears_org() {
        let c = OrgSchemaCache::new();
        c.put(mk("orgA", "logs", StreamType::Logs));
        c.put(mk("orgB", "logs", StreamType::Logs));
        c.invalidate_org(&Id("orgA".into()));
        assert!(
            c.get(&Id("orgA".into()), "logs", StreamType::Logs)
                .is_none()
        );
        assert!(
            c.get(&Id("orgB".into()), "logs", StreamType::Logs)
                .is_some()
        );
    }
}
