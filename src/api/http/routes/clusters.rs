// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Remote clusters CRUD。
//!
//! Owner-only。`token_secret_ref` 在 list/get 响应中 mask 为 `***`。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::{cluster::RemoteCluster, persistence::repositories::cluster::events::OrgLink},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/clusters", get(list).post(create))
        .route("/clusters/{id}", get(get_one).put(update).delete(delete))
        // 跨集群 org 映射（含 per-org token_secret_ref）：发送端按 link 过滤可发 org，
        // 接收端 remote_org→local_org 反查否则拒收。token_secret_ref 响应 mask。
        .route("/clusters/{id}/org_map", get(list_org_map).put(put_org_map))
        .route(
            "/clusters/{id}/org_map/{local_org_id}",
            axum::routing::delete(delete_org_map),
        )
        // 远端节点可见性：经 gRPC 调远端 NodeService.List，返回其活跃 querier/节点（运维 + 路由）。
        .route("/clusters/{id}/nodes", get(list_nodes))
}

#[derive(Debug, Deserialize)]
pub struct CreateReq {
    pub name: String,
    pub advertise_addr: String,
    pub token_secret_ref: String,
    #[serde(default = "default_true")]
    pub tls_verify: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateReq {
    pub name: String,
    pub advertise_addr: String,
    pub token_secret_ref: Option<String>,
    pub tls_verify: bool,
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct ClusterResp {
    pub id: String,
    pub name: String,
    pub advertise_addr: String,
    pub token_secret_ref: String, // masked
    pub tls_verify: bool,
    pub enabled: bool,
    /// gossip 发现、尚未由 admin 启用/配置的待审集群。
    pub discovered: bool,
    pub created_at_micros: i64,
    pub updated_at_micros: i64,
}

/// federated search 是商业 feature；OSS 无许可证 → 任何 remote_clusters
/// 操作 403。这等价于"禁止设置跨集群联邦"。
fn require_federation_license(state: &AppState) -> Result<()> {
    if !state.platform.license.has_feature("federated_search") {
        return Err(Error::forbidden("federated_search feature not licensed"));
    }
    Ok(())
}

fn mask(c: RemoteCluster) -> ClusterResp {
    ClusterResp {
        id: c.id.0,
        name: c.name,
        advertise_addr: c.advertise_addr,
        token_secret_ref: "***".to_string(),
        tls_verify: c.tls_verify,
        enabled: c.enabled,
        discovered: c.discovered,
        created_at_micros: c.created_at.0,
        updated_at_micros: c.updated_at.0,
    }
}

#[permission("org.settings.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<ClusterResp>>> {
    require_federation_license(&state)?;
    Ok(Json(
        state
            .cluster
            .remote_clusters
            .list()
            .await?
            .into_iter()
            .map(mask)
            .collect(),
    ))
}

#[permission("org.settings.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<ClusterResp>> {
    require_federation_license(&state)?;
    let c = state.cluster.remote_clusters.get(&Id(id)).await?;
    Ok(Json(mask(c)))
}

#[permission("org.settings.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Json<ClusterResp>> {
    require_federation_license(&state)?;
    let now = TimestampMicros::now();
    let c = RemoteCluster {
        id: Id::new(),
        name: req.name,
        advertise_addr: req.advertise_addr,
        token_secret_ref: req.token_secret_ref,
        tls_verify: req.tls_verify,
        enabled: req.enabled,
        discovered: false,
        created_at: now,
        updated_at: now,
    };
    let c = state.cluster.remote_clusters.create(c).await?;
    Ok(Json(mask(c)))
}

#[permission("org.settings.manage")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<ClusterResp>> {
    require_federation_license(&state)?;
    let id = Id(id);
    let existing = state.cluster.remote_clusters.get(&id).await?;
    let c = RemoteCluster {
        id: existing.id,
        name: req.name,
        advertise_addr: req.advertise_addr,
        token_secret_ref: req.token_secret_ref.unwrap_or(existing.token_secret_ref),
        tls_verify: req.tls_verify,
        enabled: req.enabled,
        // admin 编辑即把 gossip 发现的待审集群升级为正式配置（清掉 discovered 标记）。
        discovered: false,
        created_at: existing.created_at,
        updated_at: TimestampMicros::now(),
    };
    let c = state.cluster.remote_clusters.update(c).await?;
    Ok(Json(mask(c)))
}

#[permission("org.settings.manage")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    require_federation_license(&state)?;
    state.cluster.remote_clusters.delete(&Id(id)).await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}

// ===== org 映射（cluster_org_link）=====

#[derive(Debug, Deserialize)]
pub struct OrgMapReq {
    /// 本集群的 org id。
    pub local_org_id: String,
    /// 该集群上对应的远端 org id。
    pub remote_org_id: String,
    /// per-org token 引用（`env:VAR` / `cipher_keys:<id>`）；缺省回退集群级 token。
    #[serde(default)]
    pub token_secret_ref: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OrgMapResp {
    pub remote_cluster_id: String,
    pub local_org_id: String,
    pub remote_org_id: String,
    /// masked：有 per-org token → `***`，否则空串（回退集群级）。
    pub token_secret_ref: String,
}

fn mask_link(l: OrgLink) -> OrgMapResp {
    OrgMapResp {
        remote_cluster_id: l.remote_cluster_id.0,
        local_org_id: l.local_org_id.0,
        remote_org_id: l.remote_org_id.0,
        token_secret_ref: if l
            .token_secret_ref
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
        {
            "***".to_string()
        } else {
            String::new()
        },
    }
}

#[permission("org.settings.read")]
async fn list_org_map(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<OrgMapResp>>> {
    require_federation_license(&state)?;
    let links = state.cluster.org_link.list(&Id(id)).await?;
    Ok(Json(links.into_iter().map(mask_link).collect()))
}

#[permission("org.settings.manage")]
async fn put_org_map(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<OrgMapReq>,
) -> Result<Json<OrgMapResp>> {
    require_federation_license(&state)?;
    if req.local_org_id.trim().is_empty() || req.remote_org_id.trim().is_empty() {
        return Err(Error::invalid(
            "local_org_id and remote_org_id are required",
        ));
    }
    // 集群必须存在。
    let _ = state.cluster.remote_clusters.get(&Id(id.clone())).await?;
    let link = OrgLink {
        remote_cluster_id: Id(id),
        local_org_id: Id(req.local_org_id),
        remote_org_id: Id(req.remote_org_id),
        token_secret_ref: req.token_secret_ref.filter(|s| !s.trim().is_empty()),
    };
    state.cluster.org_link.upsert(link.clone()).await?;
    Ok(Json(mask_link(link)))
}

#[permission("org.settings.manage")]
async fn delete_org_map(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((id, local_org_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    require_federation_license(&state)?;
    state
        .cluster
        .org_link
        .delete(&Id(id), &Id(local_org_id))
        .await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}

// ===== 远端节点可见性（GET /clusters/{id}/nodes）=====

#[derive(Debug, Serialize)]
pub struct RemoteNodeResp {
    pub node_id: String,
    pub advertise_addr: String,
    pub roles: Vec<String>,
    pub version: String,
}

fn role_name(v: i32) -> String {
    use crate::protocol::cluster::v1::NodeRole;
    match NodeRole::try_from(v).unwrap_or(NodeRole::Unspecified) {
        NodeRole::Router => "router",
        NodeRole::Ingester => "ingester",
        NodeRole::Querier => "querier",
        NodeRole::Compactor => "compactor",
        NodeRole::AlertManager => "alert_manager",
        NodeRole::Standalone => "standalone",
        NodeRole::Unspecified => "unspecified",
    }
    .to_string()
}

#[permission("org.settings.read")]
async fn list_nodes(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<RemoteNodeResp>>> {
    use crate::{
        infra::{cluster::grpc_channel, secret::resolve_secret_ref},
        protocol::cluster::v1::{ListNodesRequest, node_service_client::NodeServiceClient},
    };

    require_federation_license(&state)?;
    let c = state.cluster.remote_clusters.get(&Id(id)).await?;
    let channel = grpc_channel::connect(&c.advertise_addr, c.tls_verify)
        .await
        .map_err(|e| Error::internal(format!("connect remote cluster: {e}")))?;
    let mut req = tonic::Request::new(ListNodesRequest { roles: vec![] });
    // token best-effort（NodeService 在可信端口不强校验；解析失败则不带 bearer）。
    if let Ok(token) = resolve_secret_ref(
        &c.token_secret_ref,
        &ctx.org_id,
        Some(state.cluster.secrets.as_ref()),
    )
    .await
    {
        grpc_channel::with_bearer(&mut req, &token)
            .map_err(|e| Error::internal(format!("bearer: {e}")))?;
    }
    let mut client = NodeServiceClient::new(channel);
    let resp = crate::shared::grpc_trace::call(
        req,
        "cluster.v1.NodeService",
        "List",
        crate::shared::grpc_trace::GrpcTarget::Internal,
        |request| client.list(request),
    )
    .await
    .map_err(|s| Error::internal(format!("remote NodeService.List: {}", s.message())))?;
    let nodes = resp
        .into_inner()
        .nodes
        .into_iter()
        .map(|n| RemoteNodeResp {
            node_id: n.node_id,
            advertise_addr: n.advertise_addr,
            roles: n.roles.iter().map(|r| role_name(*r)).collect(),
            version: n.version,
        })
        .collect();
    Ok(Json(nodes))
}
