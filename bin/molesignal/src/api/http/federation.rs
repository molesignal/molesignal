// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 跨集群事件总线的发送端接入点：在 org 维度资源 CUD 成功后调 [`emit_cud`] 落一条
//! CloudEvent 进 outbox（best-effort，**绝不**让 CUD 失败）。后台 `cluster_event_sync`
//! worker 负责推送到远端。未启用联邦时立即返回（OSS 零开销）。
//!
//! 接收端 apply 在 [`crate::api::grpc::event_server`]——走 repo 层、不经本 helper，故不回环。

use serde::Serialize;

use crate::{
    api::AppState,
    domain::federation::{CloudEvent, CudAction, ResourceKind},
    shared::{ids::Id, time::TimestampMicros},
};

/// CUD 成功后发一条跨集群事件。`resource` 序列化进 `data`（delete 传 `{"id": ...}` 即可）。
pub async fn emit_cud<T: Serialize>(
    state: &AppState,
    org_id: &Id,
    kind: ResourceKind,
    action: CudAction,
    resource_id: &str,
    resource: &T,
) {
    // 运行时读实例设置：cluster_id 非空才发（DB 驱动、前端改即生效）。
    let settings = match state.iam.instance_settings.get().await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "federation: read instance_settings failed; skip emit");
            return;
        }
    };
    if !settings.federation_enabled() {
        return;
    }
    let cluster = settings.federation_cluster_id.trim();
    // 原子拿单调 Lamport 版本（writer = 本集群）。
    let version = match state
        .cluster
        .resource_version
        .bump_local(kind.as_str(), org_id, resource_id, cluster)
        .await
    {
        Ok(v) => v.max(0) as u64,
        Err(e) => {
            tracing::warn!(error = %e, kind = kind.as_str(), "federation: version bump failed; skip emit");
            return;
        }
    };
    let data = match serde_json::to_value(resource) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, kind = kind.as_str(), "federation: resource serialize failed; skip emit");
            return;
        }
    };
    let ev = CloudEvent::new(
        Id::new().0,
        cluster,
        kind,
        action,
        &org_id.0,
        resource_id,
        version,
        data,
        TimestampMicros::now().to_datetime().to_rfc3339(),
    );
    if let Err(e) = state.cluster.event_outbox.append(org_id, &ev).await {
        tracing::warn!(error = %e, kind = kind.as_str(), "federation: outbox append failed; event dropped");
    }
}

/// 删除事件的便捷 data 载荷：`{"id": "<id>"}`。
pub fn delete_payload(id: &str) -> serde_json::Value {
    serde_json::json!({ "id": id })
}
