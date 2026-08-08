// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Per-org 脱敏规则解析 + 缓存（写入端热路径用）。
//!
//! [`MaskingService`] 包 [`RegexPatternRepository`]，把该 org 标了 `apply_on_ingest` 的
//! regex pattern 编译成 [`Masker`]（domain 纯逻辑）并按 org 进 moka 缓存（短 TTL）。
//! `IngestService` 每批调 [`MaskingService::ingest_masker`]：空规则集返回空 masker，
//! 据此零开销跳过；规则增删改后路由层调 [`MaskingService::invalidate`] 即时失效。
//!
//! 查询端 `mask(col)` UDF 不走本服务（仅 SQL 命中 `mask(` 时按 org 现取规则编译，
//! 与 `extract_pattern` 一致），故这里只服务写入路径。

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use moka::future::Cache;

use crate::{
    domain::masking::{Masker, MaskingProvider},
    infra::persistence::repositories::regex_patterns::RegexPatternRepository,
    shared::{Result, ids::Id},
};

mod field;

pub use field::FieldMaskingService;

/// 解析缓存 TTL（秒）：规则增删改在此窗口内对所有读者最终一致（本进程内由 invalidate 即时生效）。
const CACHE_TTL_SECS: u64 = 60;

/// 脱敏规则服务：per-org 编译 + 缓存写入端 [`Masker`]。
pub struct MaskingService {
    patterns: Arc<dyn RegexPatternRepository>,
    cache: Cache<String, Arc<Masker>>,
}

impl MaskingService {
    pub fn new(patterns: Arc<dyn RegexPatternRepository>) -> Self {
        Self {
            patterns,
            cache: Cache::builder()
                .max_capacity(4096)
                .time_to_live(Duration::from_secs(CACHE_TTL_SECS))
                .build(),
        }
    }

    /// 失效该 org 的缓存：regex pattern 增删改后调用，使下次 `ingest_masker` 立刻重读。
    pub async fn invalidate(&self, org: &Id) {
        self.cache.invalidate(&org.0).await;
    }
}

#[async_trait]
impl MaskingProvider for MaskingService {
    async fn ingest_masker(&self, org_id: &Id) -> Result<Arc<Masker>> {
        if let Some(m) = self.cache.get(&org_id.0).await {
            return Ok(m);
        }
        let rows = self.patterns.list(org_id).await?;
        let masker = Arc::new(Masker::compile(
            rows.into_iter()
                .filter(|p| p.apply_on_ingest)
                .map(|p| (p.pattern, p.replacement)),
        ));
        self.cache.insert(org_id.0.clone(), masker.clone()).await;
        Ok(masker)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use super::*;
    use crate::{
        infra::persistence::repositories::regex_patterns::RegexPattern,
        shared::time::TimestampMicros,
    };

    /// 进程内 RegexPatternRepository，仅测试用。
    #[derive(Default)]
    struct InMemPatterns {
        rows: StdMutex<Vec<RegexPattern>>,
    }

    #[async_trait]
    impl RegexPatternRepository for InMemPatterns {
        async fn list(&self, org_id: &Id) -> Result<Vec<RegexPattern>> {
            let mut v: Vec<RegexPattern> = self
                .rows
                .lock()
                .unwrap()
                .iter()
                .filter(|r| &r.org_id == org_id)
                .cloned()
                .collect();
            v.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(v)
        }
        async fn create(&self, p: RegexPattern) -> Result<RegexPattern> {
            self.rows.lock().unwrap().push(p.clone());
            Ok(p)
        }
        async fn update(&self, p: RegexPattern) -> Result<RegexPattern> {
            let mut rows = self.rows.lock().unwrap();
            if let Some(slot) = rows
                .iter_mut()
                .find(|r| r.id == p.id && r.org_id == p.org_id)
            {
                *slot = p.clone();
            }
            Ok(p)
        }
        async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
            self.rows
                .lock()
                .unwrap()
                .retain(|r| !(&r.org_id == org_id && &r.id == id));
            Ok(())
        }
    }

    fn pat(org: &Id, name: &str, pattern: &str, apply_on_ingest: bool) -> RegexPattern {
        RegexPattern {
            id: Id::new(),
            org_id: org.clone(),
            name: name.into(),
            pattern: pattern.into(),
            description: String::new(),
            replacement: "[X]".into(),
            apply_on_ingest,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[tokio::test]
    async fn ingest_masker_only_includes_ingest_flagged() {
        let repo = Arc::new(InMemPatterns::default());
        let org = Id::from_string("org-1");
        repo.create(pat(&org, "ssn", r"\d{3}-\d{2}-\d{4}", true))
            .await
            .unwrap();
        // query-only 规则不该进写入端 masker。
        repo.create(pat(&org, "email", r"\w+@\w+", false))
            .await
            .unwrap();
        let svc = MaskingService::new(repo);

        let m = svc.ingest_masker(&org).await.unwrap();
        assert_eq!(m.mask_str("ssn 123-45-6789"), "ssn [X]");
        // email 规则未启用写入脱敏 → 原样保留。
        assert_eq!(m.mask_str("a@b"), "a@b");
    }

    #[tokio::test]
    async fn no_ingest_rules_yields_empty_masker() {
        let repo = Arc::new(InMemPatterns::default());
        let org = Id::from_string("org-1");
        repo.create(pat(&org, "email", r"\w+@\w+", false))
            .await
            .unwrap();
        let svc = MaskingService::new(repo);
        assert!(svc.ingest_masker(&org).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn invalidate_picks_up_new_rule() {
        let repo = Arc::new(InMemPatterns::default());
        let org = Id::from_string("org-1");
        let svc = MaskingService::new(repo.clone());

        // 首次：无规则 → 空 masker（已缓存）。
        assert!(svc.ingest_masker(&org).await.unwrap().is_empty());
        repo.create(pat(&org, "ssn", r"\d{3}-\d{2}-\d{4}", true))
            .await
            .unwrap();
        // 不失效仍读到旧的空 masker。
        assert!(svc.ingest_masker(&org).await.unwrap().is_empty());
        // 失效后立刻反映新规则。
        svc.invalidate(&org).await;
        assert_eq!(
            svc.ingest_masker(&org)
                .await
                .unwrap()
                .mask_str("123-45-6789"),
            "[X]"
        );
    }
}
