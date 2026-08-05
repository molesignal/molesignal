// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Instance info HTTP：暴露部署级（非用户级）配置给前端。
//!
//! - `GET /api/v1/instance` → `{ external_url, signup_enabled, version, release_channel }`
//!   数据源/RUM 接入页用它拼真实 ingest URL（留空时前端按访问来源推导）。

use axum::{Extension, Json, Router, extract::State, routing::get};
use serde::{Deserialize, Serialize};

use crate::{
    api::{AppState, http::client_ip},
    app::iam::IamContext,
    domain::iam::{ClientIpResolverSettings, permission},
    shared::{Error, Result, build_info, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/instance", get(get_instance))
        .route(
            "/settings/signup",
            get(get_signup_policy).put(put_signup_policy),
        )
        .route(
            "/settings/service_graph",
            get(get_service_graph).put(put_service_graph),
        )
        .route(
            "/settings/federation",
            get(get_federation).put(put_federation),
        )
        .route("/settings/client_ip", get(get_client_ip).put(put_client_ip))
}

#[derive(Debug, Serialize)]
pub struct InstanceResp {
    /// 对外访问 URL；留空表示前端应按当前访问来源（origin）推导。
    pub external_url: String,
    /// 是否开放自助注册：signin 页据此显示「注册」入口（公开读，无需认证）。
    pub signup_enabled: bool,
    /// 产品版本与部署发布通道：signin 页公开展示，不包含 commit/branch。
    pub version: &'static str,
    pub release_channel: &'static str,
}

async fn get_instance(State(state): State<AppState>) -> Result<Json<InstanceResp>> {
    let signup_enabled = state
        .iam
        .instance_settings
        .get()
        .await
        .map(|s| s.signup_enabled)
        .unwrap_or(false);
    Ok(Json(InstanceResp {
        external_url: state.platform.external_url.clone(),
        signup_enabled,
        version: env!("MOLESIGNAL_PRODUCT_VERSION"),
        release_channel: build_info::release_channel(),
    }))
}

/// 自助注册策略：`org.settings.read` 读取，`org.settings.manage` 写入。
#[derive(Debug, Serialize, Deserialize)]
pub struct SignupPolicy {
    pub signup_enabled: bool,
    pub signup_require_approval: bool,
}

#[permission("org.settings.read")]
async fn get_signup_policy(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
) -> Result<Json<SignupPolicy>> {
    let s = state.iam.instance_settings.get().await?;
    Ok(Json(SignupPolicy {
        signup_enabled: s.signup_enabled,
        signup_require_approval: s.signup_require_approval,
    }))
}

#[permission("org.settings.manage")]
async fn put_signup_policy(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Json(req): Json<SignupPolicy>,
) -> Result<Json<SignupPolicy>> {
    // read-modify-write：保留 service_graph_source 等其它实例设置。
    let mut s = state.iam.instance_settings.get().await?;
    s.signup_enabled = req.signup_enabled;
    s.signup_require_approval = req.signup_require_approval;
    s.updated_at = TimestampMicros::now();
    let updated = state.iam.instance_settings.update(s).await?;
    Ok(Json(SignupPolicy {
        signup_enabled: updated.signup_enabled,
        signup_require_approval: updated.signup_require_approval,
    }))
}

/// 服务图数据来源模式：`org.settings.read` 读取，`org.settings.manage` 写入。
#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceGraphSettings {
    /// `ingest`（各进程内存配对 + flush）或 `storage`（单例 worker 从存储重算，跨节点正确）。
    pub source: String,
}

#[permission("org.settings.read")]
async fn get_service_graph(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
) -> Result<Json<ServiceGraphSettings>> {
    let s = state.iam.instance_settings.get().await?;
    Ok(Json(ServiceGraphSettings {
        source: s.service_graph_source,
    }))
}

#[permission("org.settings.manage")]
async fn put_service_graph(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Json(req): Json<ServiceGraphSettings>,
) -> Result<Json<ServiceGraphSettings>> {
    if req.source != "ingest" && req.source != "storage" {
        return Err(Error::invalid("source must be 'ingest' or 'storage'"));
    }
    // read-modify-write：保留 signup 等其它实例设置。
    let mut s = state.iam.instance_settings.get().await?;
    s.service_graph_source = req.source;
    s.updated_at = TimestampMicros::now();
    let updated = state.iam.instance_settings.update(s).await?;
    Ok(Json(ServiceGraphSettings {
        source: updated.service_graph_source,
    }))
}

/// 跨集群联邦配置：`org.settings.read` 读取，`org.settings.manage` 写入。
#[derive(Debug, Serialize, Deserialize)]
pub struct FederationSettings {
    /// 本集群稳定唯一 id（事件 source/writer）；非空 = 启用联邦，留空 = 关闭。
    pub cluster_id: String,
    /// outbox drain → 推送远端周期（秒）。
    pub drain_interval_secs: i64,
    /// 单次推送批量上限。
    pub push_batch_size: i64,
    /// 接收端去重表保留窗口（秒）。
    pub seen_events_ttl_secs: i64,
    /// 集群拓扑 gossip 周期（秒）。
    pub gossip_interval_secs: i64,
}

#[permission("org.settings.read")]
async fn get_federation(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
) -> Result<Json<FederationSettings>> {
    let s = state.iam.instance_settings.get().await?;
    Ok(Json(FederationSettings {
        cluster_id: s.federation_cluster_id,
        drain_interval_secs: s.federation_drain_interval_secs,
        push_batch_size: s.federation_push_batch_size,
        seen_events_ttl_secs: s.federation_seen_events_ttl_secs,
        gossip_interval_secs: s.federation_gossip_interval_secs,
    }))
}

#[permission("org.settings.manage")]
async fn put_federation(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Json(req): Json<FederationSettings>,
) -> Result<Json<FederationSettings>> {
    let cluster_id = req.cluster_id.trim().to_string();
    if cluster_id.len() > 128 {
        return Err(Error::invalid("cluster_id too long (max 128)"));
    }
    // 周期 / 批量 / 窗口须为正。
    if req.drain_interval_secs < 1
        || req.push_batch_size < 1
        || req.seen_events_ttl_secs < 1
        || req.gossip_interval_secs < 1
    {
        return Err(Error::invalid(
            "drain_interval_secs / push_batch_size / seen_events_ttl_secs / gossip_interval_secs must be >= 1",
        ));
    }
    // read-modify-write：保留 signup / service_graph 等其它实例设置。
    let mut s = state.iam.instance_settings.get().await?;
    s.federation_cluster_id = cluster_id;
    s.federation_drain_interval_secs = req.drain_interval_secs;
    s.federation_push_batch_size = req.push_batch_size;
    s.federation_seen_events_ttl_secs = req.seen_events_ttl_secs;
    s.federation_gossip_interval_secs = req.gossip_interval_secs;
    s.updated_at = TimestampMicros::now();
    let updated = state.iam.instance_settings.update(s).await?;
    Ok(Json(FederationSettings {
        cluster_id: updated.federation_cluster_id,
        drain_interval_secs: updated.federation_drain_interval_secs,
        push_batch_size: updated.federation_push_batch_size,
        seen_events_ttl_secs: updated.federation_seen_events_ttl_secs,
        gossip_interval_secs: updated.federation_gossip_interval_secs,
    }))
}

/// RUM 客户端 IP 识别是部署级设置；代理 Header 仅在连接来源命中可信 CIDR 时生效。
#[permission("sys.settings.manage")]
async fn get_client_ip(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<ClientIpResolverSettings>> {
    let settings = state.iam.instance_settings.get().await?;
    Ok(Json(settings.rum_client_ip_resolver))
}

#[permission("sys.settings.manage")]
async fn put_client_ip(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Extension(resolver): Extension<client_ip::ClientIpResolverHandle>,
    Json(request): Json<ClientIpResolverSettings>,
) -> Result<Json<ClientIpResolverSettings>> {
    // 先完整校验并规范化，避免把当前进程无法加载的配置写入数据库。
    let normalized = client_ip::normalize_settings(request)?;
    let mut settings = state.iam.instance_settings.get().await?;
    settings.rum_client_ip_resolver = normalized;
    settings.updated_at = TimestampMicros::now();
    let updated = state.iam.instance_settings.update(settings).await?;

    // 本节点立即生效；其它节点由后台数据库快照刷新接收变更。
    resolver.replace(&updated.rum_client_ip_resolver)?;
    Ok(Json(updated.rum_client_ip_resolver))
}
