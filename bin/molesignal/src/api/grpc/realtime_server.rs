// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `alerting.v1.RealtimeService`：本节点上实时规则窗口内的命中数，给负责汇总的 alert 节点拉。
//!
//! 只挂在内部 gRPC 端口；即便误暴露到公共网络，调用方仍必须同时带内部 origin 标记和集群 bearer。

use std::sync::Arc;

use tonic::{Request, Response, Status};

use crate::{
    app::alerting::realtime::{INTERNAL_ORIGIN_HEADER, REALTIME_COUNTS_ORIGIN, RealtimeAlerts},
    protocol::alerting::v1::{
        RealtimeServiceCountsRequest, RealtimeServiceCountsResponse, RuleCount,
        realtime_service_server::{RealtimeService, RealtimeServiceServer},
    },
    shared::{ids::Id, time::TimestampMicros},
};

/// 一次最多问几条规则。
const MAX_RULES: usize = 10_000;

pub struct RealtimeCountsGrpc {
    alerts: Arc<RealtimeAlerts>,
    cluster_token: Option<Arc<str>>,
}

impl RealtimeCountsGrpc {
    pub fn new(alerts: Arc<RealtimeAlerts>, cluster_token: Option<Arc<str>>) -> Self {
        Self {
            alerts,
            cluster_token,
        }
    }

    pub fn into_server(self) -> RealtimeServiceServer<Self> {
        RealtimeServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl RealtimeService for RealtimeCountsGrpc {
    async fn counts(
        &self,
        request: Request<RealtimeServiceCountsRequest>,
    ) -> Result<Response<RealtimeServiceCountsResponse>, Status> {
        authenticate(&request, self.cluster_token.as_deref())?;

        let rule_ids: Vec<Id> = request.into_inner().rule_ids.into_iter().map(Id).collect();

        if rule_ids.len() > MAX_RULES {
            return Err(Status::invalid_argument("too many rules in one request"));
        }

        let counts = self
            .alerts
            .counts(&rule_ids, TimestampMicros::now())
            .into_iter()
            .map(|(id, count)| RuleCount {
                rule_id: id.0,
                count,
            })
            .collect();

        Ok(Response::new(RealtimeServiceCountsResponse { counts }))
    }
}

fn authenticate<T>(request: &Request<T>, expected_token: Option<&str>) -> Result<(), Status> {
    let origin = request
        .metadata()
        .get(INTERNAL_ORIGIN_HEADER)
        .and_then(|value| value.to_str().ok());

    if origin != Some(REALTIME_COUNTS_ORIGIN) {
        return Err(Status::permission_denied(
            "internal real-time counts origin required",
        ));
    }

    let expected = expected_token
        .ok_or_else(|| Status::unauthenticated("cluster bearer is not configured"))?;

    let presented = request
        .metadata()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| Status::unauthenticated("missing cluster bearer"))?;

    if !crate::app::self_telemetry::cluster_token_matches(expected, presented) {
        return Err(Status::unauthenticated("invalid cluster bearer"));
    }

    Ok(())
}
