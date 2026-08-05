// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Service graph 存储重算 worker（`[service_graph].source = storage` 模式）。
//!
//! 跨节点正确性方案：路由层无法按 trace_id 分片（批量转发不解析、一批混多 trace），故同一 trace
//! 的父子 span 可能落到不同 ingest 节点、各自的内存聚合器配不上对。本 worker 作为**单例**
//! （alert_manager 角色）周期从**共享存储**重算——查询近窗口的 trace span（此时见到完整集合，
//! 无视哪个节点接入），复用 [`ServiceGraphAggregator`] 配对出 caller→callee 边，**先删后插**
//! 窗口内的边实现幂等（重算覆盖、含晚到 span）。
//!
//! 是否启用由 DB 实例设置 `service_graph_source` 决定（前端设置-通用页可改，运行时生效）：
//! `storage` 才工作；`ingest` 时本 worker 空转、由各进程的 flush worker 落库。

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    app::query::QueryService,
    domain::{
        iam::{InstanceSettingsRepository, OrganizationRepository},
        query::{QueryLanguage, QueryRequest, StreamHint},
        stream::{StreamRepository, StreamType},
    },
    infra::traces::{ServiceGraphAggregator, ServiceGraphRepository},
    shared::{
        Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

const BUCKET_SIZE_US: i64 = 60 * 1_000_000;

#[derive(Debug, Clone)]
pub struct ServiceGraphRecomputeConfig {
    /// 重算周期秒。
    pub interval_secs: u64,
    /// 每轮回看窗口秒（覆盖晚到 + flush 延迟；窗口内桶先删后插，幂等）。
    pub lookback_secs: u64,
    /// 单流单轮查询行上限（控内存）。
    pub max_rows: usize,
}

impl Default for ServiceGraphRecomputeConfig {
    fn default() -> Self {
        Self {
            interval_secs: 60,
            lookback_secs: 300,
            max_rows: 200_000,
        }
    }
}

pub struct ServiceGraphRecomputer {
    query: Arc<QueryService>,
    orgs: Arc<dyn OrganizationRepository>,
    streams: Arc<dyn StreamRepository>,
    repo: Arc<dyn ServiceGraphRepository>,
    instance_settings: Arc<dyn InstanceSettingsRepository>,
    cfg: ServiceGraphRecomputeConfig,
}

impl ServiceGraphRecomputer {
    pub fn new(
        query: Arc<QueryService>,
        orgs: Arc<dyn OrganizationRepository>,
        streams: Arc<dyn StreamRepository>,
        repo: Arc<dyn ServiceGraphRepository>,
        instance_settings: Arc<dyn InstanceSettingsRepository>,
        cfg: ServiceGraphRecomputeConfig,
    ) -> Self {
        Self {
            query,
            orgs,
            streams,
            repo,
            instance_settings,
            cfg,
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(self.cfg.interval_secs));
            loop {
                tick.tick().await;
                if let Err(e) = self.recompute_once().await {
                    tracing::warn!(error = %e, "service graph recompute failed");
                }
            }
        })
    }

    #[tracing::instrument(
        name = "worker.service_graph_recompute",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "service_graph_recompute")
    )]
    async fn recompute_once(&self) -> Result<()> {
        // 仅 storage 模式工作；ingest 模式由各进程 flush worker 落库。
        let storage = self
            .instance_settings
            .get()
            .await
            .map(|s| s.service_graph_storage_mode())
            .unwrap_or(false);
        if !storage {
            return Ok(());
        }
        let now = TimestampMicros::now().0;
        let from = now - (self.cfg.lookback_secs as i64) * 1_000_000;
        let from_bucket = (from / BUCKET_SIZE_US) * BUCKET_SIZE_US;

        for org in self.orgs.list().await? {
            let traces: Vec<String> = self
                .streams
                .list(&org.id)
                .await
                .unwrap_or_default()
                .into_iter()
                .filter(|s| s.stream_type == StreamType::Traces)
                .map(|s| s.name)
                .collect();
            if traces.is_empty() {
                continue;
            }
            let agg = ServiceGraphAggregator::new();
            let mut queried_ok = false;
            for stream in &traces {
                match self.query_into(&org.id, stream, from, now, &agg).await {
                    Ok(()) => queried_ok = true,
                    // 缺列（如无 parent_span_id）/查询失败 → 跳过该流（debug，不噪）。
                    Err(e) => tracing::debug!(
                        error = %e, org = %org.id.0, stream = %stream,
                        "service graph recompute: trace query skipped"
                    ),
                }
            }
            // 全部流查询失败 → 不删既有边（避免误清）。
            if !queried_ok {
                continue;
            }
            let snaps = agg.flush_due(i64::MAX);
            // 窗口幂等：先删窗口内既有边，再插重算结果（snaps 为空 → 仅清空窗口、正确）。
            self.repo.delete_range(&org.id, from_bucket, now).await?;
            if !snaps.is_empty() {
                self.repo.insert_many(&snaps).await?;
            }
        }
        Ok(())
    }

    /// 查一个 traces 流的窗口 span，喂给聚合器。
    async fn query_into(
        &self,
        org_id: &Id,
        stream: &str,
        from: i64,
        to: i64,
        agg: &ServiceGraphAggregator,
    ) -> Result<()> {
        // 流名含引号/反斜杠 → 跳过（防 SQL 注入；正常流名不含）。
        if stream.contains('"') || stream.contains('\\') {
            return Ok(());
        }
        let sql = format!(
            "SELECT _timestamp, trace_id, span_id, parent_span_id, \"service.name\", \
                    \"service.namespace\", \"molesignal.execution.role\", duration_ns, status_code \
             FROM \"{stream}\" WHERE _timestamp BETWEEN {from} AND {to}"
        );
        let req = QueryRequest {
            org_id: org_id.clone(),
            language: QueryLanguage::Sql,
            statement: sql,
            time_range: TimeRange::new(TimestampMicros(from), TimestampMicros(to)),
            stream: Some(StreamHint {
                name: stream.to_string(),
                stream_type: StreamType::Traces,
            }),
            limit: Some(self.cfg.max_rows),
            federation_clusters: Vec::new(),
        };
        let result = self.query.run(req).await?;
        for row in &result.rows {
            if row.len() < 9 {
                continue;
            }
            let ts = TimestampMicros(row[0].as_i64().unwrap_or(0));
            let Some(trace_id) = row[1].as_str().filter(|s| !s.is_empty()) else {
                continue;
            };
            let Some(span_id) = row[2].as_str().filter(|s| !s.is_empty()) else {
                continue;
            };
            let parent = row[3].as_str().filter(|s| !s.is_empty());
            let service = crate::infra::traces::service_graph::effective_service_name(
                row[4].as_str(),
                row[5].as_str(),
                row[6].as_str(),
            );
            let duration_us = row[7]
                .as_i64()
                .or_else(|| row[7].as_u64().map(|u| u as i64))
                .unwrap_or(0)
                / 1_000;
            let is_error = row[8]
                .as_str()
                .map(|s| s.eq_ignore_ascii_case("ERROR"))
                .unwrap_or(false);
            agg.observe_span(
                org_id,
                trace_id,
                span_id,
                parent,
                &service,
                ts,
                duration_us,
                is_error,
            );
        }
        Ok(())
    }
}
