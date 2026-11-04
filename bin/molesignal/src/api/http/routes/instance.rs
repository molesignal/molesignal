// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Instance info HTTP：暴露部署级（非用户级）配置给前端。
//!
//! - `GET /api/v1/instance` → `{ external_url, signup_enabled, signup_verifies_email, passkey_signin,
//!   version }` 数据源/RUM 接入页用它拼真实 intake URL（留空时前端按访问来源推导）；登录页和注册页
//!   读 `signup_enabled`、`signup_verifies_email`、`passkey_signin` 与 `version`。该接口无需认证，所以只给这几项：发布通道、
//!   commit、branch 等部署细节只出现在已认证的 `GET /api/v1/version`（前端「关于」弹窗）里。

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    api::{
        AppState,
        http::{client_ip, routes::auth::can_verify_email},
    },
    app::iam::{IamContext, mfa::passkey},
    domain::iam::{ClientIpResolverSettings, MAX_AUDIT_RETENTION_DAYS, permission},
    infra::persistence::repositories::audit_events::{AuditEvent, AuditRangeSummary},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod alerting;
mod data;
mod signin_limits;

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
        .route(
            "/settings/audit_retention",
            get(get_audit_retention).put(put_audit_retention),
        )
        .route(
            "/settings/audit_retention/preview",
            get(get_audit_retention_preview),
        )
        .merge(data::routes())
        .merge(alerting::routes())
        .merge(signin_limits::routes())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceResp {
    /// 对外访问 URL；留空表示前端应按当前访问来源（origin）推导。
    pub external_url: String,
    /// 是否开放自助注册：signin 页据此显示「注册」入口（公开读，无需认证）。
    pub signup_enabled: bool,
    /// 注册先验邮箱：注册页据此决定第一步问什么。
    pub signup_verifies_email: bool,
    /// 这个实例能不能用 Passkey 登录：signin 页据此显示「使用 Passkey 登录」。
    pub passkey_signin: bool,
    /// 产品版本：signin 页公开展示；不含发布通道、commit、branch（见模块文档）。
    pub version: &'static str,
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
        signup_verifies_email: can_verify_email(&state),
        passkey_signin: passkey::relying_party(&state.platform.external_url).is_ok(),
        version: env!("MOLESIGNAL_PRODUCT_VERSION"),
    }))
}

/// 自助注册策略：整个实例一份，平台设置（`sys.settings.signin.*`）。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignupPolicy {
    pub signup_enabled: bool,
    pub signup_require_approval: bool,
    /// 只读。实例不能发邮件时证明不了邮箱归属，注册一律走审批，不管 `signup_require_approval`。
    #[serde(default)]
    pub can_verify_email: bool,
}

#[permission("sys.settings.signin.read")]
async fn get_signup_policy(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
) -> Result<Json<SignupPolicy>> {
    let s = state.iam.instance_settings.get().await?;

    Ok(Json(SignupPolicy {
        signup_enabled: s.signup_enabled,
        signup_require_approval: s.signup_require_approval,
        can_verify_email: can_verify_email(&state),
    }))
}

#[permission("sys.settings.signin.manage")]
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
        can_verify_email: can_verify_email(&state),
    }))
}

/// 服务图数据来源模式：整个实例一份，平台设置（`sys.settings.topology.*`）。
#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceGraphSettings {
    /// `intake`（各进程内存配对 + flush）或 `storage`（单例 worker 从存储重算，跨节点正确）。
    pub source: String,
}

#[permission("sys.settings.topology.read")]
async fn get_service_graph(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
) -> Result<Json<ServiceGraphSettings>> {
    let s = state.iam.instance_settings.get().await?;

    Ok(Json(ServiceGraphSettings {
        source: s.service_graph_source,
    }))
}

#[permission("sys.settings.topology.manage")]
async fn put_service_graph(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Json(req): Json<ServiceGraphSettings>,
) -> Result<Json<ServiceGraphSettings>> {
    if req.source != "intake" && req.source != "storage" {
        return Err(Error::invalid("source must be 'intake' or 'storage'"));
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

/// 跨集群联邦配置：整个实例一份，平台设置（`sys.settings.topology.*`）。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

#[permission("sys.settings.topology.read")]
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

#[permission("sys.settings.topology.manage")]
async fn put_federation(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Json(req): Json<FederationSettings>,
) -> Result<Json<FederationSettings>> {
    let cluster_id = req.cluster_id.trim().to_string();

    if cluster_id.len() > 128 {
        return Err(Error::invalid("clusterId too long (max 128)"));
    }

    // 周期 / 批量 / 窗口须为正。
    if req.drain_interval_secs < 1
        || req.push_batch_size < 1
        || req.seen_events_ttl_secs < 1
        || req.gossip_interval_secs < 1
    {
        return Err(Error::invalid(
            "drainIntervalSecs / push_batch_size / seen_events_ttl_secs / gossip_interval_secs must be >= 1",
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
#[permission("sys.settings.signin.read")]
async fn get_client_ip(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<ClientIpResolverSettings>> {
    let settings = state.iam.instance_settings.get().await?;
    Ok(Json(settings.rum_client_ip_resolver))
}

#[permission("sys.settings.signin.manage")]
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

/// 审计记录保留天数：部署级设置，组织审计与平台审计共用；compactor 每天删除更早的记录，
/// 0 = 永久保留。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRetentionSettings {
    pub retention_days: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuditRetentionPreviewParams {
    retention_days: u32,
}

/// 把审计保留期改成 `retention_days` 会多删除的记录。审计记录在 Postgres 的表里，是按时间索引数
/// 出来的条数，不是估算。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRetentionPreview {
    pub retention_days: u32,
    pub previous_retention_days: u32,
    pub rows: u64,
    pub earliest_at: Option<i64>,
}

fn validate_audit_retention_days(days: u32) -> Result<()> {
    if days > MAX_AUDIT_RETENTION_DAYS {
        return Err(Error::invalid(format!(
            "retention_days must be between 0 and {MAX_AUDIT_RETENTION_DAYS}"
        )));
    }

    Ok(())
}

/// 保留起点从 `previous` 移到 `cutoff` 之后多删除的范围 `[from, before)`。`None` 是永久保留：
/// 移到永久保留或更早的起点不会多删除什么；从永久保留移过来，更早的记录都要删除。
fn newly_expired(
    previous: Option<TimestampMicros>,
    cutoff: Option<TimestampMicros>,
) -> Option<(Option<TimestampMicros>, TimestampMicros)> {
    let cutoff = cutoff?;

    match previous {
        Some(previous) if previous >= cutoff => None,
        previous => Some((previous, cutoff)),
    }
}

#[permission("sys.settings.retention.read")]
async fn get_audit_retention_preview(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<AuditRetentionPreviewParams>,
) -> Result<Json<AuditRetentionPreview>> {
    validate_audit_retention_days(params.retention_days)?;

    let mut settings = state.iam.instance_settings.get().await?;
    let previous_retention_days = settings.audit_retention_days;
    let now = TimestampMicros::now();
    let previous_cutoff = settings.audit_retention_cutoff(now);

    settings.audit_retention_days = params.retention_days;

    let summary = match newly_expired(previous_cutoff, settings.audit_retention_cutoff(now)) {
        Some((from, before)) => state.iam.audit_events.summarize_range(from, before).await?,
        None => AuditRangeSummary::default(),
    };

    Ok(Json(AuditRetentionPreview {
        retention_days: params.retention_days,
        previous_retention_days,
        rows: summary.rows,
        earliest_at: summary.earliest.map(|earliest| earliest.0),
    }))
}

#[permission("sys.settings.retention.read")]
async fn get_audit_retention(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<AuditRetentionSettings>> {
    let settings = state.iam.instance_settings.get().await?;

    Ok(Json(AuditRetentionSettings {
        retention_days: settings.audit_retention_days,
    }))
}

#[permission("sys.settings.retention.manage")]
async fn put_audit_retention(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<AuditRetentionSettings>,
) -> Result<Json<AuditRetentionSettings>> {
    validate_audit_retention_days(request.retention_days)?;

    // read-modify-write：保留其它实例设置。
    let mut settings = state.iam.instance_settings.get().await?;
    let previous = settings.audit_retention_days;
    settings.audit_retention_days = request.retention_days;
    settings.updated_at = TimestampMicros::now();
    let updated = state.iam.instance_settings.update(settings).await?;

    // 保留天数决定审计记录留多久，改动本身记入平台审计。
    if updated.audit_retention_days != previous {
        let event = AuditEvent {
            id: Id::new(),
            org_id: state.iam.system_org_id.clone(),
            actor_kind: "user".into(),
            actor_id: ctx.user_id.0.clone(),
            action: "audit_retention.update".into(),
            target_kind: Some("instance_settings".into()),
            target_id: None,
            ip: None,
            user_agent: None,
            payload: json!({
                "retentionDays": updated.audit_retention_days,
                "previousRetentionDays": previous,
            }),
            ts: TimestampMicros::now(),
        };

        if let Err(error) = state.iam.audit_events.record(event).await {
            tracing::warn!(%error, "audit retention change could not be audited");
        }
    }

    Ok(Json(AuditRetentionSettings {
        retention_days: updated.audit_retention_days,
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{InstanceResp, TimestampMicros, newly_expired, validate_audit_retention_days};

    #[test]
    fn a_shorter_audit_retention_deletes_from_the_old_start_up_to_the_new_one() {
        let (old, new) = (TimestampMicros(100), TimestampMicros(200));

        assert_eq!(newly_expired(Some(old), Some(new)), Some((Some(old), new)));

        // 调长或不变：什么也不会多删。
        assert_eq!(newly_expired(Some(new), Some(old)), None);
        assert_eq!(newly_expired(Some(old), Some(old)), None);
    }

    #[test]
    fn keeping_audit_records_forever_is_the_longest_retention_of_all() {
        let start = TimestampMicros(100);

        // 从永久保留改成有期限：更早的记录都要删。
        assert_eq!(newly_expired(None, Some(start)), Some((None, start)));

        // 改成永久保留，或者一直永久保留：一条也不删。
        assert_eq!(newly_expired(Some(start), None), None);
        assert_eq!(newly_expired(None, None), None);
    }

    #[test]
    fn audit_retention_preview_takes_the_days_the_setting_takes() {
        assert!(validate_audit_retention_days(0).is_ok());
        assert!(validate_audit_retention_days(3650).is_ok());
        assert!(validate_audit_retention_days(3651).is_err());
    }

    #[test]
    fn public_instance_info_exposes_only_url_signup_passkey_and_version() {
        let resp = InstanceResp {
            external_url: "https://signal.example.com".to_string(),
            signup_enabled: true,
            signup_verifies_email: true,
            passkey_signin: true,
            version: "26.0.0.0",
        };

        // 公开接口不带发布通道：登录页只展示版本号，部署成熟度留给已认证的 /version。
        assert_eq!(
            serde_json::to_value(&resp).expect("instance response serializes"),
            json!({
                "externalUrl": "https://signal.example.com",
                "signupEnabled": true,
                "signupVerifiesEmail": true,
                "passkeySignin": true,
                "version": "26.0.0.0",
            }),
        );
    }
}
