// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 查询上下文：查询请求、结果、跨节点查询计划元信息。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    domain::{
        metrics::PrometheusExemplarQueryResult, storage::PhysicalDatasetKind, stream::StreamType,
    },
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryLanguage {
    Sql,
    Promql,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    pub org_id: Id,
    pub language: QueryLanguage,
    pub statement: String,
    pub time_range: TimeRange,
    /// 可选：限定查询的 stream，便于做分区裁剪
    pub stream: Option<StreamHint>,
    pub limit: Option<usize>,
    /// 联邦查询目标集群（spec federated-search）。
    ///
    /// 由 HTTP 层从 `?clusters=<csv>` 填充；`"local"` 表示本集群，其它为
    /// `remote_clusters` 注册的远端 id/name。空 vec 或仅含 `"local"` 时
    /// 行为与非联邦本地查询 100% 等价（[`FederatedDistributedEngine`] 透传 inner）。
    #[serde(default)]
    pub federation_clusters: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamHint {
    pub name: String,
    pub stream_type: StreamType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub scanned_rows: u64,
    pub took_ms: u64,
    /// 联邦查询元信息（spec federated-search）。
    ///
    /// 仅当查询真正扇出到 ≥1 个远端集群时为 `Some`；普通本地查询为 `None`，
    /// 序列化时整体省略，不影响既有响应形态。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub federation: Option<FederationMeta>,
}

/// 联邦查询的扇出结果元信息（响应里挂在 `meta.federation` 下）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FederationMeta {
    /// 成功返回数据的集群（含 `"local"`）。
    pub scanned_clusters: Vec<String>,
    /// 不可达 / 鉴权失败而被降级跳过的集群。
    pub degraded_clusters: Vec<String>,
    /// 每个降级集群对应的失败原因。
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub degraded_reason: std::collections::BTreeMap<String, String>,
}

/// 慢查询记录（per org，按 fingerprint 去重累计）。查询执行超阈值时由 API 层 best-effort
/// 落库；`GET /query/slow` 读、周期 worker 批量分析给优化建议。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlowQuery {
    pub id: Id,
    pub org_id: Id,
    /// 规范化语句的稳定指纹（去重键）。
    pub fingerprint: String,
    pub language: QueryLanguage,
    pub statement: String,
    pub scanned_rows: i64,
    pub returned_rows: i64,
    pub took_ms: i64,
    pub time_range_secs: Option<i64>,
    /// 命中次数（同 fingerprint 重复出现累加）。
    pub hit_count: i64,
    pub first_seen: TimestampMicros,
    pub last_seen: TimestampMicros,
}

/// 慢查询持久化端口。
#[async_trait]
pub trait SlowQueryRepository: Send + Sync {
    /// 记录一次慢查询；同 (org, fingerprint) 已存在则累加 hit_count + 更新最新统计/last_seen。
    async fn record(&self, q: SlowQuery) -> Result<()>;
    /// 列出某 org 最近的慢查询（按 last_seen 降序）。
    async fn list_recent(&self, org_id: &Id, limit: i64) -> Result<Vec<SlowQuery>>;
}

/// 整结果缓存端口（infra 用 moka 实装）。
///
/// 时间窗尚未封闭（`end` 落在最近的新鲜窗口内）的查询一律不缓存——由实装内部判断，
/// 调用方不必关心，所以实时面板不会读到陈旧数据。
///
/// `role_filter` 必须进缓存键：不同角色可见的数据范围不同，共用一份结果会串数据。
#[async_trait]
pub trait QueryResultCachePort: Send + Sync {
    /// miss、或时间窗未封闭 → `None`。
    async fn get(
        &self,
        req: &QueryRequest,
        role_filter: &str,
        now_micros: i64,
    ) -> Option<QueryResult>;

    /// 时间窗未封闭时是 no-op。
    async fn put(
        &self,
        req: &QueryRequest,
        role_filter: &str,
        now_micros: i64,
        result: QueryResult,
    );
}

/// SQL 查询执行端口（DataFusion 实现）。
#[async_trait]
pub trait QueryEngine: Send + Sync {
    async fn execute(&self, req: QueryRequest) -> Result<QueryResult>;

    /// 可信内部读模型查询。外部 SQL 仍只走逻辑 stream；列表、目录等产品端点可显式
    /// 选择同一 stream 下的窄物理数据集。默认仅允许 raw，具体引擎需显式实现派生读取。
    async fn execute_dataset(
        &self,
        req: QueryRequest,
        dataset_kind: PhysicalDatasetKind,
    ) -> Result<QueryResult> {
        if dataset_kind == PhysicalDatasetKind::Raw {
            self.execute(req).await
        } else {
            Err(Error::invalid(format!(
                "query engine does not support physical dataset `{dataset_kind}`"
            )))
        }
    }

    /// 带跨集群查询 id 的执行（#12 super-cluster leader）。`fed_id` 非空时，联邦引擎把它
    /// 随 `QueryShard` 下发给远端，使远端子查询可经 `CancelQuery(fed_id)` 显式取消。
    /// 默认忽略 `fed_id`，等价于 [`Self::execute`]（非联邦引擎无需 override，零 ripple）。
    async fn execute_federated(
        &self,
        req: QueryRequest,
        _fed_id: Option<String>,
    ) -> Result<QueryResult> {
        self.execute(req).await
    }

    /// 规划查询并返回优化后逻辑计划文本（不执行、不读数据）。search inspector 用。
    /// 默认未实现；DataFusion 引擎 override（schema-only 规划）。
    async fn explain(&self, _req: QueryRequest) -> Result<String> {
        Err(Error::invalid("explain not supported by this engine"))
    }
}

/// PromQL 执行端口。
///
/// 与 [`QueryEngine`] 并列，因为 PromQL 的 instant / range vector 模型
/// 与 SQL 不同；[`QueryService`](crate::domain::query) 按 `language` 派发到对应 engine。
///
/// infra 端实现见 `molesignal-infra` 的 `query::promql`：基于 `promql-parser`
/// 解析 + 自研 evaluator over Arrow/DataFusion，支持 instant / range 两类求值。
#[async_trait]
pub trait PromqlEngine: Send + Sync {
    async fn execute(&self, req: QueryRequest) -> Result<QueryResult>;

    async fn query_exemplars(&self, _req: QueryRequest) -> Result<PrometheusExemplarQueryResult> {
        Err(Error::invalid(
            "Prometheus exemplar queries are not supported by this engine",
        ))
    }
}
