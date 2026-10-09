// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 经内部 gRPC（`intake.v1.IntakeService/Push`）把写入转发给 intake 节点。
//!
//! 目标节点按 `(org, stream)` 一致性哈希选出，同一条数据流的写入尽量落在同一个节点上。
//! 目标节点不可达时依次尝试其余 intake 节点。

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use parking_lot::Mutex;
use tonic::{Code, Status, transport::Channel};

use super::{grpc_channel, grpc_status::status_to_error};
use crate::{
    app::{
        cluster::{ClusterRegistry, PeerInfo, PeerRole},
        intake::IntakeForwarder,
    },
    domain::{
        intake::{IntakeBatch, IntakeError, IntakeResult},
        stream::StreamType,
    },
    protocol::intake::v1::{
        PushRequest, PushResponse, StreamType as ProtoStreamType,
        intake_service_client::IntakeServiceClient,
    },
    shared::{Error, Result, ids::Id},
};

/// 一次转发最多尝试的 intake 节点数。
const MAX_ATTEMPTS: usize = 3;

pub struct GrpcIntakeForwarder {
    registry: Arc<dyn ClusterRegistry>,
    channels: Mutex<HashMap<String, Channel>>,
}

impl GrpcIntakeForwarder {
    pub fn new(registry: Arc<dyn ClusterRegistry>) -> Self {
        Self {
            registry,
            channels: Mutex::new(HashMap::new()),
        }
    }

    /// 按尝试顺序排好的 intake 节点：哈希选出的节点在前，其余节点随后。
    async fn candidates(&self, org_id: &Id, stream: &str) -> Vec<PeerInfo> {
        let mut peers = self.registry.list_role(PeerRole::Intake).await;
        if let Some(first) = self.registry.pick_intake(org_id, stream).await
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
        let channel = grpc_channel::connect(addr, false)
            .await
            .map_err(Status::unavailable)?;
        self.channels
            .lock()
            .insert(addr.to_owned(), channel.clone());
        Ok(channel)
    }

    async fn push(
        &self,
        addr: &str,
        request: PushRequest,
    ) -> std::result::Result<PushResponse, Status> {
        let mut client = IntakeServiceClient::new(self.channel(addr).await?);
        let response = crate::shared::grpc_trace::call(
            tonic::Request::new(request),
            "intake.v1.IntakeService",
            "Push",
            crate::shared::grpc_trace::GrpcTarget::Internal,
            |request| client.push(request),
        )
        .await?;
        Ok(response.into_inner())
    }
}

#[async_trait]
impl IntakeForwarder for GrpcIntakeForwarder {
    async fn forward(&self, batch: IntakeBatch) -> Result<IntakeResult> {
        let peers = self.candidates(&batch.org_id, &batch.stream).await;
        if peers.is_empty() {
            return Err(Error::unavailable("no intake node is available"));
        }
        let request = push_request(&batch)?;
        let mut unreachable = String::new();
        for peer in peers.into_iter().take(MAX_ATTEMPTS) {
            match self.push(&peer.advertise_addr, request.clone()).await {
                Ok(response) => return Ok(into_result(response)),
                // 连接失败：这个节点没有处理批次，换下一个。
                Err(status) if status.code() == Code::Unavailable => {
                    self.channels.lock().remove(&peer.advertise_addr);
                    tracing::warn!(
                        node_id = %peer.node_id,
                        reason = status.message(),
                        "intake node unreachable; trying the next one"
                    );
                    unreachable = status.message().to_owned();
                }
                Err(status) => return Err(status_to_error(status)),
            }
        }
        Err(Error::unavailable(format!(
            "no intake node accepted the batch: {unreachable}"
        )))
    }
}

fn push_request(batch: &IntakeBatch) -> Result<PushRequest> {
    let payload = serde_json::to_vec(&batch.events)
        .map_err(|error| Error::internal(format!("encode intake batch: {error}")))?;
    Ok(PushRequest {
        batch_id: batch.batch_id.0.clone(),
        org_id: batch.org_id.0.clone(),
        stream: batch.stream.clone(),
        stream_type: proto_stream_type(batch.stream_type) as i32,
        payload: payload.into(),
        received_at_micros: batch.received_at.0,
        stream_type_name: batch.stream_type.external_name().to_owned(),
    })
}

fn proto_stream_type(stream_type: StreamType) -> ProtoStreamType {
    if stream_type == StreamType::LOGS {
        ProtoStreamType::Logs
    } else if stream_type == StreamType::METRICS {
        ProtoStreamType::Metrics
    } else if stream_type == StreamType::TRACES {
        ProtoStreamType::Traces
    } else if stream_type == StreamType::PROFILES {
        ProtoStreamType::Profiles
    } else {
        ProtoStreamType::Unspecified
    }
}

fn into_result(response: PushResponse) -> IntakeResult {
    IntakeResult {
        accepted: response.accepted as usize,
        rejected: response.rejected as usize,
        errors: response
            .errors
            .into_iter()
            .map(|error| IntakeError {
                index: error.index as usize,
                reason: error.reason,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests;
