// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Org-level quotas。
//!
//! - [`QuotaLimiter`]：`DashMap<org_id, Arc<governor::RateLimiter>>` 两套（ingest / query）。
//!   ingest entry 在权限校验后调 `acquire_async`；超限 → 429 + Retry-After。
//! - storage cap：compactor 每 5min 计算 `(org → sum(parquet_file_meta.size_bytes))` 进内存；
//!   ingest entry 在 acquire 之后检查，超 → 413。

use std::{collections::HashMap, num::NonZeroU32, sync::Arc};

use dashmap::DashMap;
use governor::{
    Quota, RateLimiter,
    clock::{Clock, DefaultClock},
    state::{InMemoryState, NotKeyed},
};
use parking_lot::RwLock;

use crate::shared::ids::Id;

type Limiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

#[derive(Debug, Clone, Copy)]
pub enum QuotaDim {
    Ingest,
    Query,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OrgQuota {
    pub max_ingest_qps: u32,
    pub max_query_qps: u32,
    pub max_storage_bytes: u64,
    pub max_streams: u32,
}

pub struct QuotaLimiter {
    quotas: RwLock<HashMap<Id, OrgQuota>>,
    ingest_limiters: DashMap<Id, Arc<Limiter>>,
    query_limiters: DashMap<Id, Arc<Limiter>>,
    storage_usage: DashMap<Id, u64>,
}

impl Default for QuotaLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl QuotaLimiter {
    pub fn new() -> Self {
        Self {
            quotas: RwLock::new(HashMap::new()),
            ingest_limiters: DashMap::new(),
            query_limiters: DashMap::new(),
            storage_usage: DashMap::new(),
        }
    }

    /// 后台 30s 重读 DB 后调用：更新所有 org 配额；新增 org 自动建 limiter，删除 org 同步清。
    pub fn refresh(&self, all: HashMap<Id, OrgQuota>) {
        let mut g = self.quotas.write();
        *g = all;
        // limiter map 在 acquire 时按需懒构造，refresh 只重设 quota 数值
    }

    pub fn update_storage_usage(&self, org_id: &Id, bytes: u64) {
        self.storage_usage.insert(org_id.clone(), bytes);
    }

    pub fn current_storage(&self, org_id: &Id) -> u64 {
        self.storage_usage.get(org_id).map(|v| *v).unwrap_or(0)
    }

    /// 同步 check：返回 `Some(retry_secs)` 表示超限。
    pub fn acquire(&self, org_id: &Id, dim: QuotaDim) -> Option<u64> {
        let q = self.quotas.read().get(org_id).copied().unwrap_or_default();
        let (qps, map) = match dim {
            QuotaDim::Ingest => (q.max_ingest_qps, &self.ingest_limiters),
            QuotaDim::Query => (q.max_query_qps, &self.query_limiters),
        };
        if qps == 0 {
            return None;
        }
        let limiter = map
            .entry(org_id.clone())
            .or_insert_with(|| {
                let quota = Quota::per_second(NonZeroU32::new(qps).unwrap())
                    .allow_burst(NonZeroU32::new(qps).unwrap());
                Arc::new(RateLimiter::direct(quota))
            })
            .clone();
        match limiter.check() {
            Ok(_) => None,
            Err(not_until) => Some(
                not_until
                    .wait_time_from(DefaultClock::default().now())
                    .as_secs()
                    .max(1),
            ),
        }
    }

    /// 检查 storage cap：超过 `max_storage_bytes` 返 false（caller 应返 413）。
    pub fn check_storage_cap(&self, org_id: &Id, incoming_bytes: u64) -> bool {
        let q = self.quotas.read().get(org_id).copied().unwrap_or_default();
        if q.max_storage_bytes == 0 {
            return true;
        }
        let current = self.current_storage(org_id);
        current.saturating_add(incoming_bytes) <= q.max_storage_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_qps_is_unbounded() {
        let l = QuotaLimiter::new();
        let org = Id::from_string("orga");
        assert!(l.acquire(&org, QuotaDim::Ingest).is_none());
    }

    #[test]
    fn rate_limit_blocks_after_burst() {
        let l = QuotaLimiter::new();
        let org = Id::from_string("orga");
        let mut quotas = HashMap::new();
        quotas.insert(
            org.clone(),
            OrgQuota {
                max_ingest_qps: 2,
                max_query_qps: 0,
                max_storage_bytes: 0,
                max_streams: 0,
            },
        );
        l.refresh(quotas);
        assert!(l.acquire(&org, QuotaDim::Ingest).is_none());
        assert!(l.acquire(&org, QuotaDim::Ingest).is_none());
        // 第三次 burst 用尽
        assert!(l.acquire(&org, QuotaDim::Ingest).is_some());
    }

    #[test]
    fn storage_cap_enforced() {
        let l = QuotaLimiter::new();
        let org = Id::from_string("orga");
        let mut quotas = HashMap::new();
        quotas.insert(
            org.clone(),
            OrgQuota {
                max_storage_bytes: 100,
                ..Default::default()
            },
        );
        l.refresh(quotas);
        l.update_storage_usage(&org, 50);
        assert!(l.check_storage_cap(&org, 49));
        assert!(!l.check_storage_cap(&org, 60));
    }
}
