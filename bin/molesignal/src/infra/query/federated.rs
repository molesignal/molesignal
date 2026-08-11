// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Federated DistributedEngine（spec federated-search）。
//!
//! 包装 inner（local / 集群内 distributed）引擎，在 coordinator path 上扇出到 remote
//! cluster：当 `QueryRequest.federation_clusters` 含非 `"local"` 集群时——
//! 1. 本地扫描 `SELECT * FROM <stream>` → `RecordBatch`；
//! 2. 对每个命中的 enabled remote，经 Arrow Flight `do_get` 跑同一份 scan SQL（携
//!    bearer token；ticket 的 `parquet_file_metas` 留空 → 远端自解析本集群的 parquet_file_meta）；
//! 3. UNION 所有 `RecordBatch` → 本地 DataFusion 跑 final user SQL（聚合只在 coordinator
//!    跑一次）；
//! 4. 不可达 / 鉴权失败的 cluster 记入 `FederationMeta.degraded_clusters`，不 500。
//!
//! 无非 `local` 目标时 100% 透传 inner（OSS / 普通本地查询行为不变）。
//!
//! 已知限制（留 follow-up）：
//! - 本地分支在 coordinator 单点扫描，不再走集群内分片（federation 优先正确性）。
//! - 跨集群 schema 需一致（与集群内 distributed 同假设）；不一致时 UNION 会报错。
//! - `tls_verify=false` 当前映射为明文 http；跳过证书校验的 https 需额外 tonic feature。

use std::{collections::BTreeMap, sync::Arc};

use arrow::{array::RecordBatch, datatypes::Schema as ArrowSchema};
use arrow_flight::{
    Ticket, decode::FlightRecordBatchStream, error::FlightError,
    flight_service_client::FlightServiceClient,
};
use async_trait::async_trait;
use datafusion::{common::TableReference, datasource::MemTable, prelude::SessionContext};
use futures::TryStreamExt;
use object_store::ObjectStore;
use prost::Message;

use crate::{
    domain::{
        masking::Masker,
        query::{FederationMeta, QueryEngine, QueryRequest, QueryResult, StreamHint},
        storage::{ParquetFileMetaRepository, PhysicalDatasetKind},
        stream::StreamRepository,
    },
    infra::{
        cluster::{
            cluster_secrets_repo::ClusterSecretRepository,
            remote_clusters_repo::{RemoteCluster, RemoteClustersRepository},
        },
        persistence::repositories::{
            log_patterns::LogPatternRepository, regex_patterns::RegexPatternRepository,
        },
        query::distributed::{batches_to_json, stream_type_to_proto},
        secret::resolve_secret_ref,
    },
    protocol::query::v1::QueryShard,
    shared::{Error, Result},
};

mod cancel_guard;
mod local_scan;
mod telemetry;

use cancel_guard::DispatchGuard;

pub struct FederatedDistributedEngine {
    inner: Arc<dyn QueryEngine>,
    remote_clusters: Arc<dyn RemoteClustersRepository>,
    files: Arc<dyn ParquetFileMetaRepository>,
    object_store: Arc<dyn ObjectStore>,
    secrets: Option<Arc<dyn ClusterSecretRepository>>,
    /// 字段加密 DEK 服务；非空时 coordinator final SQL 按 org 预载 DEK 注册 `decrypt(col)`。
    field_keys: Option<Arc<crate::infra::cipher::FieldKeyService>>,
    /// 脱敏规则 repo；非空时含 `mask(` 的 final SQL 按 org 加载规则注册 `mask(col)`。
    regex_patterns: Option<Arc<dyn RegexPatternRepository>>,
    /// log-pattern repo；非空时含 `extract_pattern(` 的 final SQL 注册 `extract_pattern(col)`。
    log_patterns: Option<Arc<dyn LogPatternRepository>>,
    /// 跨集群查询取消表（#12）：非空时记录每条联邦查询派发到的集群，供 cancel 路由定向 fan-out。
    cancel_registry: Option<Arc<crate::infra::query::federation_cancel::FederationCancelRegistry>>,
    /// stream repo：联邦 final SQL 自建 `SessionContext`、不走 `inner.execute`，
    /// 归属 / queryable 校验只能在这一层自己做。
    streams: Option<Arc<dyn StreamRepository>>,
}

impl FederatedDistributedEngine {
    pub fn new(
        inner: Arc<dyn QueryEngine>,
        remote_clusters: Arc<dyn RemoteClustersRepository>,
        files: Arc<dyn ParquetFileMetaRepository>,
        object_store: Arc<dyn ObjectStore>,
    ) -> Self {
        Self {
            inner,
            remote_clusters,
            files,
            object_store,
            secrets: None,
            field_keys: None,
            regex_patterns: None,
            log_patterns: None,
            cancel_registry: None,
            streams: None,
        }
    }

    /// 注入 stream repo，启用联邦路径的归属 / queryable 校验。
    /// 未注入时联邦查询不校验 `queryable`——`wire` 必须接上。
    pub fn with_streams(mut self, streams: Arc<dyn StreamRepository>) -> Self {
        self.streams = Some(streams);
        self
    }

    /// 注入跨集群查询取消表 → 联邦查询派发集群被记录，cancel 路由可只通知参与者。
    pub fn with_cancel_registry(
        mut self,
        reg: Arc<crate::infra::query::federation_cancel::FederationCancelRegistry>,
    ) -> Self {
        self.cancel_registry = Some(reg);
        self
    }

    /// federated-search：注入 cluster-secrets 存储，使 `cipher_keys:<id>`
    /// 形式的 `token_secret_ref` 可从 DB 解出明文 token。未注入时仅 `env:` 可用。
    pub fn with_secrets(mut self, secrets: Arc<dyn ClusterSecretRepository>) -> Self {
        self.secrets = Some(secrets);
        self
    }

    /// 注入字段加密 DEK 服务；非空时联邦 final SQL 注册 `decrypt(col)`。
    pub fn with_field_keys(mut self, svc: Arc<crate::infra::cipher::FieldKeyService>) -> Self {
        self.field_keys = Some(svc);
        self
    }

    /// 注入脱敏规则 repo；非空时含 `mask(` 的联邦 final SQL 注册 `mask(col)`。
    pub fn with_regex_patterns(mut self, patterns: Arc<dyn RegexPatternRepository>) -> Self {
        self.regex_patterns = Some(patterns);
        self
    }

    /// 注入 log-pattern repo；非空时含 `extract_pattern(` 的联邦 final SQL 注册 UDF。
    pub fn with_log_patterns(mut self, patterns: Arc<dyn LogPatternRepository>) -> Self {
        self.log_patterns = Some(patterns);
        self
    }

    /// 返回当前 enabled remote cluster 数量，供 handler 显示集群状态。
    pub async fn enabled_remote_count(&self) -> usize {
        self.remote_clusters
            .list_enabled()
            .await
            .map(|v| v.len())
            .unwrap_or(0)
    }

    /// 本集群扫描 stream → `RecordBatch`（联邦的 "local" 分支）。
    #[tracing::instrument(
        name = "query.federation.local",
        skip_all,
        fields(otel.kind = "internal", molesignal.query.stage = "local_scan")
    )]
    async fn scan_local(
        &self,
        req: &QueryRequest,
        stream: &StreamHint,
    ) -> Result<Vec<RecordBatch>> {
        local_scan::run(
            &self.files,
            &self.object_store,
            self.streams.as_ref(),
            req,
            stream,
        )
        .await
    }

    /// 对单个 remote 经 Arrow Flight `do_get` 跑 scan SQL → `RecordBatch`。
    /// 失败返回 `(reason, is_auth)`，由调用方记入 degraded + 指标。
    #[tracing::instrument(
        name = "query.federation.remote",
        skip_all,
        fields(otel.kind = "internal", molesignal.query.stage = "remote_scan")
    )]
    async fn fan_out_one(
        &self,
        cluster: &RemoteCluster,
        req: &QueryRequest,
        stream: &StreamHint,
        scan_sql: &str,
        fed_id: Option<&str>,
    ) -> std::result::Result<Vec<RecordBatch>, (String, bool)> {
        // token 解析（明文绝不入日志 / 响应）。
        let token = match resolve_secret_ref(
            &cluster.token_secret_ref,
            &req.org_id,
            self.secrets.as_deref(),
        )
        .await
        {
            Ok(t) => t,
            Err(e) => {
                // 详情（ref_id / env 变量名）只进服务端日志；返回客户端的 degraded_reason
                // 用泛化文案，避免经查询响应枚举 secret 指针内部信息。明文 token 不在 e 内。
                tracing::warn!(cluster = %cluster.name, error = %e, "federated secret resolve failed");
                return Err(("secret resolution failed".to_string(), false));
            }
        };

        // endpoint + 可选 TLS（共用 `grpc_channel` 基元）。
        let channel = crate::infra::cluster::grpc_channel::connect(
            &cluster.advertise_addr,
            cluster.tls_verify,
        )
        .await
        .map_err(|e| (e, false))?;

        // 空 parquet_file_metas → 远端自解析；带 stream_type 供远端 find()。
        let shard = QueryShard {
            org_id: req.org_id.0.clone(),
            stream: stream.name.clone(),
            sql: scan_sql.to_string(),
            parquet_file_metas: Vec::new(),
            projection: Vec::new(),
            time_start_micros: req.time_range.start.0,
            time_end_micros: req.time_range.end.0,
            stream_type: stream_type_to_proto(stream.stream_type).to_string(),
            federation_query_id: fed_id.unwrap_or_default().to_string(),
        };
        let mut buf = Vec::with_capacity(shard.encoded_len());
        shard
            .encode(&mut buf)
            .map_err(|e| (format!("shard encode: {e}"), false))?;
        let mut request = tonic::Request::new(Ticket { ticket: buf.into() });
        crate::infra::cluster::grpc_channel::with_bearer(&mut request, &token)
            .map_err(|e| (e, false))?;

        let mut client = FlightServiceClient::new(channel);
        let resp = crate::shared::grpc_trace::call(
            request,
            "arrow.flight.protocol.FlightService",
            "DoGet",
            crate::shared::grpc_trace::GrpcTarget::Internal,
            |request| client.do_get(request),
        )
        .await
        .map_err(|status| {
            let is_auth = matches!(
                status.code(),
                tonic::Code::Unauthenticated | tonic::Code::PermissionDenied
            );
            (format!("do_get: {}", status.message()), is_auth)
        })?;
        let inbound = resp.into_inner().map_err(FlightError::from);
        let mut s = FlightRecordBatchStream::new_from_flight_data(inbound);
        let mut batches = Vec::new();
        while let Some(b) = s
            .try_next()
            .await
            .map_err(|e| (format!("flight decode: {e}"), false))?
        {
            batches.push(b);
        }
        Ok(batches)
    }
}

#[async_trait]
impl QueryEngine for FederatedDistributedEngine {
    async fn execute(&self, req: QueryRequest) -> Result<QueryResult> {
        self.execute_inner(req, None).await
    }

    async fn execute_dataset(
        &self,
        req: QueryRequest,
        dataset_kind: PhysicalDatasetKind,
    ) -> Result<QueryResult> {
        if req
            .federation_clusters
            .iter()
            .any(|cluster| !cluster.eq_ignore_ascii_case("local"))
        {
            return Err(Error::invalid(
                "physical read models cannot be queried across federation",
            ));
        }
        self.inner.execute_dataset(req, dataset_kind).await
    }

    /// coordinator 把 `fed_id` 随分片下发，使远端子查询可经 `CancelQuery(fed_id)` 取消。
    async fn execute_federated(
        &self,
        req: QueryRequest,
        fed_id: Option<String>,
    ) -> Result<QueryResult> {
        self.execute_inner(req, fed_id).await
    }
}

impl FederatedDistributedEngine {
    #[tracing::instrument(
        name = "query.federated",
        skip_all,
        fields(otel.kind = "internal", molesignal.query.engine = "federated")
    )]
    async fn execute_inner(
        &self,
        req: QueryRequest,
        fed_id: Option<String>,
    ) -> Result<QueryResult> {
        // 无非 local 目标 → 透传 inner（与非联邦完全一致）。
        let has_remote = req
            .federation_clusters
            .iter()
            .any(|c| !c.eq_ignore_ascii_case("local"));
        if !has_remote {
            return self.inner.execute(req).await;
        }

        let started = std::time::Instant::now();
        let stream = req
            .stream
            .clone()
            .ok_or_else(|| Error::invalid("federated query requires query.stream hint to scan"))?;

        // final SQL 在本层自建的 SessionContext 上跑，`inner.execute` 的归属 / queryable
        // 校验对这条路径不生效。在本地扫描与远端扇出之前先校验。
        if let Some(streams) = &self.streams {
            crate::infra::query::planner::ensure_stream_in_org(
                streams.as_ref(),
                &req.org_id,
                &stream.name,
                stream.stream_type,
            )
            .await?;
        }

        let scan_sql = format!(
            "SELECT * FROM \"{}\"",
            crate::infra::query::escape_sql_ident(&stream.name)
        );

        // 1. 本地扫描。
        let mut all_batches = self.scan_local(&req, &stream).await?;
        let mut scanned_clusters = vec!["local".to_string()];
        let mut degraded_clusters: Vec<String> = Vec::new();
        let mut degraded_reason: BTreeMap<String, String> = BTreeMap::new();

        // 2. 远端扇出。
        let enabled = self
            .remote_clusters
            .list_enabled()
            .await
            .unwrap_or_default();
        let requested: Vec<String> = req
            .federation_clusters
            .iter()
            .filter(|c| !c.eq_ignore_ascii_case("local"))
            .cloned()
            .collect();
        // 记录本次查询实际派发到的集群 id，cancel 路由据此只通知参与者；RAII 在查询结束清理。
        let _dispatch_guard = match (&self.cancel_registry, fed_id.as_deref()) {
            (Some(reg), Some(fid)) if !fid.is_empty() => {
                let participants: Vec<String> = requested
                    .iter()
                    .filter_map(|name| enabled.iter().find(|c| c.name == *name || c.id.0 == *name))
                    .map(|c| c.id.0.clone())
                    .collect();
                reg.track_dispatch(fid, participants);
                Some(DispatchGuard::new(reg.clone(), fid.to_string()))
            }
            _ => None,
        };
        // 未知 / 已禁用的集群不涉及 IO，先就地标降级；其余每个集群一个 future 并发扇出，
        // 延迟取决于最慢的那个远端而非所有远端之和。
        // 各 future 只借用这些，别让 async move 把它们整个搬进第一个 future。
        let (req_ref, stream_ref, scan_sql_ref) = (&req, &stream, scan_sql.as_str());
        let fed_id_ref = fed_id.as_deref();
        let mut fan_outs = Vec::new();
        for name in &requested {
            match enabled.iter().find(|c| c.name == *name || c.id.0 == *name) {
                None => {
                    degraded_clusters.push(name.clone());
                    degraded_reason.insert(name.clone(), "unknown or disabled cluster".to_string());
                }
                Some(cluster) => {
                    fan_outs.push(async move {
                        let r = self
                            .fan_out_one(cluster, req_ref, stream_ref, scan_sql_ref, fed_id_ref)
                            .await;
                        (name, r)
                    });
                }
            }
        }
        // join_all 而非 try_join_all：联邦是降级语义，单个远端失败只标 degraded，
        // 不能让整条查询失败。
        for (name, result) in futures::future::join_all(fan_outs).await {
            match result {
                Ok(batches) => {
                    all_batches.extend(batches);
                    scanned_clusters.push(name.clone());
                }
                Err((reason, is_auth)) => {
                    if is_auth {
                        telemetry::auth_errors()
                            .with_label_values(&[name.as_str()])
                            .inc();
                    }
                    degraded_clusters.push(name.clone());
                    degraded_reason.insert(name.clone(), reason);
                }
            }
        }

        let federation = Some(FederationMeta {
            scanned_clusters,
            degraded_clusters,
            degraded_reason,
        });

        // 无任何数据（本地空 + 远端全降级）→ 返回空结果，避免 final SQL 因表未注册而报错。
        if all_batches.is_empty() {
            return Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                scanned_rows: 0,
                took_ms: started.elapsed().as_millis() as u64,
                federation,
            });
        }

        // 3. UNION → 本地 final user SQL（聚合只跑一次）。
        let scanned_rows: u64 = all_batches.iter().map(|b| b.num_rows() as u64).sum();
        let schema = all_batches
            .first()
            .map(|b| b.schema())
            .unwrap_or_else(|| Arc::new(ArrowSchema::empty()));
        let ctx = SessionContext::new();
        // 联邦 coordinator 跑完整 user SQL，需注册同样的 UDAF（如 approx_topk）。
        ctx.register_udaf(crate::infra::query::udafs::approx_topk_udf());
        // 字段级加密：注册 `decrypt(col)`。远端已各自解密、回传明文（未加标记），本地 final
        // SQL 的 decrypt 对明文幂等透传；故即便本节点 DEK 不同也安全。
        if let Some(svc) = &self.field_keys {
            let keys = svc.decrypt_map(&req.org_id).await?;
            ctx.register_udf(crate::infra::query::udfs::build_decrypt_udf(keys));
        }
        // 脱敏：含 `mask(` 的 final SQL 注册 `mask(col)`。远端各自脱敏后回传，本地 `mask()` 对
        // 已脱敏文本幂等（`[REDACTED]` 不再匹配原正则），故跨集群安全（与上方 decrypt 同理）。
        if req.statement.contains("mask(")
            && let Some(repo) = &self.regex_patterns
        {
            let pats = repo.list(&req.org_id).await.unwrap_or_default();
            let masker = Masker::compile(pats.into_iter().map(|p| (p.pattern, p.replacement)));
            ctx.register_udf(crate::infra::query::udfs::build_mask_udf(Arc::new(masker)));
        }
        if req.statement.contains("extract_pattern(")
            && let Some(repo) = &self.log_patterns
        {
            let pats = repo.list(&req.org_id).await.unwrap_or_default();
            let rows: Vec<(String, String, i32)> = pats
                .into_iter()
                .map(|p| (p.regex, p.category, p.priority))
                .collect();
            ctx.register_udf(crate::infra::query::udfs::build_extract_pattern_udf(
                crate::infra::query::udfs::compile_patterns(rows),
            ));
        }
        let mem = MemTable::try_new(schema, vec![all_batches])
            .map_err(|e| Error::internal(format!("federated memtable: {e}")))?;
        ctx.register_table(TableReference::bare(stream.name.clone()), Arc::new(mem))
            .map_err(|e| Error::internal(format!("federated register: {e}")))?;
        // datafusion/sqlparser 原始 error（含内部列名 / schema 细节）只进服务端 log，
        // 对外给泛化提示——与单机 DataFusionEngine 一致，别在联邦路径漏内部细节。
        let df = ctx.sql(&req.statement).await.map_err(|e| {
            tracing::warn!(error = %e, sql = %req.statement, "federated query planning failed");
            Error::invalid("query could not be planned: check the SQL syntax and that every referenced field exists in the stream")
        })?;
        let out = df
            .collect()
            .await
            .map_err(|e| Error::internal(format!("federated collect: {e}")))?;
        let (columns, rows) = batches_to_json(&out);

        Ok(QueryResult {
            columns,
            rows,
            scanned_rows,
            took_ms: started.elapsed().as_millis() as u64,
            federation,
        })
    }
}
