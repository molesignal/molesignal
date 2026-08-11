// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Arrow Flight server：实现 `arrow_flight::FlightService`。
//!
//! - `do_get(Ticket)`：把 ticket bytes 反序列化为 `query.v1.QueryShard`，按 `parquet_file_metas`
//!   把显式 parquet 清单注册为 `ParquetExec` → 跑 shard.sql（仅 scan + WHERE + projection；最终聚合
//!   留 coordinator） → 用 `FlightDataEncoderBuilder` 把 `RecordBatch` 流编码为
//!   `FlightData` 流返回。
//! - 其他 RPC（`handshake / list_flights / get_flight_info / get_schema / do_put /
//!   do_action / list_actions / do_exchange / poll_flight_info`）返 `Unimplemented`。

use std::{
    pin::Pin,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use arrow::datatypes::Schema as ArrowSchema;
use arrow_flight::{
    Action, ActionType, Criteria, Empty, FlightData, FlightDescriptor, FlightInfo,
    HandshakeRequest, HandshakeResponse, PollInfo, PutResult, SchemaResult, Ticket,
    encode::FlightDataEncoderBuilder,
    flight_service_server::{FlightService, FlightServiceServer},
};
use datafusion::{
    common::TableReference, execution::object_store::ObjectStoreUrl, prelude::SessionContext,
};
use futures::stream::{BoxStream, StreamExt};
use object_store::ObjectStore;
use prost::Message;
use tonic::{Request, Response, Status, Streaming};

use crate::{
    api::http::middleware::auth::authenticate_api_token,
    app::iam::IamService,
    domain::{
        iam::api_token::ApiTokenRepository,
        storage::{ParquetFileMeta, ParquetFileMetaRepository, PhysicalDatasetKind},
        stream::StreamType,
    },
    infra::{
        query::{federation_cancel::FederationCancelRegistry, parquet_table::PrunedParquetTable},
        storage::parquet::reader::ParquetReader,
    },
    protocol::query::v1::QueryShard,
    shared::{
        ids::Id,
        time::{TimeRange, TimestampMicros},
        trace_stream::segmented_result_stream,
    },
};

pub mod sql;

pub struct FlightGrpc {
    object_store: Arc<dyn ObjectStore>,
    /// federated-search 联邦请求（空 parquet_file_metas）时远端用它自解析本集群的
    /// parquet_file_meta；None 时不支持联邦自解析（仅集群内预解析分片可用）。
    files: Option<Arc<dyn ParquetFileMetaRepository>>,
    /// federated-search 联邦请求的 bearer 校验用的 API token repo；
    /// None 时联邦请求一律拒绝（未配置鉴权）。集群内分片不受影响。
    api_tokens: Option<Arc<dyn ApiTokenRepository>>,
    /// 联邦 API token 所属用户的实时状态校验。与 `api_tokens` 一起注入，
    /// 确保用户停用后既有 token 立即失效。
    iam: Option<Arc<IamService>>,
    /// 跨集群查询取消表（#12）：带 `federation_query_id` 的分片登记于此，使其可被
    /// coordinator 的 `CancelQuery(fed_id)` 显式中断。None 时仅靠 gRPC 流断开兜底。
    cancel_registry: Option<Arc<FederationCancelRegistry>>,
}

impl FlightGrpc {
    pub fn new(object_store: Arc<dyn ObjectStore>) -> Self {
        Self {
            object_store,
            files: None,
            api_tokens: None,
            iam: None,
            cancel_registry: None,
        }
    }

    /// 注入跨集群查询取消表 → 带 `federation_query_id` 的联邦子查询可被远程取消。
    pub fn with_cancel_registry(mut self, reg: Arc<FederationCancelRegistry>) -> Self {
        self.cancel_registry = Some(reg);
        self
    }

    /// 注入 ParquetFileMetaRepository → 远端可处理联邦自解析分片（空 parquet_file_metas）。
    pub fn with_files(mut self, files: Arc<dyn ParquetFileMetaRepository>) -> Self {
        self.files = Some(files);
        self
    }

    /// 注入 ApiTokenRepository → 远端可校验联邦请求的 bearer token。
    pub fn with_api_tokens(mut self, api_tokens: Arc<dyn ApiTokenRepository>) -> Self {
        self.api_tokens = Some(api_tokens);
        self
    }

    /// 注入 IAM service → 联邦 token 校验同时检查用户是否仍可访问。
    pub fn with_iam(mut self, iam: Arc<IamService>) -> Self {
        self.iam = Some(iam);
        self
    }

    pub fn into_server(self) -> FlightServiceServer<Self> {
        FlightServiceServer::new(self)
    }

    /// 共享给 [`crate::api::grpc::serve_grpc`]：让单进程内的 DistributedDataFusionEngine 也能
    /// 直接调 do_get（in-proc shortcut，省去 tonic 一跳）。
    pub async fn execute_shard(
        &self,
        shard: QueryShard,
    ) -> Result<Vec<arrow::array::RecordBatch>, String> {
        let store = self.object_store.clone();
        let reader = ParquetReader::new(store.clone());

        // parquet_file_metas 为空 = 联邦自解析 —— 远端用
        // (org, stream, stream_type, time_range) 查本集群 parquet_file_meta；非空则沿用
        // coordinator 预解析的集群内分片。
        // 两条分支统一成 ParquetFileMeta，直接交给 ParquetExec；不再先把所有文件解码到 MemTable。
        let mut targets: Vec<ParquetFileMeta> = if shard.parquet_file_metas.is_empty() {
            let files = self.files.as_ref().ok_or_else(|| {
                "parquet_file_meta repo not configured for federated self-resolve".to_string()
            })?;
            let st = parse_stream_type(&shard.stream_type);
            let time_range = TimeRange::new(
                TimestampMicros(shard.time_start_micros),
                TimestampMicros(shard.time_end_micros),
            );
            let org_id = Id(shard.org_id.clone());
            let lookups = crate::domain::storage::logical_query_datasets(st)
                .iter()
                .map(|dataset_kind| {
                    files.find_dataset(&org_id, &shard.stream, st, *dataset_kind, time_range)
                });
            futures::future::try_join_all(lookups)
                .await
                .map_err(|e| format!("parquet_file_meta find: {e}"))?
                .into_iter()
                .flatten()
                .collect()
        } else {
            shard
                .parquet_file_metas
                .iter()
                .map(|file| ParquetFileMeta {
                    id: Id(file.id.clone()),
                    org_id: Id(file.org_id.clone()),
                    stream: file.stream.clone(),
                    stream_type: parse_stream_type(&file.stream_type),
                    dataset_kind: PhysicalDatasetKind::Raw,
                    object_key: file.object_key.clone(),
                    time_range: TimeRange::new(
                        TimestampMicros(file.time_start_micros),
                        TimestampMicros(file.time_end_micros),
                    ),
                    rows: file.rows,
                    size_bytes: file.size_bytes,
                    min_values: serde_json::Map::new(),
                    max_values: serde_json::Map::new(),
                    deleted: false,
                })
                .collect()
        };
        // Prefer the newest footer for schema-evolution fallback and keep the same candidate
        // ordering as the common latest-first query path.
        targets.sort_by(|left, right| {
            right
                .time_range
                .end
                .cmp(&left.time_range.end)
                .then_with(|| right.id.0.cmp(&left.id.0))
        });
        if targets.is_empty() {
            return Ok(Vec::new());
        }
        let first = &targets[0];
        let schema = reader
            .schema_from_store(store.clone(), &first.object_key, first.size_bytes)
            .await
            .map_err(|e| format!("parquet schema: {e}"))?;
        let ctx = SessionContext::new();
        let object_store_url = ObjectStoreUrl::parse("molesignal://flight")
            .map_err(|e| format!("object store URL: {e}"))?;
        ctx.runtime_env()
            .register_object_store(object_store_url.as_ref(), store);
        let table = PrunedParquetTable::new(
            schema,
            &targets,
            object_store_url,
            TimeRange::new(
                TimestampMicros(shard.time_start_micros),
                TimestampMicros(shard.time_end_micros),
            ),
            None,
        );
        ctx.register_table(TableReference::bare(shard.stream.clone()), Arc::new(table))
            .map_err(|e| format!("register: {e}"))?;
        let df = ctx
            .sql(&shard.sql)
            .await
            .map_err(|e| format!("sql parse: {e}"))?;
        df.collect().await.map_err(|e| format!("collect: {e}"))
    }

    /// 执行分片；带 `federation_query_id` 时登记进取消表并与 cancel 标志 race，
    /// coordinator 的 `CancelQuery(fed_id)` 置位即中断、返回 gRPC `cancelled`。
    async fn execute_shard_cancellable(
        &self,
        shard: QueryShard,
    ) -> Result<Vec<arrow::array::RecordBatch>, Status> {
        let fed_id = shard.federation_query_id.clone();
        let reg = match (fed_id.is_empty(), self.cancel_registry.clone()) {
            (false, Some(reg)) => reg,
            // 无 fed_id 或未配置取消表 → 直接执行（行为不变）。
            _ => return self.execute_shard(shard).await.map_err(Status::internal),
        };
        let flag = reg.register(&fed_id);
        // RAII 注销：执行结束（含被取消、出错、正常）都从表里摘掉。
        struct Dereg {
            reg: Arc<FederationCancelRegistry>,
            id: String,
        }
        impl Drop for Dereg {
            fn drop(&mut self) {
                self.reg.deregister(&self.id);
            }
        }
        let _g = Dereg {
            reg: reg.clone(),
            id: fed_id,
        };
        let fut = self.execute_shard(shard);
        tokio::pin!(fut);
        loop {
            tokio::select! {
                biased;
                out = &mut fut => return out.map_err(Status::internal),
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    if flag.load(Ordering::Relaxed) {
                        return Err(Status::cancelled("federated sub-query cancelled by coordinator"));
                    }
                }
            }
        }
    }
}

/// proto 里的 stream_type 字符串 → domain [`StreamType`]；未知值退回 `Logs`。
fn parse_stream_type(s: &str) -> StreamType {
    match s {
        "metrics" => StreamType::Metrics,
        "traces" => StreamType::Traces,
        "profiles" => StreamType::Profiles,
        "extend" => StreamType::Extend,
        _ => StreamType::Logs,
    }
}

type FlightStream<T> = Pin<Box<dyn futures::Stream<Item = Result<T, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl FlightService for FlightGrpc {
    type HandshakeStream = FlightStream<HandshakeResponse>;
    type ListFlightsStream = FlightStream<FlightInfo>;
    type DoGetStream = FlightStream<FlightData>;
    type DoPutStream = FlightStream<PutResult>;
    type DoActionStream = FlightStream<arrow_flight::Result>;
    type ListActionsStream = FlightStream<ActionType>;
    type DoExchangeStream = FlightStream<FlightData>;

    async fn do_get(
        &self,
        request: Request<Ticket>,
    ) -> Result<Response<Self::DoGetStream>, Status> {
        // 先取 bearer（联邦请求要校验），再消费 body。
        let bearer = request
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer ").map(str::to_string));
        let ticket = request.into_inner();
        let shard = QueryShard::decode(ticket.ticket.as_ref())
            .map_err(|e| Status::invalid_argument(format!("decode shard: {e}")))?;

        // federated-search 联邦请求（空 parquet_file_metas = 远端自解析）必须带合法
        // bearer 且 token 的 org 与 query org 一致；集群内分片走可信网络免鉴权（行为不变）。
        if shard.parquet_file_metas.is_empty() {
            let token = bearer.ok_or_else(|| {
                Status::unauthenticated("missing bearer token for federated query")
            })?;
            let repo = self.api_tokens.clone().ok_or_else(|| {
                Status::unavailable("federation auth not configured on this node")
            })?;
            let iam = self.iam.as_deref().ok_or_else(|| {
                Status::unavailable("federation auth not configured on this node")
            })?;
            let ctx = authenticate_api_token(&token, iam, repo)
                .await
                .map_err(|e| Status::unauthenticated(e.to_string()))?;
            if ctx.org_id.0 != shard.org_id {
                return Err(Status::permission_denied(
                    "token org does not match query org",
                ));
            }
        }

        let batches = self.execute_shard_cancellable(shard).await?;

        // 编码成 FlightData stream（FlightDataEncoderBuilder 期望 Result<_, FlightError>）
        let stream: BoxStream<
            'static,
            Result<arrow::array::RecordBatch, arrow_flight::error::FlightError>,
        > = Box::pin(futures::stream::iter(batches.into_iter().map(Ok)));
        let encoded = FlightDataEncoderBuilder::new()
            .build(stream)
            .map(|r| r.map_err(|e| Status::internal(format!("flight encode: {e}"))));
        Ok(Response::new(segmented_result_stream(
            encoded,
            "flight.do_get.stream",
            "flight",
        )))
    }

    async fn handshake(
        &self,
        _r: Request<Streaming<HandshakeRequest>>,
    ) -> Result<Response<Self::HandshakeStream>, Status> {
        Err(Status::unimplemented("handshake"))
    }
    async fn list_flights(
        &self,
        _r: Request<Criteria>,
    ) -> Result<Response<Self::ListFlightsStream>, Status> {
        Err(Status::unimplemented("list_flights"))
    }
    async fn get_flight_info(
        &self,
        _r: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        Err(Status::unimplemented("get_flight_info"))
    }
    async fn poll_flight_info(
        &self,
        _r: Request<FlightDescriptor>,
    ) -> Result<Response<PollInfo>, Status> {
        Err(Status::unimplemented("poll_flight_info"))
    }
    async fn get_schema(
        &self,
        _r: Request<FlightDescriptor>,
    ) -> Result<Response<SchemaResult>, Status> {
        let _ = ArrowSchema::empty();
        Err(Status::unimplemented("get_schema"))
    }
    async fn do_put(
        &self,
        _r: Request<Streaming<FlightData>>,
    ) -> Result<Response<Self::DoPutStream>, Status> {
        Err(Status::unimplemented("do_put"))
    }
    async fn do_action(
        &self,
        _r: Request<Action>,
    ) -> Result<Response<Self::DoActionStream>, Status> {
        Err(Status::unimplemented("do_action"))
    }
    async fn list_actions(
        &self,
        _r: Request<Empty>,
    ) -> Result<Response<Self::ListActionsStream>, Status> {
        Err(Status::unimplemented("list_actions"))
    }
    async fn do_exchange(
        &self,
        _r: Request<Streaming<FlightData>>,
    ) -> Result<Response<Self::DoExchangeStream>, Status> {
        Err(Status::unimplemented("do_exchange"))
    }
}
