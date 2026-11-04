// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Kubernetes 集群的登记，给 Kubernetes 修复动作用：
//!
//! - `GET/POST /alerts/remediation/clusters`、`PUT/DELETE /alerts/remediation/clusters/{id}`：看要
//!   `alerts.remediation.read`，改要 `alerts.remediation.manage`。ServiceAccount Token 不出库，修改时留空保持原样；还有动作在用的
//!   集群不能删。
//! - `POST /alerts/remediation/clusters/{id}/test`：读一次集群版本。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{alerting::remediation::kubernetes, iam::IamContext},
    domain::{
        alerting::remediation::{KubernetesCluster, RemediationTarget},
        iam::permission,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/alerts/remediation/clusters",
            get(list_clusters).post(create_cluster),
        )
        .route(
            "/alerts/remediation/clusters/{id}",
            put(update_cluster).delete(delete_cluster),
        )
        .route("/alerts/remediation/clusters/{id}/test", post(test_cluster))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClusterReq {
    name: String,
    api_server: String,
    #[serde(default)]
    ca_cert: String,
    #[serde(default)]
    token: String,
}

#[permission("alerts.remediation.read")]
async fn list_clusters(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let clusters = state
        .alerting
        .remediation
        .repository()
        .list_clusters(&ctx.org_id)
        .await?;

    Ok(Json(json!({ "clusters": clusters })))
}

#[permission("alerts.remediation.manage")]
async fn create_cluster(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<ClusterReq>,
) -> Result<Json<Value>> {
    if req.token.trim().is_empty() {
        return Err(Error::invalid("a cluster needs a ServiceAccount token"));
    }

    let now = TimestampMicros::now();
    let mut cluster = KubernetesCluster {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name: req.name,
        api_server: req.api_server,
        ca_cert: req.ca_cert,
        token: req.token.trim().to_string(),
        created_at: now,
        updated_at: now,
    };

    cluster.validate()?;

    let saved = state
        .alerting
        .remediation
        .repository()
        .save_cluster(cluster)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "kubernetes_cluster.created",
        "kubernetes_cluster",
        &saved.id.0,
        json!({ "name": saved.name, "apiServer": saved.api_server }),
    )
    .await;

    Ok(Json(serde_json::to_value(saved).unwrap_or(Value::Null)))
}

#[permission("alerts.remediation.manage")]
async fn update_cluster(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<ClusterReq>,
) -> Result<Json<Value>> {
    let repository = state.alerting.remediation.repository();
    let existing = repository
        .get_cluster(&ctx.org_id, &Id::from_string(id))
        .await?
        .ok_or_else(|| Error::not_found("cluster not found"))?;

    let mut cluster = KubernetesCluster {
        name: req.name,
        api_server: req.api_server,
        ca_cert: req.ca_cert,
        token: if req.token.trim().is_empty() {
            existing.token.clone()
        } else {
            req.token.trim().to_string()
        },
        updated_at: TimestampMicros::now(),
        ..existing
    };

    cluster.validate()?;

    let saved = repository.save_cluster(cluster).await?;

    activity_audit::record(
        &state,
        &ctx,
        "kubernetes_cluster.updated",
        "kubernetes_cluster",
        &saved.id.0,
        json!({ "name": saved.name, "apiServer": saved.api_server }),
    )
    .await;

    Ok(Json(serde_json::to_value(saved).unwrap_or(Value::Null)))
}

#[permission("alerts.remediation.manage")]
async fn delete_cluster(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let id = Id::from_string(id);
    let repository = state.alerting.remediation.repository();

    let used = repository.list_actions(&ctx.org_id).await?.into_iter().find(|action| {
        matches!(&action.target, RemediationTarget::Kubernetes(kubernetes) if kubernetes.cluster_id == id)
    });

    if let Some(action) = used {
        return Err(Error::invalid(format!(
            "the remediation action `{}` uses this cluster",
            action.name
        )));
    }

    if !repository.delete_cluster(&ctx.org_id, &id).await? {
        return Err(Error::not_found("cluster not found"));
    }

    activity_audit::record(
        &state,
        &ctx,
        "kubernetes_cluster.deleted",
        "kubernetes_cluster",
        &id.0,
        json!({}),
    )
    .await;

    Ok(Json(json!({ "deleted": true })))
}

#[permission("alerts.remediation.manage")]
async fn test_cluster(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let cluster = state
        .alerting
        .remediation
        .repository()
        .get_cluster(&ctx.org_id, &Id::from_string(id))
        .await?
        .ok_or_else(|| Error::not_found("cluster not found"))?;

    let version = kubernetes::check(&cluster).await?;

    Ok(Json(json!({ "ok": true, "version": version })))
}
