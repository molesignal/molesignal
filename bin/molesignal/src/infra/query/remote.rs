// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 把查询交给 querier 节点执行的引擎。
//!
//! 没有 querier 角色的节点（router、alert 等）用 [`RemoteSqlEngine`] 与
//! [`RemotePromqlEngine`] 作为 SQL 与 PromQL 引擎：请求经内部 gRPC
//! （`query.v1.QueryExecutionService/Execute`）发给 querier 节点，由它用本地引擎完整执行
//! （文件裁剪、全文索引、聚合都在 querier 上），本节点只接收最终结果。
//!
//! 指明了数据流的查询按 rendezvous 哈希固定到一个 querier：同一组织、同一数据流的查询总在
//! 同一个节点上执行，它的块缓存、元数据缓存与结果缓存才会被反复命中，节点增减时也只迁移
//! 少数数据流。其余查询优先取轮询选出的 querier。目标节点不可达时依次尝试其余 querier。

use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Serialize, de::DeserializeOwned};
use tonic::{Code, Status, transport::Channel};

use crate::{
    app::cluster::{ClusterRegistry, PeerInfo, PeerRole},
    domain::{
        metrics::PrometheusExemplarQueryResult,
        query::{PromqlEngine, QueryEngine, QueryRequest, QueryResult},
        storage::DatasetTypeId,
    },
    infra::cluster::{grpc_channel, grpc_status::status_to_error},
    protocol::query::v1::{
        ExecuteOp, ExecuteRequest, query_execution_service_client::QueryExecutionServiceClient,
    },
    shared::{Error, Result, rendezvous},
};

/// 一次转发最多尝试的 querier 节点数。
const MAX_ATTEMPTS: usize = 3;

/// 单个查询在 querier 上的最长执行时间。更长的查询走异步 job。
const QUERY_TIMEOUT: Duration = Duration::from_secs(300);

pub struct RemoteQueryClient {
    registry: Arc<dyn ClusterRegistry>,
    channels: Mutex<HashMap<String, Channel>>,
    max_message_bytes: usize,
}

impl RemoteQueryClient {
    pub fn new(registry: Arc<dyn ClusterRegistry>, max_message_bytes: usize) -> Arc<Self> {
        Arc::new(Self {
            registry,
            channels: Mutex::new(HashMap::new()),
            max_message_bytes,
        })
    }

    /// 按尝试顺序排好的 querier 节点。有 `affinity` 时按它的 rendezvous 权重从大到小排，
    /// 否则轮询选出的节点在前。
    async fn candidates(&self, affinity: Option<&str>) -> Vec<PeerInfo> {
        let mut peers = self.registry.list_role(PeerRole::Querier).await;

        if let Some(key) = affinity {
            peers.sort_by_cached_key(|peer| {
                std::cmp::Reverse(rendezvous::weight(key, &peer.node_id))
            });
            return peers;
        }

        if let Some(first) = self.registry.pick_querier().await
            && let Some(position) = peers.iter().position(|peer| peer.node_id == first.node_id)
        {
            peers.swap(0, position);
        }

        peers
    }

    async fn channel(&self, addr: &str) -> std::result::Result<Channel, Status> {
        if let Some(channel) = self.channels.lock().get(addr) {
            return Ok(channel.clone());
        }

        let url = grpc_channel::endpoint_url(addr, false);

        let channel = Channel::from_shared(url)
            .map_err(|error| Status::unavailable(format!("invalid endpoint: {error}")))?
            .connect_timeout(grpc_channel::CONNECT_TIMEOUT)
            .timeout(QUERY_TIMEOUT)
            .connect()
            .await
            .map_err(|error| Status::unavailable(format!("connect: {error}")))?;

        self.channels
            .lock()
            .insert(addr.to_owned(), channel.clone());

        Ok(channel)
    }

    async fn call<Req: Serialize, Res: DeserializeOwned>(
        &self,
        op: ExecuteOp,
        request: &Req,
        dataset_type: Option<&DatasetTypeId>,
        affinity: Option<String>,
    ) -> Result<Res> {
        let peers = self.candidates(affinity.as_deref()).await;

        if peers.is_empty() {
            return Err(Error::unavailable("no querier node is available"));
        }

        let message = ExecuteRequest {
            op: op as i32,
            request: serde_json::to_vec(request)
                .map_err(|error| Error::internal(format!("encode query: {error}")))?
                .into(),
            dataset_type: dataset_type
                .map(|dataset| dataset.as_str().to_owned())
                .unwrap_or_default(),
        };

        let mut unreachable = String::new();

        for peer in peers.into_iter().take(MAX_ATTEMPTS) {
            match self.send(&peer.advertise_addr, message.clone()).await {
                Ok(bytes) => {
                    return serde_json::from_slice(&bytes)
                        .map_err(|error| Error::internal(format!("decode query result: {error}")));
                }
                // 连接失败：这个节点没有执行查询，换下一个。
                Err(status) if status.code() == Code::Unavailable => {
                    self.channels.lock().remove(&peer.advertise_addr);
                    tracing::warn!(
                        node_id = %peer.node_id,
                        reason = status.message(),
                        "querier node unreachable; trying the next one"
                    );
                    unreachable = status.message().to_owned();
                }
                Err(status) => return Err(status_to_error(status)),
            }
        }

        Err(Error::unavailable(format!(
            "no querier node could run the query: {unreachable}"
        )))
    }

    async fn send(
        &self,
        addr: &str,
        message: ExecuteRequest,
    ) -> std::result::Result<bytes::Bytes, Status> {
        let mut client = QueryExecutionServiceClient::new(self.channel(addr).await?)
            .max_decoding_message_size(self.max_message_bytes)
            .max_encoding_message_size(self.max_message_bytes);

        // The caller's read grants go along: the querier reads `_audit` only if they allow it.
        let mut request = tonic::Request::new(message);

        if let Some(grants) = super::grants::current().header_value() {
            request.metadata_mut().insert(
                super::grants::HEADER,
                tonic::metadata::MetadataValue::from_static(grants),
            );
        }

        let response = crate::shared::grpc_trace::call(
            request,
            "query.v1.QueryExecutionService",
            "Execute",
            crate::shared::grpc_trace::GrpcTarget::Internal,
            |request| client.execute(request),
        )
        .await?;

        Ok(response.into_inner().result)
    }
}

/// 查询固定到哪个 querier 的依据：组织与数据流。没有指明数据流的查询不固定。
fn affinity(req: &QueryRequest) -> Option<String> {
    let stream = req.stream.as_ref()?;

    Some(format!(
        "{}\0{}\0{}",
        req.org_id,
        stream.stream_type.as_str(),
        stream.name
    ))
}

/// SQL 引擎：查询在 querier 节点上执行。
pub struct RemoteSqlEngine(Arc<RemoteQueryClient>);

impl RemoteSqlEngine {
    pub fn new(client: Arc<RemoteQueryClient>) -> Self {
        Self(client)
    }
}

#[async_trait]
impl QueryEngine for RemoteSqlEngine {
    async fn execute(&self, req: QueryRequest) -> Result<QueryResult> {
        self.0
            .call(ExecuteOp::Sql, &req, None, affinity(&req))
            .await
    }

    async fn execute_dataset(
        &self,
        req: QueryRequest,
        dataset_type: DatasetTypeId,
    ) -> Result<QueryResult> {
        self.0
            .call(
                ExecuteOp::SqlDataset,
                &req,
                Some(&dataset_type),
                affinity(&req),
            )
            .await
    }

    async fn explain(&self, req: QueryRequest) -> Result<String> {
        self.0
            .call(ExecuteOp::SqlExplain, &req, None, affinity(&req))
            .await
    }
}

/// PromQL 引擎：查询在 querier 节点上执行。
pub struct RemotePromqlEngine(Arc<RemoteQueryClient>);

impl RemotePromqlEngine {
    pub fn new(client: Arc<RemoteQueryClient>) -> Self {
        Self(client)
    }
}

#[async_trait]
impl PromqlEngine for RemotePromqlEngine {
    async fn execute(&self, req: QueryRequest) -> Result<QueryResult> {
        self.0
            .call(ExecuteOp::Promql, &req, None, affinity(&req))
            .await
    }

    async fn query_exemplars(&self, req: QueryRequest) -> Result<PrometheusExemplarQueryResult> {
        self.0
            .call(ExecuteOp::PromqlExemplars, &req, None, affinity(&req))
            .await
    }
}

#[cfg(test)]
mod tests;
