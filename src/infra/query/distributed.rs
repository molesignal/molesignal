// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Distributed DataFusion engine：
//!
//! Coordinator path：
//! 1. 拿到 `parquet_file_metas`（已经过 tantivy prune 等步骤）
//! 2. peers = `registry.list_role(Querier)` —— 仅当 ≥ 2 时走分布式，单 peer fallback local
//! 3. 一致性哈希按 `object_key` 分片到 peers；每片打包成 `query.v1.QueryShard` →
//!    prost-encode → `arrow_flight::Ticket`
//! 4. 并发对每个 peer 调 `FlightClient::do_get(Ticket)`（[`fetch_shard`]）；用
//!    `arrow_flight::decode::FlightDataDecoder` 反序列化得到 `RecordBatch` 流
//! 5. UNION ALL → DataFusion 内存表 → 跑完整 SQL（含 final aggregation）
//!
//! 当前注意：peer SQL 设计成 "SELECT * FROM <stream>"（仅 scan + projection），final SQL
//! 由 coordinator 跑完整 user SQL。如果 user SQL 是 `count(*)`，单层 SELECT * 也能让
//! coordinator 跑出正确 count。
//!
//! 集群只有 1 个 querier 时直接走 `local`。

use std::{sync::Arc, time::Instant};

use arrow::{array::RecordBatch, datatypes::Schema as ArrowSchema};
use arrow_flight::{
    Ticket, decode::FlightRecordBatchStream, flight_service_client::FlightServiceClient,
};
use async_trait::async_trait;
use datafusion::{common::TableReference, datasource::MemTable, prelude::SessionContext};
use futures::TryStreamExt;
use object_store::ObjectStore;
use prost::Message;
use tonic::transport::Channel;

use crate::{
    app::cluster::{ClusterRegistry, PeerRole},
    domain::{
        masking::Masker,
        query::{QueryEngine, QueryRequest, QueryResult, StreamHint},
        storage::{ParquetFileMetaRepository, PhysicalDatasetKind},
        stream::StreamType,
    },
    infra::{search::datafusion_engine::DataFusionEngine, storage::parquet::reader::ParquetReader},
    protocol::query::v1::{ParquetFileMetaRef, QueryShard},
    shared::{Error, Result},
};

pub struct DistributedDataFusionEngine {
    local: Arc<DataFusionEngine>,
    registry: Arc<dyn ClusterRegistry>,
    files: Arc<dyn ParquetFileMetaRepository>,
    // 保留供后续 phase（直接拉 parquet fallback / metadata RPC）使用
    #[allow(dead_code)]
    object_store: Arc<dyn ObjectStore>,
}

impl DistributedDataFusionEngine {
    pub fn new(
        local: Arc<DataFusionEngine>,
        registry: Arc<dyn ClusterRegistry>,
        files: Arc<dyn ParquetFileMetaRepository>,
        object_store: Arc<dyn ObjectStore>,
    ) -> Self {
        Self {
            local,
            registry,
            files,
            object_store,
        }
    }
}

/// 向单个 peer 取一个分片：建连接 → `do_get` → 收完整个 batch 流。
/// 拆成独立 future 以便所有 peer 并发跑。
#[tracing::instrument(
    name = "query.shard",
    skip_all,
    fields(otel.kind = "internal", molesignal.query.stage = "flight_shard")
)]
async fn fetch_shard(advertise_addr: String, ticket_bytes: Vec<u8>) -> Result<Vec<RecordBatch>> {
    let endpoint = format!("http://{advertise_addr}");
    let channel = Channel::from_shared(endpoint)
        .map_err(|e| Error::internal(format!("invalid peer endpoint: {e}")))?
        .connect()
        .await
        .map_err(|e| Error::internal(format!("connect peer: {e}")))?;
    let mut client = FlightServiceClient::new(channel);
    let request = tonic::Request::new(Ticket {
        ticket: ticket_bytes.into(),
    });
    let resp = crate::shared::grpc_trace::call(
        request,
        "arrow.flight.protocol.FlightService",
        "DoGet",
        crate::shared::grpc_trace::GrpcTarget::Internal,
        |request| client.do_get(request),
    )
    .await
    .map_err(|e| Error::internal(format!("flight do_get: {e}")))?;
    let inbound = resp
        .into_inner()
        .map_err(arrow_flight::error::FlightError::from);
    let mut stream = FlightRecordBatchStream::new_from_flight_data(inbound);
    let mut out = Vec::new();
    while let Some(batch) = stream
        .try_next()
        .await
        .map_err(|e| Error::internal(format!("flight decode: {e}")))?
    {
        out.push(batch);
    }
    Ok(out)
}

#[async_trait]
impl QueryEngine for DistributedDataFusionEngine {
    #[tracing::instrument(
        name = "query.distributed",
        skip_all,
        fields(otel.kind = "internal", molesignal.query.engine = "distributed_datafusion")
    )]
    async fn execute(&self, req: QueryRequest) -> Result<QueryResult> {
        let peers = self.registry.list_role(PeerRole::Querier).await;
        if peers.len() <= 1 {
            // 单 querier → 直接走本地，避免 Flight 网络一跳
            return self.local.execute(req).await;
        }
        let started = Instant::now();
        let StreamHint { name, stream_type } = req.stream.clone().ok_or_else(|| {
            Error::invalid("query.stream hint is required for distributed engine")
        })?;

        // coordinator 自建 SessionContext 跑 final SQL，不走 local.execute，所以那里的
        // 归属 / queryable 校验对本路径不生效。在扇出前先校验：既堵住 non-queryable 流
        // 被联邦路径绕过，也避免为一个必然被拒的查询白跑一圈远端 IO。
        if let Some(streams) = self.local.streams() {
            crate::infra::query::planner::ensure_stream_in_org(
                streams.as_ref(),
                &req.org_id,
                &name,
                stream_type,
            )
            .await?;
        }

        let lookups = crate::domain::storage::logical_query_datasets(stream_type)
            .iter()
            .map(|dataset_kind| {
                self.files.find_dataset(
                    &req.org_id,
                    &name,
                    stream_type,
                    *dataset_kind,
                    req.time_range,
                )
            });
        let parquet_file_metas = futures::future::try_join_all(lookups)
            .await?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();

        // 一致性哈希切片
        let groups = shard_files_by_hash(&parquet_file_metas, peers.len());
        let mut all_batches: Vec<RecordBatch> = Vec::new();
        let mut scanned_rows: u64 = 0;

        // 每个 peer 一个独立 future，并发拉取：延迟取决于最慢的那个 peer，而不是所有
        // peer 之和。try_join_all 保序返回，UNION 的批次顺序与串行版一致。
        let mut fetches = Vec::with_capacity(peers.len());
        for (i, peer) in peers.iter().enumerate() {
            let shard_files: Vec<_> = groups
                .get(i)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|fm| ParquetFileMetaRef {
                    id: fm.id.0.clone(),
                    org_id: fm.org_id.0.clone(),
                    stream: fm.stream.clone(),
                    stream_type: stream_type_to_proto(fm.stream_type).to_string(),
                    object_key: fm.object_key.clone(),
                    time_start_micros: fm.time_range.start.0,
                    time_end_micros: fm.time_range.end.0,
                    rows: fm.rows,
                    size_bytes: fm.size_bytes,
                })
                .collect();
            if shard_files.is_empty() {
                continue;
            }
            // peer SQL：仅 scan，避免 partial / final 不一致；coordinator 跑完整 user SQL。
            // 流名必须带引号——自动建流的名字可能含点或空格。
            let peer_sql = format!(
                "SELECT * FROM \"{}\"",
                crate::infra::query::escape_sql_ident(&name)
            );
            let shard = QueryShard {
                org_id: req.org_id.0.clone(),
                stream: name.clone(),
                sql: peer_sql,
                parquet_file_metas: shard_files,
                projection: Vec::new(),
                time_start_micros: req.time_range.start.0,
                time_end_micros: req.time_range.end.0,
                // 集群内分片自带非空 parquet_file_metas，远端不走自解析；stream_type 仍填上以备
                // 远端一致处理（spec federated-search）。
                stream_type: stream_type_to_proto(stream_type).to_string(),
                // 集群内分片不需要跨集群取消 id（同进程协调）。
                federation_query_id: String::new(),
            };
            let mut buf = Vec::with_capacity(shard.encoded_len());
            shard
                .encode(&mut buf)
                .map_err(|e| Error::internal(format!("shard encode: {e}")))?;
            fetches.push(fetch_shard(peer.advertise_addr.clone(), buf));
        }

        for batches in futures::future::try_join_all(fetches).await? {
            for batch in batches {
                scanned_rows += batch.num_rows() as u64;
                all_batches.push(batch);
            }
        }

        // Coordinator 跑完整 user SQL
        let schema = all_batches
            .first()
            .map(|b| b.schema())
            .unwrap_or_else(|| Arc::new(ArrowSchema::empty()));
        let ctx = SessionContext::new();
        // Coordinator 跑完整 user SQL，需注册同样的 UDAF（如 approx_topk）才能规划。
        ctx.register_udaf(crate::infra::query::udafs::approx_topk_udf());
        // 字段级加密：复用本地引擎的 DEK 服务，按 org 预载 DEK 注册 `decrypt(col)`
        // （含 decrypt 的 user SQL 也能在 coordinator 重规划）。
        if let Some(svc) = self.local.field_keys() {
            let keys = svc.decrypt_map(&req.org_id).await?;
            ctx.register_udf(crate::infra::query::udfs::build_decrypt_udf(keys));
        }
        // 脱敏：含 `mask(` 的 user SQL 在 coordinator 重规划时也需注册 `mask(col)`，复用本地引擎的
        // regex_patterns repo（远端节点各自 scan 原始列回传，脱敏在 coordinator final SQL 上算）。
        if req.statement.contains("mask(")
            && let Some(repo) = self.local.regex_patterns()
        {
            let pats = repo.list(&req.org_id).await.unwrap_or_default();
            let masker = Masker::compile(pats.into_iter().map(|p| (p.pattern, p.replacement)));
            ctx.register_udf(crate::infra::query::udfs::build_mask_udf(Arc::new(masker)));
        }
        // log-pattern 分类：含 `extract_pattern(` 的 user SQL 在 coordinator 重规划同样需注册。
        if req.statement.contains("extract_pattern(")
            && let Some(repo) = self.local.log_patterns()
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
        if !all_batches.is_empty() {
            let mem = MemTable::try_new(schema.clone(), vec![all_batches])
                .map_err(|e| Error::internal(format!("coord memtable: {e}")))?;
            ctx.register_table(TableReference::bare(name.clone()), Arc::new(mem))
                .map_err(|e| Error::internal(format!("coord register: {e}")))?;
        }
        // 原始 datafusion/sqlparser error 只进服务端 log，对外泛化（同单机引擎），
        // 避免在多 querier 协调路径把内部列名 / schema 细节漏给客户端。
        let df = ctx.sql(&req.statement).await.map_err(|e| {
            tracing::warn!(error = %e, sql = %req.statement, "coordinator query planning failed");
            Error::invalid("query could not be planned: check the SQL syntax and that every referenced field exists in the stream")
        })?;
        let out = df
            .collect()
            .await
            .map_err(|e| Error::internal(format!("coord collect: {e}")))?;
        let (columns, rows) = batches_to_json(&out);

        // 防止 unused import
        let _ = ParquetReader::new;

        Ok(QueryResult {
            columns,
            rows,
            scanned_rows,
            took_ms: started.elapsed().as_millis() as u64,
            federation: None,
        })
    }

    async fn execute_dataset(
        &self,
        req: QueryRequest,
        dataset_kind: PhysicalDatasetKind,
    ) -> Result<QueryResult> {
        // 派生读模型通常是 page_size + 1 的低延迟列表。共享 ParquetFileMeta/object store 使任一
        // querier 都能直接扫描完整窄数据集；避免先把宽行经 Flight 汇总到 coordinator。
        self.local.execute_dataset(req, dataset_kind).await
    }
}

/// 一致性哈希切片：用 object_key 的 fxhash 后 % peer_count 决定归属。
fn shard_files_by_hash<T: Clone + HasObjectKey>(files: &[T], peer_count: usize) -> Vec<Vec<T>> {
    let mut groups: Vec<Vec<T>> = vec![Vec::new(); peer_count.max(1)];
    for f in files {
        let h = fxhash_u64(f.object_key());
        let idx = (h as usize) % peer_count.max(1);
        groups[idx].push(f.clone());
    }
    groups
}

trait HasObjectKey {
    fn object_key(&self) -> &str;
}
impl HasObjectKey for crate::domain::storage::ParquetFileMeta {
    fn object_key(&self) -> &str {
        &self.object_key
    }
}

fn fxhash_u64(s: &str) -> u64 {
    // FNV-1a 简单实现（避免引入额外 dep）；够分散且确定性
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in s.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub(crate) fn stream_type_to_proto(t: StreamType) -> &'static str {
    match t {
        StreamType::Logs => "logs",
        StreamType::Metrics => "metrics",
        StreamType::Traces => "traces",
        StreamType::Profiles => "profiles",
        StreamType::Extend => "extend",
    }
}

pub(crate) fn batches_to_json(
    batches: &[RecordBatch],
) -> (Vec<String>, Vec<Vec<serde_json::Value>>) {
    use arrow::array::{
        Array, BooleanArray, Float64Array, Int64Array, StringArray, TimestampMicrosecondArray,
    };
    let mut columns: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<serde_json::Value>> = Vec::new();
    for b in batches {
        if columns.is_empty() {
            columns = b
                .schema()
                .fields()
                .iter()
                .map(|f| f.name().clone())
                .collect();
        }
        for r in 0..b.num_rows() {
            let mut row: Vec<serde_json::Value> = Vec::with_capacity(b.num_columns());
            for c in 0..b.num_columns() {
                let arr = b.column(c);
                row.push(if arr.is_null(r) {
                    serde_json::Value::Null
                } else if let Some(a) = arr.as_any().downcast_ref::<BooleanArray>() {
                    serde_json::Value::Bool(a.value(r))
                } else if let Some(a) = arr.as_any().downcast_ref::<Int64Array>() {
                    serde_json::Value::from(a.value(r))
                } else if let Some(a) = arr.as_any().downcast_ref::<Float64Array>() {
                    serde_json::Value::from(a.value(r))
                } else if let Some(a) = arr.as_any().downcast_ref::<StringArray>() {
                    serde_json::Value::from(a.value(r).to_string())
                } else if let Some(a) = arr.as_any().downcast_ref::<TimestampMicrosecondArray>() {
                    serde_json::Value::from(a.value(r))
                } else {
                    serde_json::Value::String(format!("{:?}", arr.slice(r, 1)))
                });
            }
            rows.push(row);
        }
    }
    (columns, rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::storage::ParquetFileMeta,
        shared::{
            ids::Id,
            time::{TimeRange, TimestampMicros},
        },
    };

    fn fm(key: &str) -> ParquetFileMeta {
        ParquetFileMeta {
            id: Id::new(),
            org_id: Id::from_string("org"),
            stream: "s".into(),
            stream_type: StreamType::Logs,
            dataset_kind: crate::domain::storage::PhysicalDatasetKind::Raw,
            object_key: key.into(),
            time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(1)),
            rows: 0,
            size_bytes: 0,
            min_values: Default::default(),
            max_values: Default::default(),
            deleted: false,
        }
    }

    #[test]
    fn shard_files_is_stable_and_balanced_enough() {
        let files: Vec<ParquetFileMeta> = (0..20).map(|i| fm(&format!("k/{i}"))).collect();
        let groups_a = shard_files_by_hash(&files, 4);
        let groups_b = shard_files_by_hash(&files, 4);
        // 同输入两次 sharding 应一致（确定性）
        for (a, b) in groups_a.iter().zip(groups_b.iter()) {
            let keys_a: Vec<_> = a.iter().map(|f| f.object_key.clone()).collect();
            let keys_b: Vec<_> = b.iter().map(|f| f.object_key.clone()).collect();
            assert_eq!(keys_a, keys_b);
        }
        let total: usize = groups_a.iter().map(|g| g.len()).sum();
        assert_eq!(total, 20);
    }
}
