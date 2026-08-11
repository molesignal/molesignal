// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Query orchestration + in-process query registry。
//!
//! `QueryService` 是 SQL/PromQL 引擎的瘦封装。`backend-settings-endpoints` 引入：
//! - [`QueryRegistry`] —— in-flight 查询表，用于 `/api/v1/query/running` 列表。
//! - cancel 标志位 —— 客户端 `POST /query/{id}/cancel` 翻 `AtomicBool`，best-effort 通知正在执行的查询。
//!   DataFusion 端的批量中断在引擎侧逐步接入；当前 stub 完成注册/反注册和 cancel API。

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use parking_lot::RwLock;
use serde::Serialize;

use crate::{
    app::search::AdmissionController,
    domain::{
        masking::FieldMaskingProvider,
        metrics::PrometheusExemplarQueryResult,
        query::{
            PromqlEngine, QueryEngine, QueryLanguage, QueryRequest, QueryResult,
            QueryResultCachePort,
        },
        storage::PhysicalDatasetKind,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Clone)]
pub struct ActiveQuery {
    pub id: Id,
    pub org_id: Id,
    pub user_id: Id,
    pub statement: String,
    pub started_at: TimestampMicros,
    pub cancel: Arc<AtomicBool>,
    /// 联邦查询的跨集群 id（#12）；非联邦为 `None`。cancel 路由据此向远端 fan-out
    /// `CancelQuery(fed_id)`。
    pub federation_query_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActiveQuerySnapshot {
    pub id: String,
    pub org_id: String,
    pub user_id: String,
    pub statement: String,
    pub started_at_micros: i64,
    pub cancelled: bool,
}

pub struct QueryRegistry {
    inner: RwLock<HashMap<String, ActiveQuery>>,
}

impl Default for QueryRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl QueryRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }

    fn insert(&self, q: ActiveQuery) {
        self.inner.write().insert(q.id.0.clone(), q);
    }

    fn remove(&self, id: &str) {
        self.inner.write().remove(id);
    }

    pub fn list_for(&self, org_id: Option<&Id>) -> Vec<ActiveQuerySnapshot> {
        let g = self.inner.read();
        g.values()
            .filter(|q| match org_id {
                Some(o) => &q.org_id == o,
                None => true,
            })
            .map(|q| ActiveQuerySnapshot {
                id: q.id.0.clone(),
                org_id: q.org_id.0.clone(),
                user_id: q.user_id.0.clone(),
                statement: q.statement.clone(),
                started_at_micros: q.started_at.0,
                cancelled: q.cancel.load(Ordering::Relaxed),
            })
            .collect()
    }

    /// 返回命中条目的 org_id（用于 ACL 校验）和 cancel handle。
    pub fn lookup_org(&self, id: &str) -> Option<Id> {
        self.inner.read().get(id).map(|q| q.org_id.clone())
    }

    /// 命中条目的跨集群查询 id（联邦查询才有）；cancel 路由用它向远端 fan-out 取消。
    pub fn federation_query_id(&self, id: &str) -> Option<String> {
        self.inner
            .read()
            .get(id)
            .and_then(|q| q.federation_query_id.clone())
    }

    pub fn cancel(&self, id: &str) -> Result<()> {
        let g = self.inner.read();
        match g.get(id) {
            Some(q) => {
                q.cancel.store(true, Ordering::Relaxed);
                Ok(())
            }
            None => Err(Error::not_found("query not found")),
        }
    }
}

pub struct QueryService {
    sql: Arc<dyn QueryEngine>,
    promql: Arc<dyn PromqlEngine>,
    registry: Arc<QueryRegistry>,
    admission: Arc<AdmissionController>,
    /// 整结果级缓存；只在 [`Self::run_tracked`] 生效——缓存键含角色，而只有那条路径
    /// 拿得到 `Role`。未注入时全部直通。
    result_cache: Option<Arc<dyn QueryResultCachePort>>,
    field_masking: Option<Arc<dyn FieldMaskingProvider>>,
}

impl QueryService {
    pub fn new(
        sql: Arc<dyn QueryEngine>,
        promql: Arc<dyn PromqlEngine>,
        admission: Arc<AdmissionController>,
    ) -> Self {
        Self {
            sql,
            promql,
            registry: Arc::new(QueryRegistry::new()),
            admission,
            result_cache: None,
            field_masking: None,
        }
    }

    /// 注入整结果缓存。缓存只覆盖时间窗已封闭的查询（新鲜窗口内的查询直通不缓存），
    /// 所以不会让实时面板读到陈旧数据。
    pub fn with_result_cache(mut self, cache: Arc<dyn QueryResultCachePort>) -> Self {
        self.result_cache = Some(cache);
        self
    }

    pub fn with_field_masking(mut self, masking: Arc<dyn FieldMaskingProvider>) -> Self {
        self.field_masking = Some(masking);
        self
    }

    pub fn registry(&self) -> Arc<QueryRegistry> {
        self.registry.clone()
    }

    /// 准入控制器（`GET /query/admission` 读快照）。
    pub fn admission(&self) -> Arc<AdmissionController> {
        self.admission.clone()
    }

    pub async fn run(&self, req: QueryRequest) -> Result<QueryResult> {
        let mut result = self.run_raw(req.clone()).await?;
        self.mask_result(&req, &mut result).await?;
        Ok(result)
    }

    /// 可信内部计算入口：返回原始值。仅供需要继续聚合或派生数据的 worker 使用。
    pub(crate) async fn run_raw(&self, req: QueryRequest) -> Result<QueryResult> {
        match req.language {
            QueryLanguage::Sql => self.sql.execute(req).await,
            QueryLanguage::Promql => self.promql.execute(req).await,
        }
    }

    /// 产品内部的窄物理读模型查询，不对通用查询 API 暴露数据集选择。
    pub async fn run_dataset(
        &self,
        req: QueryRequest,
        dataset_kind: PhysicalDatasetKind,
    ) -> Result<QueryResult> {
        if req.language != QueryLanguage::Sql {
            return Err(Error::invalid(
                "physical dataset selection is only available for SQL",
            ));
        }
        let mut result = self.sql.execute_dataset(req.clone(), dataset_kind).await?;
        self.mask_result(&req, &mut result).await?;
        Ok(result)
    }

    /// Prometheus `query_exemplars` 使用同一查询准入池，但不进入整结果缓存。
    ///
    /// Exemplar 响应依赖刚写入的 trace correlation labels，缓存会让指标图上的 Trace
    /// 跳转滞后；底层查询自身有硬结果上限。
    pub async fn run_exemplars(
        &self,
        req: QueryRequest,
        role_key: &str,
    ) -> Result<PrometheusExemplarQueryResult> {
        let _slot = self.admission.acquire_for_role(role_key).ok_or_else(|| {
            Error::resource_exhausted(
                "search admission: too many concurrent queries for your work group; retry shortly",
            )
        })?;
        let mut result = self.promql.query_exemplars(req.clone()).await?;
        if let Some(masking) = &self.field_masking {
            masking.mask_exemplars(&req, &mut result).await?;
        }
        Ok(result)
    }

    /// 规划并返回优化后逻辑计划文本（search inspector 用，不执行）。仅 SQL 支持。
    pub async fn explain(&self, req: QueryRequest) -> Result<String> {
        match req.language {
            QueryLanguage::Sql => self.sql.explain(req).await,
            QueryLanguage::Promql => {
                Err(Error::invalid("explain is only available for SQL queries"))
            }
        }
    }

    /// 准入 + 注册 + 执行 + 反注册。`/api/v1/query` / `/api/v1/query/stream` 与 Flight SQL
    /// 调用，让 `/api/v1/query/running` 能看到这条查询。`role` 决定准入工作组。
    ///
    /// 先按角色申请并发槽位（[`AdmissionController`]）：满则返 429（`ResourceExhausted`），
    /// 不进入执行；槽位 RAII guard 持有到本次执行结束自动释放。
    #[tracing::instrument(
        name = "query.execute",
        skip_all,
        fields(
            otel.kind = "internal",
            molesignal.query.language = ?req.language,
            molesignal.query.federated = req.federation_clusters.iter().any(|cluster| !cluster.eq_ignore_ascii_case("local"))
        )
    )]
    pub async fn run_tracked(
        &self,
        req: QueryRequest,
        user_id: Id,
        role_key: &str,
    ) -> Result<QueryResult> {
        let _slot = self.admission.acquire_for_role(role_key).ok_or_else(|| {
            Error::resource_exhausted(
                "search admission: too many concurrent queries for your work group; retry shortly",
            )
        })?;
        let id = Id::new();
        let cancel = Arc::new(AtomicBool::new(false));
        // 仅联邦查询（目标含非 local 集群）生成跨集群 id，使远端子查询可经 CancelQuery 取消。
        let fed_id = req
            .federation_clusters
            .iter()
            .any(|c| !c.eq_ignore_ascii_case("local"))
            .then(|| Id::new().0);
        let entry = ActiveQuery {
            id: id.clone(),
            org_id: req.org_id.clone(),
            user_id,
            statement: req.statement.clone(),
            started_at: TimestampMicros::now(),
            cancel: cancel.clone(),
            federation_query_id: fed_id.clone(),
        };
        self.registry.insert(entry);
        struct Guard<'a> {
            reg: &'a QueryRegistry,
            id: String,
        }
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.reg.remove(&self.id);
            }
        }
        let _g = Guard {
            reg: &self.registry,
            id: id.0.clone(),
        };

        let Some(cache) = self.result_cache.clone() else {
            let mut result = self.run_cancellable(req.clone(), &cancel, fed_id).await?;
            self.mask_result(&req, &mut result).await?;
            return Ok(result);
        };
        // 角色进缓存键：不同角色可见的数据范围不同，不能共用一份结果。
        // 时间窗未封闭时 get/put 都是 no-op，实时查询照常直通。
        let role_filter = role_key.to_string();
        let now = TimestampMicros::now().0;
        if let Some(mut hit) = cache.get(&req, &role_filter, now).await {
            self.mask_result(&req, &mut hit).await?;
            return Ok(hit);
        }
        let raw = self.run_cancellable(req.clone(), &cancel, fed_id).await?;
        cache.put(&req, &role_filter, now, raw.clone()).await;
        let mut result = raw;
        self.mask_result(&req, &mut result).await?;
        Ok(result)
    }

    /// 按 language 派发，SQL 路径带上 `fed_id`（联邦引擎据此让远端子查询可被 CancelQuery 取消）。
    async fn run_fed(&self, req: QueryRequest, fed_id: Option<String>) -> Result<QueryResult> {
        match req.language {
            QueryLanguage::Sql => self.sql.execute_federated(req, fed_id).await,
            QueryLanguage::Promql => self.promql.execute(req).await,
        }
    }

    /// 把查询 future 与一个轮询 cancel 标志的计时器 race，实现**真中断**：cancel 置位时
    /// 丢弃查询 future —— DataFusion 的 RecordBatch 收集流停止被 poll（合作式取消，批间
    /// 粒度），联邦扇出的远端连接也随之关闭。返回 499 cancelled，而非跑完才丢结果。
    /// SQL / PromQL 两路统一适用。
    async fn run_cancellable(
        &self,
        req: QueryRequest,
        cancel: &Arc<AtomicBool>,
        fed_id: Option<String>,
    ) -> Result<QueryResult> {
        let run_fut = self.run_fed(req, fed_id);
        tokio::pin!(run_fut);
        loop {
            tokio::select! {
                biased;
                out = &mut run_fut => return out,
                _ = tokio::time::sleep(CANCEL_POLL_INTERVAL) => {
                    if cancel.load(Ordering::Relaxed) {
                        return Err(Error::cancelled("query cancelled"));
                    }
                }
            }
        }
    }

    async fn mask_result(&self, req: &QueryRequest, result: &mut QueryResult) -> Result<()> {
        if let Some(masking) = &self.field_masking {
            masking.mask_result(req, result).await?;
        }
        Ok(())
    }
}

/// cancel 标志轮询间隔：取消最坏延迟 ≤ 此值（批间粒度已足够，不必更密）。
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::{
        app::search::AdmissionConfig,
        domain::query::{FederationMeta, QueryResult},
        shared::time::{TimeRange, TimestampMicros},
    };

    /// 模拟一条很慢的查询：sleep 远超测试预算后才返回。cancel 必须在 sleep 完成前中断它。
    struct SlowEngine;
    #[async_trait]
    impl QueryEngine for SlowEngine {
        async fn execute(&self, _req: QueryRequest) -> Result<QueryResult> {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok(QueryResult {
                columns: vec![],
                rows: vec![],
                scanned_rows: 0,
                took_ms: 0,
                federation: None::<FederationMeta>,
            })
        }
    }

    struct NoPromql;
    #[async_trait]
    impl PromqlEngine for NoPromql {
        async fn execute(&self, _req: QueryRequest) -> Result<QueryResult> {
            Err(Error::invalid("no promql"))
        }
    }

    fn sql_req() -> QueryRequest {
        QueryRequest {
            org_id: Id::from_string("org"),
            language: QueryLanguage::Sql,
            statement: "SELECT 1".into(),
            time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(1)),
            stream: None,
            limit: None,
            federation_clusters: vec![],
        }
    }

    #[tokio::test]
    async fn cancel_aborts_running_query_early() {
        let svc = Arc::new(QueryService::new(
            Arc::new(SlowEngine),
            Arc::new(NoPromql),
            Arc::new(AdmissionController::new(AdmissionConfig::default())),
        ));
        let registry = svc.registry();
        let svc2 = svc.clone();
        let handle = tokio::spawn(async move {
            svc2.run_tracked(sql_req(), Id::from_string("user"), "admin")
                .await
        });

        // 等查询登记进 registry，再 cancel；轮询有 5s 上限护栏。
        let start = std::time::Instant::now();
        loop {
            if let Some(q) = registry.list_for(None).first() {
                registry.cancel(&q.id).unwrap();
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "query never registered"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        let result = handle.await.unwrap();
        assert!(
            matches!(result, Err(Error::Cancelled(_))),
            "expected cancelled, got {result:?}"
        );
        // 真中断：远早于 SlowEngine 的 30s 返回（否则就是"跑完才丢结果"）。
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "cancel must abort the in-flight query promptly"
        );
    }
}
