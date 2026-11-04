// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 把外部告警转给告警来源的接收集群：经联邦的 gRPC（`cluster.v1.EventService/ForwardAlerts`），
//! 用这个组织在接收集群上的 per-org token（没有就用集群级 token），与事件推送相同。
//!
//! 最多等 [`FORWARD_TIMEOUT`]：阿里云云监控只等 5 秒，转发要在发送方放弃之前回话；超时、连不上都
//! 当作接收集群不可达，发送方会重试。

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use serde_json::Value;

use super::{
    ClusterSecretRepository, RemoteClustersRepository, grpc_channel, grpc_status::status_to_error,
};
use crate::{
    app::alerting::sources::{AlertForwarder, ForwardedRequest, IntakeReply},
    infra::{
        persistence::repositories::cluster::events::ClusterOrgLinkRepository,
        secret::resolve_secret_ref,
    },
    protocol::cluster::v1::{ForwardAlertsRequest, event_service_client::EventServiceClient},
    shared::{Error, Result, ids::Id},
};

/// 转发最多等多久。
const FORWARD_TIMEOUT: Duration = Duration::from_secs(3);

pub struct GrpcAlertForwarder {
    remote_clusters: Arc<dyn RemoteClustersRepository>,
    org_links: Arc<dyn ClusterOrgLinkRepository>,
    secrets: Arc<dyn ClusterSecretRepository>,
    max_message_size: usize,
}

impl GrpcAlertForwarder {
    pub fn new(
        remote_clusters: Arc<dyn RemoteClustersRepository>,
        org_links: Arc<dyn ClusterOrgLinkRepository>,
        secrets: Arc<dyn ClusterSecretRepository>,
        max_message_size: usize,
    ) -> Self {
        Self {
            remote_clusters,
            org_links,
            secrets,
            max_message_size,
        }
    }
}

#[async_trait]
impl AlertForwarder for GrpcAlertForwarder {
    async fn forward(&self, request: ForwardedRequest<'_>) -> Result<IntakeReply> {
        let cluster = match self
            .remote_clusters
            .get(&Id(request.home_cluster.to_string()))
            .await
        {
            Ok(cluster) if cluster.enabled => cluster,
            Ok(_) | Err(Error::NotFound(_)) => {
                return Err(Error::unavailable(format!(
                    "cluster {} is not an enabled remote cluster here",
                    request.home_cluster
                )));
            }
            Err(error) => return Err(error),
        };

        let link = self
            .org_links
            .get_for_local(&cluster.id, request.org_id)
            .await?
            .ok_or_else(|| {
                Error::unavailable(format!(
                    "the organization is not mapped to cluster {}",
                    request.home_cluster
                ))
            })?;

        // per-org token 优先；缺省回退集群级 token。
        let token_ref = link
            .token_secret_ref
            .filter(|reference| !reference.trim().is_empty())
            .unwrap_or_else(|| cluster.token_secret_ref.clone());

        let token = resolve_secret_ref(&token_ref, request.org_id, Some(self.secrets.as_ref()))
            .await
            .map_err(|_| {
                Error::unavailable("the token for the receiving cluster cannot be resolved")
            })?;

        let channel = grpc_channel::connect(&cluster.advertise_addr, cluster.tls_verify)
            .await
            .map_err(Error::unavailable)?;

        let mut call = tonic::Request::new(ForwardAlertsRequest {
            source_cluster_id: request.local_cluster.to_string(),
            org_id: request.org_id.0.clone(),
            source_id: request.source_id.0.clone(),
            content_type: request.content_type.unwrap_or_default().to_string(),
            body: request.body.to_vec().into(),
        });

        call.set_timeout(FORWARD_TIMEOUT);
        grpc_channel::with_bearer(&mut call, &token).map_err(Error::unavailable)?;

        let mut client =
            EventServiceClient::new(channel).max_encoding_message_size(self.max_message_size);

        let response = tokio::time::timeout(
            FORWARD_TIMEOUT,
            crate::shared::grpc_trace::call(
                call,
                "cluster.v1.EventService",
                "ForwardAlerts",
                crate::shared::grpc_trace::GrpcTarget::Internal,
                |call| client.forward_alerts(call),
            ),
        )
        .await
        .map_err(|_| Error::unavailable("the receiving cluster did not answer in time"))?
        .map_err(status_to_error)?
        .into_inner();

        Ok(IntakeReply {
            status: u16::try_from(response.status).unwrap_or(502),
            body: serde_json::from_slice(&response.body).unwrap_or(Value::Null),
        })
    }
}
