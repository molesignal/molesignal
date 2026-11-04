// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警来源在联邦集群之间：
//!
//! - `cluster.v1.EventService/ForwardAlerts` 的接收端：别的集群转来的外部告警。和事件推送一样按
//!   per-org token 鉴权、按 org 映射把转发集群的组织换成本地组织，再交给告警来源处理；处理结果
//!   原样回给转发集群，由它回给发送方。
//! - 同步来的来源写到本集群；同步来的删除先解决这个来源在本集群的活跃事件，它们再也收不到恢复。

use tonic::{Request, Response, Status};

use crate::{
    api::{AppState, http::middleware::auth::authenticate_api_token},
    domain::{alerting::source::AlertSource, federation::CudAction},
    protocol::cluster::v1::{ForwardAlertsRequest, ForwardAlertsResponse},
    shared::{Error, Result, ids::Id},
};

/// 同步来的来源变化落到本集群（组织已换成本地组织）。
pub(super) async fn apply_source(
    state: &AppState,
    action: CudAction,
    local_org: &Id,
    resource_id: &str,
    data: &serde_json::Value,
) -> Result<()> {
    let sources = &state.alerting.sources;
    let id = Id(resource_id.to_string());

    match action {
        CudAction::Deleted => sources.remove_replica(local_org, &id).await,
        CudAction::Created | CudAction::Updated => {
            let mut source: AlertSource = serde_json::from_value(data.clone())
                .map_err(|error| Error::invalid(format!("federation payload: {error}")))?;

            source.org_id = local_org.clone();
            source.id = id;
            source.last_intake_at = None;
            sources.apply_replica(source).await
        }
    }
}

pub(super) async fn forward_alerts(
    state: &AppState,
    request: Request<ForwardAlertsRequest>,
) -> std::result::Result<Response<ForwardAlertsResponse>, Status> {
    let bearer = request
        .metadata()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer ").map(str::to_string))
        .ok_or_else(|| Status::unauthenticated("missing bearer token"))?;

    let token_org = authenticate_api_token(
        &bearer,
        state.iam.service.as_ref(),
        state.iam.api_tokens.clone(),
    )
    .await
    .map_err(|error| Status::unauthenticated(error.to_string()))?
    .org_id;

    let request = request.into_inner();

    // 转发集群的组织 → 本地组织；没映射的组织不收。
    let link = state
        .cluster
        .org_link
        .get_for_remote(
            &Id(request.source_cluster_id.clone()),
            &Id(request.org_id.clone()),
        )
        .await
        .map_err(|error| Status::unavailable(error.to_string()))?
        .ok_or_else(|| Status::permission_denied("the organization is not mapped"))?;

    if token_org != link.local_org_id {
        return Err(Status::permission_denied(
            "the token belongs to another organization",
        ));
    }

    let content_type = Some(request.content_type.as_str()).filter(|value| !value.is_empty());

    let reply = state
        .alerting
        .sources
        .intake_forwarded(
            &link.local_org_id,
            &Id(request.source_id),
            &request.source_cluster_id,
            content_type,
            &request.body,
        )
        .await;

    let body = serde_json::to_vec(&reply.body)
        .map_err(|error| Status::internal(format!("encode the reply: {error}")))?;

    Ok(Response::new(ForwardAlertsResponse {
        status: u32::from(reply.status),
        body: body.into(),
    }))
}
