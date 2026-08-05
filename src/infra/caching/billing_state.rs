// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 订阅门禁判定缓存（热路径优化）。
//!
//! 计费启用时，ingest gate 每批本应查一次 `marketplace_subscriptions` 算出"该 org 的
//! 订阅是否拦截写入"。本缓存把它降到**每 org 每 TTL 一次**：
//!
//! - `org_id → blocked(bool)`，moka `time_to_live` 兜底陈旧（TTL 内状态变更最多延迟生效）；
//! - webhook（Stripe / AWS / Azure）upsert 订阅后调 [`invalidate`](Self::invalidate)，
//!   让状态变更**即时**生效，不必等 TTL。
//!
//! 缓存只做 KV，miss 时由 caller 查 repo 填回——与 [`super::OrgSchemaCache`] 同模式。

use std::time::Duration;

use moka::future::Cache;

use crate::shared::ids::Id;

/// TTL：webhook 精确失效是主路径，TTL 仅作兜底（如 license 直改 DB 等旁路变更）。
const TTL_SECS: u64 = 30;
/// 容量上限：每 org 一条，远超常见租户数即可。
const CAPACITY: u64 = 50_000;

pub struct BillingStateCache {
    inner: Cache<String, bool>,
}

impl BillingStateCache {
    pub fn new() -> Self {
        Self {
            inner: Cache::builder()
                .max_capacity(CAPACITY)
                .time_to_live(Duration::from_secs(TTL_SECS))
                .build(),
        }
    }

    /// 命中返回缓存的 blocked 判定；未命中 / 已过期返回 None。
    pub async fn get(&self, org_id: &Id) -> Option<bool> {
        self.inner.get(&org_id.0).await
    }

    /// 写入某 org 的 blocked 判定。
    pub async fn put(&self, org_id: &Id, blocked: bool) {
        self.inner.insert(org_id.0.clone(), blocked).await;
    }

    /// 撤销某 org 的缓存（订阅状态变更后调，确保即时生效）。
    pub async fn invalidate(&self, org_id: &Id) {
        self.inner.invalidate(&org_id.0).await;
    }
}

impl Default for BillingStateCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_get_invalidate_roundtrip() {
        let cache = BillingStateCache::new();
        let org = Id::from_string("org-1");
        assert_eq!(cache.get(&org).await, None);
        cache.put(&org, true).await;
        assert_eq!(cache.get(&org).await, Some(true));
        cache.invalidate(&org).await;
        // moka invalidate 是异步生效；run_pending 后确保已撤销。
        cache.inner.run_pending_tasks().await;
        assert_eq!(cache.get(&org).await, None);
    }
}
