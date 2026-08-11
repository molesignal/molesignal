// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 慢查询周期分析 worker：定期遍历各 org 最近的慢查询，跑 [`crate::app::recommendations`]
//! 启发式引擎，对带 warning/critical 建议的慢查询发结构化日志事件（ops 可见 / 未来可接告警）。
//!
//! 单点周期任务（与告警后台同属 alert_manager 角色，只起一份）。分析逻辑复用 app 引擎
//! （纯函数），worker 只负责编排 + 发事件，不改查询、不写新表。

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    app::recommendations::{QueryProfile, RecommendationSeverity, analyze},
    domain::{iam::OrganizationRepository, query::SlowQueryRepository},
    shared::Result,
};

#[derive(Debug, Clone)]
pub struct SlowQueryAnalyzerConfig {
    pub interval_secs: u64,
    /// 每 org 每 tick 最多分析多少条最近慢查询。
    pub per_org_limit: i64,
}

impl Default for SlowQueryAnalyzerConfig {
    fn default() -> Self {
        Self {
            interval_secs: 300,
            per_org_limit: 50,
        }
    }
}

pub struct SlowQueryAnalyzer {
    orgs: Arc<dyn OrganizationRepository>,
    slow_queries: Arc<dyn SlowQueryRepository>,
    cfg: SlowQueryAnalyzerConfig,
}

impl SlowQueryAnalyzer {
    pub fn new(
        orgs: Arc<dyn OrganizationRepository>,
        slow_queries: Arc<dyn SlowQueryRepository>,
        cfg: SlowQueryAnalyzerConfig,
    ) -> Self {
        Self {
            orgs,
            slow_queries,
            cfg,
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut tick =
                tokio::time::interval(Duration::from_secs(self.cfg.interval_secs.max(1)));
            tick.tick().await; // skip 首次（与其它周期任务一致）
            loop {
                tick.tick().await;
                if let Err(e) = self.sweep_once().await {
                    tracing::warn!(error = %e, "slow query analysis sweep failed");
                }
            }
        })
    }

    #[tracing::instrument(
        name = "worker.slow_query_analyzer",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "slow_query_analyzer")
    )]
    async fn sweep_once(&self) -> Result<()> {
        let orgs = self.orgs.list().await?;
        for org in orgs {
            let slow = self
                .slow_queries
                .list_recent(&org.id, self.cfg.per_org_limit)
                .await?;
            for sq in &slow {
                let profile = QueryProfile::from(sq);
                let recs = analyze(&profile);
                let high: Vec<&str> = recs
                    .iter()
                    .filter(|r| {
                        matches!(
                            r.severity,
                            RecommendationSeverity::Warning | RecommendationSeverity::Critical
                        )
                    })
                    .map(|r| r.code.as_str())
                    .collect();
                if !high.is_empty() {
                    tracing::info!(
                        org = %org.id.0,
                        fingerprint = %sq.fingerprint,
                        took_ms = sq.took_ms,
                        scanned_rows = sq.scanned_rows,
                        hits = sq.hit_count,
                        recommendations = ?high,
                        "slow query with optimization recommendations"
                    );
                }
            }
        }
        Ok(())
    }
}
