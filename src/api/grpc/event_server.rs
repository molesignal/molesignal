// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `cluster.v1.EventService` 的 gRPC server 实装：跨集群事件总线的**接收端**。
//!
//! 远端集群把 org 维度资源的 CUD 变更（CloudEvent）推到本节点的 `PushEvents`。
//! 逐条：
//! 1. **去重**：`seen_events.insert_if_absent`，已见过 → 幂等接受（回环 / 重投兜底）。
//! 2. **org 映射**：按 `subject` 的 remote_org 反查 [`ClusterOrgLinkRepository`] →
//!    本地 org；未映射 → 拒收（不信任未声明的 org）。
//! 3. **per-org token 鉴权**：bearer 校验（一次 / 批），要求 `token.org == 映射的本地 org`
//!    （复用 Flight server 的 `token.org == req.org` 范式——token 证明发送方有权写该 org）。
//! 4. **Lamport 冲突解决**：拿本地版本，`wins((incoming.version, writer), local)` 否则跳过
//!    （旧 / 输的写不覆盖，时钟无关）；胜出则 upsert / delete 到本地 repo + `adopt` 版本。
//!
//! apply 走 repo 层、**不经 `emit_cud`**，故不回环。失败仅记 rejected + warn，下次该资源
//! 本地再编辑会带更高版本重新同步（最终一致）。

use std::sync::OnceLock;

use tonic::{Request, Response, Status};

use crate::{
    api::{AppState, http::middleware::auth::authenticate_api_token},
    domain::{
        alerting::{
            escalation::EscalationPolicy, rule::AlertRule, schedule::Schedule,
            semantic_group::SemanticGroup,
        },
        dashboard::Dashboard,
        federation::{CloudEvent, CudAction, ResourceKind, parse_event_type, parse_subject, wins},
    },
    infra::persistence::repositories::{log_patterns::LogPattern, regex_patterns::RegexPattern},
    protocol::cluster::v1::{
        CancelQueryRequest, CancelQueryResponse, ClusterRef, GossipRequest, GossipResponse,
        PushEventsRequest, PushEventsResponse,
        event_service_server::{EventService, EventServiceServer},
    },
    shared::{Error, Result, ids::Id},
};

static APPLIED: OnceLock<prometheus::IntCounterVec> = OnceLock::new();

/// 瞬时失败（可重试）vs 终态拒绝（重试无益）。DB 抖动等映射为 `Internal`/`Unavailable`/
/// `ResourceExhausted`/`Other`；policy / 解析错误（Forbidden/Unauthorized/Invalid/NotFound…）
/// 为终态。
fn is_transient(e: &Error) -> bool {
    matches!(
        e,
        Error::Internal(_) | Error::Unavailable(_) | Error::ResourceExhausted(_) | Error::Other(_)
    )
}

/// `federation_events_applied_total{result}`：接收端逐事件结果
/// （`applied` 落地 / `skipped` 去重或旧版本 / `rejected` 终态拒绝 / `retry` 瞬时待重投）。
fn applied_vec() -> &'static prometheus::IntCounterVec {
    APPLIED.get_or_init(|| {
        crate::shared::metrics::register_int_counter_vec(
            "federation_events_applied_total",
            "cross-cluster events processed by the receiver, by result",
            &["result"],
        )
    })
}

pub struct EventServiceGrpc {
    state: AppState,
}

impl EventServiceGrpc {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub fn into_server(self) -> EventServiceServer<Self> {
        EventServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl EventService for EventServiceGrpc {
    async fn push_events(
        &self,
        request: Request<PushEventsRequest>,
    ) -> std::result::Result<Response<PushEventsResponse>, Status> {
        // bearer：批内所有事件共用发送端的 per-org token（worker 按 org 分组发送）；
        // argon2 校验昂贵，**整批只验一次**，把 token 的 org 下传给逐条的映射断言。
        let bearer = request
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer ").map(str::to_string));
        let req = request.into_inner();
        let source = Id(req.source_cluster_id);
        let token_org: Option<Id> = match &bearer {
            Some(t) => authenticate_api_token(
                t,
                self.state.iam.service.as_ref(),
                self.state.iam.api_tokens.clone(),
            )
            .await
            .ok()
            .map(|ctx| ctx.org_id),
            None => None,
        };

        let mut accepted = Vec::new();
        let mut rejected = Vec::new();
        let mut retry = Vec::new();
        for bytes in &req.events_json {
            let ev: CloudEvent = match serde_json::from_slice(bytes) {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(error = %e, "federation apply: malformed CloudEvent; drop");
                    continue; // 无法解析 → 拿不到 id，无从去重 / 回报，丢弃。
                }
            };
            // 去重：首见才处理；已见过 → 幂等接受（不重复 apply）。
            match self
                .state
                .cluster
                .seen_events
                .insert_if_absent(&ev.id)
                .await
            {
                Ok(true) => {}
                Ok(false) => {
                    applied_vec().with_label_values(&["skipped"]).inc();
                    accepted.push(ev.id.clone());
                    continue;
                }
                Err(e) => {
                    // 去重表本身 DB 抖动 → 瞬时，请发送端重投（不推进游标）。
                    tracing::warn!(error = %e, id = %ev.id, "federation apply: seen dedup failed (transient)");
                    applied_vec().with_label_values(&["retry"]).inc();
                    retry.push(ev.id.clone());
                    continue;
                }
            }
            match apply_event(&self.state, &source, token_org.as_ref(), &ev).await {
                Ok(true) => {
                    applied_vec().with_label_values(&["applied"]).inc();
                    accepted.push(ev.id.clone());
                }
                Ok(false) => {
                    applied_vec().with_label_values(&["skipped"]).inc();
                    accepted.push(ev.id.clone());
                }
                Err(e) if is_transient(&e) => {
                    // 瞬时失败（DB 抖动等）：**撤销 seen 标记**使重投能再处理，并请发送端重投。
                    tracing::warn!(error = %e, id = %ev.id, ty = %ev.event_type, "federation apply transient; will retry");
                    let _ = self.state.cluster.seen_events.remove(&ev.id).await;
                    applied_vec().with_label_values(&["retry"]).inc();
                    retry.push(ev.id.clone());
                }
                Err(e) => {
                    // 终态拒绝（policy / 解析）：重试也不会成功，发送端推进游标过它。
                    tracing::warn!(error = %e, id = %ev.id, ty = %ev.event_type, "federation apply rejected (terminal)");
                    applied_vec().with_label_values(&["rejected"]).inc();
                    rejected.push(ev.id.clone());
                }
            }
        }
        Ok(Response::new(PushEventsResponse {
            accepted_ids: accepted,
            rejected_ids: rejected,
            retry_ids: retry,
        }))
    }

    /// 跨集群查询取消（#12）：coordinator 取消联邦查询时通知曾参与的远端。
    /// bearer 须为合法 api token（证明是已知 peer；fed_id 本就是不可猜 KSUID，不再按 org 收口）。
    /// best-effort：命中即置位本地子查询的 cancel 标志，未命中返回 `cancelled=false`。
    async fn cancel_query(
        &self,
        request: Request<CancelQueryRequest>,
    ) -> std::result::Result<Response<CancelQueryResponse>, Status> {
        let bearer = request
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer ").map(str::to_string))
            .ok_or_else(|| Status::unauthenticated("missing bearer token"))?;
        authenticate_api_token(
            &bearer,
            self.state.iam.service.as_ref(),
            self.state.iam.api_tokens.clone(),
        )
        .await
        .map_err(|e| Status::unauthenticated(e.to_string()))?;
        let fed_id = request.into_inner().federation_query_id;
        let cancelled = self.state.cluster.federation_cancel.cancel(&fed_id);
        Ok(Response::new(CancelQueryResponse { cancelled }))
    }

    /// gossip 节点发现（#12）：合并对端"已知集群"（未知者入库为 discovered，不自动信任），
    /// 回传本端拓扑。仅传播 id/name/addr —— token / org 映射绝不出网。
    async fn gossip_clusters(
        &self,
        request: Request<GossipRequest>,
    ) -> std::result::Result<Response<GossipResponse>, Status> {
        let bearer = request
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer ").map(str::to_string))
            .ok_or_else(|| Status::unauthenticated("missing bearer token"))?;
        authenticate_api_token(
            &bearer,
            self.state.iam.service.as_ref(),
            self.state.iam.api_tokens.clone(),
        )
        .await
        .map_err(|e| Status::unauthenticated(e.to_string()))?;
        let incoming = request.into_inner().known;
        let known = merge_gossip(&self.state, &incoming).await;
        Ok(Response::new(GossipResponse { known }))
    }
}

/// gossip 合并：未知集群（按 id，排除本集群）入库为 discovered；返回本端已知拓扑。
/// 发送端（worker）与接收端（server handler）共用，双向都学到新拓扑。
pub async fn merge_gossip(state: &AppState, incoming: &[ClusterRef]) -> Vec<ClusterRef> {
    let self_id = state
        .iam
        .instance_settings
        .get()
        .await
        .map(|s| s.federation_cluster_id)
        .unwrap_or_default();
    let mine = state
        .cluster
        .remote_clusters
        .list()
        .await
        .unwrap_or_default();
    let known: std::collections::HashSet<String> = mine.iter().map(|c| c.id.0.clone()).collect();
    for r in incoming {
        if r.id.is_empty() || r.id == self_id || known.contains(&r.id) {
            continue; // 已知 / 自身 / 空 id → 跳过。
        }
        let _ = state
            .cluster
            .remote_clusters
            .insert_discovered(&r.id, &r.name, &r.advertise_addr)
            .await;
    }
    mine.into_iter()
        .map(|c| ClusterRef {
            id: c.id.0,
            name: c.name,
            advertise_addr: c.advertise_addr,
        })
        .collect()
}

/// 把一条远端 CloudEvent 应用到本地：org 映射 + token 鉴权 + Lamport 冲突解决 + repo 落地。
/// `token_org` 是该批 bearer 解出的 org（None = 缺失 / 非法 token）。
/// 返回 `true` = 实际落地，`false` = 旧 / 同版本被跳过（幂等，仍算成功接受）。
pub async fn apply_event(
    state: &AppState,
    source_cluster: &Id,
    token_org: Option<&Id>,
    ev: &CloudEvent,
) -> Result<bool> {
    let (remote_org, resource_id) = parse_subject(&ev.subject)
        .ok_or_else(|| Error::invalid("federation: malformed subject"))?;
    // 1. org 映射：远端 org → 本地 org；未映射 → 拒收。
    let link = state
        .cluster
        .org_link
        .get_for_remote(source_cluster, &Id(remote_org.to_string()))
        .await?
        .ok_or_else(|| Error::forbidden("federation: remote org not mapped"))?;
    let local_org = link.local_org_id;
    // 2. per-org token 鉴权：token.org 必须 == 映射出的本地 org。
    let token_org =
        token_org.ok_or_else(|| Error::unauthorized("federation: missing/invalid bearer"))?;
    if token_org.0 != local_org.0 {
        return Err(Error::forbidden(
            "federation: token org does not match mapped org",
        ));
    }
    // 3. 解析资源类型 + 动作。
    let (kind, action) = parse_event_type(&ev.event_type)
        .ok_or_else(|| Error::invalid("federation: unknown event type"))?;
    // 4. Lamport 冲突解决：旧 / 输的写不覆盖（时钟无关）。
    let local = state
        .cluster
        .resource_version
        .get(kind.as_str(), &local_org, resource_id)
        .await?;
    let local_pair = local
        .as_ref()
        .map(|(v, w)| (*v as u64, w.as_str()))
        .unwrap_or((0, ""));
    if !wins((ev.xmsversion, &ev.xmswriter), local_pair) {
        return Ok(false); // 已是最新 / 更旧 → 幂等跳过（仍算成功接受）。
    }
    // 5. 落地到本地 repo（org 改写为本地 org），再采纳远端版本。
    apply_resource(state, kind, action, &local_org, resource_id, &ev.data).await?;
    state
        .cluster
        .resource_version
        .adopt(
            kind.as_str(),
            &local_org,
            resource_id,
            ev.xmsversion as i64,
            &ev.xmswriter,
        )
        .await?;
    Ok(true)
}

/// id-scoped 资源（有 `get(&Id)`/`create`/`update`/`delete(&Id)` + 字段 `id`/`org_id`）的
/// apply：Deleted → delete；Created/Updated → 反序列化 + org/id 改写为本地权威值 + upsert
/// （get 探在否决定 create / update，repo 层不依赖 update 的 NotFound 语义）。
macro_rules! apply_id_scoped {
    ($repo:expr, $ty:ty, $action:expr, $local_org:expr, $res_id:expr, $data:expr) => {{
        let repo = $repo;
        match $action {
            CudAction::Deleted => {
                repo.delete(&Id($res_id.to_string())).await?;
            }
            CudAction::Created | CudAction::Updated => {
                let mut r: $ty = serde_json::from_value($data.clone())
                    .map_err(|e| Error::invalid(format!("federation payload: {e}")))?;
                r.org_id = $local_org.clone();
                r.id = Id($res_id.to_string());
                match repo.get(&r.id).await {
                    Ok(_) => {
                        repo.update(r).await?;
                    }
                    Err(Error::NotFound(_)) => {
                        repo.create(r).await?;
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(())
    }};
}

/// 按资源类型把 CloudEvent.data 落到本地 repo（org 已改写为本地）。
/// 加新资源 = 加一个分支（与发送端 `emit_cud` 调用点一一对应）。
async fn apply_resource(
    state: &AppState,
    kind: ResourceKind,
    action: CudAction,
    local_org: &Id,
    resource_id: &str,
    data: &serde_json::Value,
) -> Result<()> {
    match kind {
        ResourceKind::RegexPattern => {
            apply_regex_pattern(state, action, local_org, resource_id, data).await
        }
        ResourceKind::AlertRule => {
            apply_id_scoped!(
                &state.alerting.service.rules,
                AlertRule,
                action,
                local_org,
                resource_id,
                data
            )
        }
        ResourceKind::EscalationPolicy => apply_id_scoped!(
            &state.alerting.service.escalations,
            EscalationPolicy,
            action,
            local_org,
            resource_id,
            data
        ),
        ResourceKind::Schedule => apply_id_scoped!(
            &state.alerting.service.schedules,
            Schedule,
            action,
            local_org,
            resource_id,
            data
        ),
        ResourceKind::SemanticGroup => apply_id_scoped!(
            &state.alerting.semantic_groups,
            SemanticGroup,
            action,
            local_org,
            resource_id,
            data
        ),
        ResourceKind::LogPattern => {
            apply_log_pattern(state, action, local_org, resource_id, data).await
        }
        ResourceKind::Dashboard => {
            apply_dashboard(state, action, local_org, resource_id, data).await
        }
        ResourceKind::MuteRule => apply_id_scoped!(
            &state.alerting.mute_rules,
            crate::domain::alerting::mute::MuteRule,
            action,
            local_org,
            resource_id,
            data
        ),
        ResourceKind::NotifyTemplate => {
            apply_notify_template(state, action, local_org, resource_id, data).await
        }
    }
}

/// LogPattern：repo 的 get / delete 带显式 org_id（与 id-scoped 资源略异）。
async fn apply_log_pattern(
    state: &AppState,
    action: CudAction,
    local_org: &Id,
    resource_id: &str,
    data: &serde_json::Value,
) -> Result<()> {
    let repo = &state.storage.log_patterns;
    match action {
        CudAction::Deleted => {
            repo.delete(local_org, &Id(resource_id.to_string())).await?;
        }
        CudAction::Created | CudAction::Updated => {
            let mut p: LogPattern = serde_json::from_value(data.clone())
                .map_err(|e| Error::invalid(format!("federation payload: {e}")))?;
            p.org_id = local_org.clone();
            p.id = Id(resource_id.to_string());
            match repo.get(local_org, &p.id).await {
                Ok(_) => {
                    repo.update(p).await?;
                }
                Err(Error::NotFound(_)) => {
                    repo.create(p).await?;
                }
                Err(e) => return Err(e),
            }
        }
    }
    Ok(())
}

/// NotifyTemplate：repo 的 get / delete 带显式 org_id（同 LogPattern）。
async fn apply_notify_template(
    state: &AppState,
    action: CudAction,
    local_org: &Id,
    resource_id: &str,
    data: &serde_json::Value,
) -> Result<()> {
    let repo = &state.alerting.templates;
    match action {
        CudAction::Deleted => {
            repo.delete(local_org, &Id(resource_id.to_string())).await?;
        }
        CudAction::Created | CudAction::Updated => {
            let mut template:
                crate::infra::persistence::repositories::notify::NotifyTemplateRecord =
                serde_json::from_value(data.clone())
                    .map_err(|e| Error::invalid(format!("federation payload: {e}")))?;
            template.organization_id = local_org.clone();
            template.id = Id(resource_id.to_string());
            match repo.get(local_org, &template.id).await {
                Ok(_) => {
                    repo.update(template).await?;
                }
                Err(Error::NotFound(_)) => {
                    repo.create(template).await?;
                }
                Err(e) => return Err(e),
            }
        }
    }
    Ok(())
}

/// Dashboard：经 `DashboardService::upsert`（service 持私有 repo）。
async fn apply_dashboard(
    state: &AppState,
    action: CudAction,
    local_org: &Id,
    resource_id: &str,
    data: &serde_json::Value,
) -> Result<()> {
    match action {
        CudAction::Deleted => {
            state.dashboard.delete(&Id(resource_id.to_string())).await?;
        }
        CudAction::Created | CudAction::Updated => {
            let mut d: Dashboard = serde_json::from_value(data.clone())
                .map_err(|e| Error::invalid(format!("federation payload: {e}")))?;
            d.org_id = local_org.clone();
            d.id = Id(resource_id.to_string());
            state.dashboard.upsert(d).await?;
        }
    }
    Ok(())
}

async fn apply_regex_pattern(
    state: &AppState,
    action: CudAction,
    local_org: &Id,
    resource_id: &str,
    data: &serde_json::Value,
) -> Result<()> {
    match action {
        CudAction::Deleted => {
            state
                .storage
                .regex_patterns
                .delete(local_org, &Id(resource_id.to_string()))
                .await?;
        }
        CudAction::Created | CudAction::Updated => {
            let mut p: RegexPattern = serde_json::from_value(data.clone())
                .map_err(|e| Error::invalid(format!("federation: regex pattern payload: {e}")))?;
            // 远端 org / id 改写为本地权威值（subject 才是真相）。
            p.org_id = local_org.clone();
            p.id = Id(resource_id.to_string());
            upsert_regex_pattern(state, p).await?;
        }
    }
    state.storage.masking.invalidate(local_org).await;
    Ok(())
}

/// upsert 语义：先 update，缺失则 create（apply 与发送端 create/update 解耦，幂等覆盖）。
async fn upsert_regex_pattern(
    state: &AppState,
    p: crate::infra::persistence::repositories::regex_patterns::RegexPattern,
) -> Result<()> {
    match state.storage.regex_patterns.update(p.clone()).await {
        Ok(_) => Ok(()),
        Err(Error::NotFound(_)) => {
            state.storage.regex_patterns.create(p).await?;
            Ok(())
        }
        Err(e) => Err(e),
    }
}
