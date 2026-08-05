// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警规则、Incident 与升级策略路由。
//!
//! 当前实装：list / get / ack / resolve；create / update 接受前端构造的完整 domain
//! 结构（DTO 层后续可拆得更薄）。

mod resources;

use std::collections::{BTreeMap, HashMap, HashSet};

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use chrono::{TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    api::{
        AppState,
        http::federation::{delete_payload, emit_cud},
    },
    app::iam::IamContext,
    domain::{
        alerting::{
            anomaly::{AnomalyParams, MAX_ANOMALY_LOOKBACK_DAYS, SUPPORTED_DETECTORS},
            escalation::{EscalationPolicy, EscalationStep},
            incident::{Incident, IncidentStatus, Severity},
            rule::{
                AlertQuery, AlertRule, AlertRuleKind, AlertTrigger, RuleState, SeverityThreshold,
            },
        },
        federation::{CudAction, ResourceKind},
        iam::{permission, resource_permission},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/alerts/rules", get(list_rules).post(create_rule))
        .route(
            "/alerts/rules/{id}",
            get(get_rule).put(update_rule).delete(delete_rule),
        )
        .route("/alerts/incidents", get(list_incidents))
        .route("/alerts/insights", get(get_insights))
        .route("/alerts/incidents/{id}", get(get_incident))
        .route(
            "/alerts/incidents/{id}/rca",
            get(get_incident_rca).post(generate_incident_rca),
        )
        .route("/alerts/incidents/{id}/ack", post(ack_incident))
        .route("/alerts/incidents/{id}/resolve", post(resolve_incident))
        .route(
            "/alerts/escalations",
            get(list_escalations).post(create_escalation),
        )
        .route(
            "/alerts/escalations/{id}",
            get(get_escalation)
                .put(update_escalation)
                .delete(delete_escalation),
        )
}

#[derive(Debug, Deserialize)]
struct RuleWriteReq {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    kind: AlertRuleKind,
    query: AlertQuery,
    trigger: AlertTrigger,
    /// 多档严重度阈值（夜莺式多档触发）；空 = 走单档 `trigger`。
    #[serde(default)]
    thresholds: Vec<SeverityThreshold>,
    /// 单档规则的显式兜底严重度。
    #[serde(default)]
    severity: Option<Severity>,
    /// 仅 `kind = anomaly` 时必填；其它 kind 忽略并落 `None`。
    #[serde(default)]
    anomaly_params: Option<AnomalyParams>,
    escalation_policy_id: Option<Id>,
    #[serde(default)]
    labels: BTreeMap<String, String>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
}

fn default_true() -> bool {
    true
}

/// 校验并规整 `kind` / `anomaly_params`：
/// - `kind = Anomaly` 必须带 `anomaly_params`，algorithm 必须是已实装 detector，
///   lookback_days∈[1,30]、k 为正有限数；否则 400。
/// - 其它 kind 一律丢弃 `anomaly_params`（落 `None`），避免脏数据混进非 anomaly 规则。
fn normalize_anomaly(
    kind: AlertRuleKind,
    params: Option<AnomalyParams>,
) -> Result<Option<AnomalyParams>> {
    if kind != AlertRuleKind::Anomaly {
        return Ok(None);
    }
    let Some(params) = params else {
        return Err(Error::invalid(
            "anomaly_params is required when kind = anomaly",
        ));
    };
    if !SUPPORTED_DETECTORS.contains(&params.algorithm.as_str()) {
        return Err(Error::invalid(format!(
            "anomaly detector not yet supported: {}",
            params.algorithm
        )));
    }
    if params.lookback_days < 1 || params.lookback_days > MAX_ANOMALY_LOOKBACK_DAYS {
        return Err(Error::invalid(format!(
            "anomaly lookback_days must be between 1 and {MAX_ANOMALY_LOOKBACK_DAYS}"
        )));
    }
    if !params.k.is_finite() || params.k <= 0.0 {
        return Err(Error::invalid("anomaly k must be a positive number"));
    }
    // α 只对 EWMA 有意义；其它算法忽略其值，不强加约束。
    if params.algorithm == "ewma"
        && (!params.alpha.is_finite() || params.alpha <= 0.0 || params.alpha > 1.0)
    {
        return Err(Error::invalid("anomaly alpha must be in (0, 1] for ewma"));
    }
    // 周季节性每 7 天才取一个同星期几点；lookback < 7 一个样本都取不到 → 永远
    // inconclusive。入口直接拒绝，避免落库一条"永不触发"的死规则。
    if params.weekly_seasonality && params.lookback_days < 7 {
        return Err(Error::invalid(
            "weekly_seasonality requires lookback_days >= 7",
        ));
    }
    Ok(Some(params))
}

#[derive(Debug, Deserialize)]
struct EscalationWriteReq {
    name: String,
    #[serde(default)]
    steps: Vec<EscalationStep>,
    #[serde(default)]
    repeat: bool,
    #[serde(default = "default_max_loops")]
    max_loops: u32,
}

fn default_max_loops() -> u32 {
    1
}

// ---- rules ----
#[permission("alerts.read")]
async fn list_rules(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    Ok(Json(
        serde_json::to_value(state.alerting.service.list_rules(&ctx.org_id).await?).unwrap(),
    ))
}
#[permission("alerts.manage")]
async fn create_rule(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<RuleWriteReq>,
) -> Result<Json<Value>> {
    if req.name.trim().is_empty() {
        return Err(Error::invalid("name cannot be empty"));
    }
    if req.query.statement.trim().is_empty() {
        return Err(Error::invalid("query.statement cannot be empty"));
    }
    let Some(stream_hint) = req.query.stream.as_ref() else {
        return Err(Error::invalid(
            "query.stream is required; must specify { name, stream_type }",
        ));
    };
    if stream_hint.name.trim().is_empty() {
        return Err(Error::invalid("query.stream.name cannot be empty"));
    }
    let anomaly_params = normalize_anomaly(req.kind, req.anomaly_params)?;
    let now = TimestampMicros::now();
    let rule = AlertRule {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name: req.name,
        description: req.description,
        enabled: req.enabled,
        kind: req.kind,
        query: req.query,
        trigger: req.trigger,
        thresholds: req.thresholds,
        severity: req.severity,
        anomaly_params,
        escalation_policy_id: req
            .escalation_policy_id
            .unwrap_or_else(|| Id::from_string("default")),
        labels: req.labels,
        annotations: req.annotations,
        last_eval_at: None,
        last_state: RuleState::Healthy,
        created_at: now,
        updated_at: now,
    };
    let saved = state.alerting.service.create_rule(rule).await?;
    emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::AlertRule,
        CudAction::Created,
        &saved.id.0,
        &saved,
    )
    .await;
    Ok(Json(serde_json::to_value(saved).unwrap()))
}
#[resource_permission(
    action = "alerts.read",
    resource = AlertRule,
    id = Id::from_string(id),
    bind = rule
)]
async fn get_rule(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(serde_json::to_value(rule).unwrap()))
}
#[resource_permission(
    action = "alerts.manage",
    resource = AlertRule,
    id = Id::from_string(id),
    bind = existing
)]
async fn update_rule(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<RuleWriteReq>,
) -> Result<Json<Value>> {
    if req.name.trim().is_empty() {
        return Err(Error::invalid("name cannot be empty"));
    }
    if req.query.statement.trim().is_empty() {
        return Err(Error::invalid("query.statement cannot be empty"));
    }
    let Some(stream_hint) = req.query.stream.as_ref() else {
        return Err(Error::invalid(
            "query.stream is required; must specify { name, stream_type }",
        ));
    };
    if stream_hint.name.trim().is_empty() {
        return Err(Error::invalid("query.stream.name cannot be empty"));
    }
    let anomaly_params = normalize_anomaly(req.kind, req.anomaly_params)?;
    let rule = AlertRule {
        id: existing.id,
        org_id: existing.org_id,
        name: req.name,
        description: req.description,
        enabled: req.enabled,
        kind: req.kind,
        query: req.query,
        trigger: req.trigger,
        thresholds: req.thresholds,
        severity: req.severity,
        anomaly_params,
        escalation_policy_id: req
            .escalation_policy_id
            .unwrap_or(existing.escalation_policy_id),
        labels: req.labels,
        annotations: req.annotations,
        last_eval_at: existing.last_eval_at,
        last_state: existing.last_state,
        created_at: existing.created_at,
        updated_at: TimestampMicros::now(),
    };
    let saved = state.alerting.service.update_rule(rule).await?;
    emit_cud(
        &state,
        &saved.org_id,
        ResourceKind::AlertRule,
        CudAction::Updated,
        &saved.id.0,
        &saved,
    )
    .await;
    Ok(Json(serde_json::to_value(saved).unwrap()))
}
#[resource_permission(
    action = "alerts.manage",
    resource = AlertRule,
    id = Id::from_string(id),
    bind = rule
)]
async fn delete_rule(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<&'static str> {
    let rid = rule.id.clone();
    state.alerting.service.delete_rule(&rid).await?;
    emit_cud(
        &state,
        &rule.org_id,
        ResourceKind::AlertRule,
        CudAction::Deleted,
        &rid.0,
        &delete_payload(&rid.0),
    )
    .await;
    Ok("deleted")
}

// ---- incidents ----

/// list endpoint 只暴露最小跨信号 handle（top 1 trace_id / host / service），
/// 并且去掉 `triggering_query.sample_values` 以控制大列表的 payload 体积。
/// drawer 打开时通过 `GET /alerts/incidents/{id}` 获取完整 context。
const LIST_HANDLES_PER_INCIDENT: usize = 1;

fn shrink_for_list(mut i: Incident) -> Incident {
    i.trace_ids.truncate(LIST_HANDLES_PER_INCIDENT);
    i.host_ids.truncate(LIST_HANDLES_PER_INCIDENT);
    i.affected_services.truncate(LIST_HANDLES_PER_INCIDENT);
    // detail-only：list 不携带触发查询样本，避免 1000 个 incident × 20 行样本的 payload 膨胀。
    i.triggering_query = None;
    i
}

#[derive(Debug, Default, Deserialize)]
struct IncidentListQuery {
    /// `active`（默认）只返回 open / acknowledged；`all` 返回窗口内历史，
    /// 并始终合并仍活跃的长周期事件，避免超过窗口的未恢复事件消失。
    scope: Option<String>,
    /// `scope=all` 的回看窗口。默认 7 天，限制在 1 小时到 365 天。
    window_secs: Option<u64>,
}

#[permission("alerts.read")]
async fn list_incidents(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<IncidentListQuery>,
) -> Result<Json<Value>> {
    let mut incidents = if query.scope.as_deref() == Some("all") {
        let window_secs = query
            .window_secs
            .unwrap_or(7 * 24 * 60 * 60)
            .clamp(60 * 60, 365 * 24 * 60 * 60);
        let since = TimestampMicros(
            TimestampMicros::now().0 - i64::try_from(window_secs).unwrap_or(i64::MAX) * 1_000_000,
        );
        let mut recent = state
            .alerting
            .service
            .incidents
            .list_since(&ctx.org_id, since)
            .await?;
        let active = state
            .alerting
            .service
            .list_incidents_active(&ctx.org_id)
            .await?;
        let mut seen: HashSet<String> = recent.iter().map(|item| item.id.0.clone()).collect();
        recent.extend(
            active
                .into_iter()
                .filter(|item| seen.insert(item.id.0.clone())),
        );
        recent
    } else {
        state
            .alerting
            .service
            .list_incidents_active(&ctx.org_id)
            .await?
    };
    incidents.sort_by_key(|item| std::cmp::Reverse(item.created_at.0));
    let items: Vec<Incident> = incidents.into_iter().map(shrink_for_list).collect();
    Ok(Json(serde_json::to_value(items).unwrap()))
}
#[resource_permission(
    action = "alerts.read",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn get_incident(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(serde_json::to_value(incident).unwrap()))
}
/// AI 根因分析（RCA）：后台 sweeper 异步产出，尚未生成时返 404。
#[resource_permission(
    action = "alerts.read",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn get_incident_rca(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let rca = state
        .intelligence
        .incident_rca
        .get(&incident.id)
        .await?
        .ok_or_else(|| Error::not_found("no root-cause analysis for this incident yet"))?;
    Ok(Json(serde_json::to_value(rca).unwrap()))
}

#[derive(Debug, Default, Deserialize)]
struct IncidentRcaGenerateQuery {
    /// 当前产品语言。只支持 en-us / zh-cn；其它值由 RCA 层安全回退为 en-us。
    locale: Option<String>,
}

/// 按需手动触发 RCA 生成（同步调 LLM，返回生成结果）。需 AlertWrite + intelligence feature。
#[resource_permission(
    action = "alerts.manage",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn generate_incident_rca(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Query(query): Query<IncidentRcaGenerateQuery>,
) -> Result<Json<Value>> {
    if !state
        .platform
        .license
        .has_feature(crate::intelligence::FEATURE)
    {
        return Err(Error::forbidden(
            "AI intelligence is not enabled for this deployment",
        ));
    }
    let language = match query.locale {
        Some(locale) => locale,
        None => state.iam.user_preferences.get(&ctx.user_id).await?.language,
    };
    let locale = crate::api::rca::RcaOutputLocale::from_language_tag(&language);
    let rca = crate::api::rca::RcaGenerator::from_state(&state)
        .generate_for_locale(
            &incident.org_id,
            &incident,
            crate::shared::time::TimestampMicros::now(),
            locale,
        )
        .await?;
    Ok(Json(serde_json::to_value(rca).unwrap()))
}
#[resource_permission(
    action = "alerts.acknowledge",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn ack_incident(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let incident = state
        .alerting
        .service
        .acknowledge(&incident.id, ctx.user_id.clone(), TimestampMicros::now())
        .await?;
    Ok(Json(serde_json::to_value(incident).unwrap()))
}
#[resource_permission(
    action = "alerts.acknowledge",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn resolve_incident(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let incident = state
        .alerting
        .service
        .resolve(&incident.id, ctx.user_id.clone(), TimestampMicros::now())
        .await?;
    Ok(Json(serde_json::to_value(incident).unwrap()))
}

// ---- insights  ----

#[derive(Deserialize)]
struct InsightsQuery {
    /// Look-back window in seconds; defaults to 7 days, clamped to [1h, 365d].
    window_secs: Option<u64>,
}

#[derive(Serialize)]
struct CountItem {
    key: String,
    count: u32,
}

#[derive(Serialize)]
struct InsightsResponse {
    window_secs: u64,
    total: usize,
    /// open + acknowledged
    active: usize,
    /// resolved + closed
    closed: usize,
    /// mean (resolved_at - created_at) over resolved incidents, in seconds.
    mttr_secs: f64,
    /// fraction (0..1) of resolved incidents that closed within 60s.
    noise_rate: f64,
    /// 24 buckets, count of incidents by UTC hour-of-day of `created_at`.
    by_hour: Vec<u32>,
    by_severity: BTreeMap<String, u32>,
    top_services: Vec<CountItem>,
    top_rules: Vec<CountItem>,
}

/// Incidents resolved faster than this are treated as noise (flapping).
const NOISE_THRESHOLD_MICROS: i64 = 60_000_000;
const TOP_N: usize = 8;

fn severity_key(s: &Severity) -> &'static str {
    match s {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Error => "error",
        Severity::Critical => "critical",
    }
}

fn top_counts(counts: HashMap<String, u32>, n: usize) -> Vec<CountItem> {
    let mut v: Vec<CountItem> = counts
        .into_iter()
        .map(|(key, count)| CountItem { key, count })
        .collect();
    // Highest count first; ties broken by key for a stable order.
    v.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.key.cmp(&b.key)));
    v.truncate(n);
    v
}

#[permission("alerts.read")]
async fn get_insights(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(q): Query<InsightsQuery>,
) -> Result<Json<InsightsResponse>> {
    let window_secs = q
        .window_secs
        .unwrap_or(7 * 24 * 3600)
        .clamp(3600, 365 * 24 * 3600);
    let now = TimestampMicros::now();
    let since = TimestampMicros(now.0 - (window_secs as i64) * 1_000_000);
    let incidents = state
        .alerting
        .service
        .incidents
        .list_since(&ctx.org_id, since)
        .await?;

    let mut by_hour = vec![0u32; 24];
    let mut by_severity: BTreeMap<String, u32> = BTreeMap::new();
    let mut svc_counts: HashMap<String, u32> = HashMap::new();
    let mut rule_counts: HashMap<String, u32> = HashMap::new();
    let mut active = 0usize;
    let mut closed = 0usize;
    let mut resolved_durations: Vec<i64> = Vec::new();
    let mut noisy = 0usize;

    for i in &incidents {
        if let Some(dt) = Utc.timestamp_micros(i.created_at.0).single() {
            by_hour[dt.hour() as usize] += 1;
        }
        *by_severity
            .entry(severity_key(&i.severity).to_string())
            .or_insert(0) += 1;
        *rule_counts.entry(i.rule_id.0.clone()).or_insert(0) += 1;
        for svc in &i.affected_services {
            *svc_counts.entry(svc.clone()).or_insert(0) += 1;
        }
        match i.status {
            IncidentStatus::Open | IncidentStatus::Acknowledged => active += 1,
            IncidentStatus::Resolved | IncidentStatus::Closed => closed += 1,
        }
        if let Some(resolved_at) = i.resolved_at {
            let dur = resolved_at.0 - i.created_at.0;
            resolved_durations.push(dur);
            if dur < NOISE_THRESHOLD_MICROS {
                noisy += 1;
            }
        }
    }

    let resolved_n = resolved_durations.len();
    let mttr_secs = if resolved_n == 0 {
        0.0
    } else {
        let sum: i64 = resolved_durations.iter().sum();
        (sum as f64 / resolved_n as f64) / 1_000_000.0
    };
    let noise_rate = if resolved_n == 0 {
        0.0
    } else {
        noisy as f64 / resolved_n as f64
    };

    Ok(Json(InsightsResponse {
        window_secs,
        total: incidents.len(),
        active,
        closed,
        mttr_secs,
        noise_rate,
        by_hour,
        by_severity,
        top_services: top_counts(svc_counts, TOP_N),
        top_rules: top_counts(rule_counts, TOP_N),
    }))
}

// ---- escalations ----
#[permission("alerts.read")]
async fn list_escalations(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    Ok(Json(
        serde_json::to_value(state.alerting.service.list_policies(&ctx.org_id).await?).unwrap(),
    ))
}
#[permission("alerts.manage")]
async fn create_escalation(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<EscalationWriteReq>,
) -> Result<Json<Value>> {
    if req.name.trim().is_empty() {
        return Err(Error::invalid("name cannot be empty"));
    }
    let policy = EscalationPolicy {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name: req.name,
        steps: req.steps,
        repeat: req.repeat,
        max_loops: req.max_loops.max(1),
    };
    let saved = state.alerting.service.create_policy(policy).await?;
    emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::EscalationPolicy,
        CudAction::Created,
        &saved.id.0,
        &saved,
    )
    .await;
    Ok(Json(serde_json::to_value(saved).unwrap()))
}
#[resource_permission(
    action = "alerts.read",
    resource = EscalationPolicy,
    id = Id::from_string(id),
    bind = policy
)]
async fn get_escalation(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(serde_json::to_value(policy).unwrap()))
}
#[resource_permission(
    action = "alerts.manage",
    resource = EscalationPolicy,
    id = Id::from_string(id),
    bind = existing
)]
async fn update_escalation(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<EscalationWriteReq>,
) -> Result<Json<Value>> {
    if req.name.trim().is_empty() {
        return Err(Error::invalid("name cannot be empty"));
    }
    let policy = EscalationPolicy {
        id: existing.id,
        org_id: existing.org_id,
        name: req.name,
        steps: req.steps,
        repeat: req.repeat,
        max_loops: req.max_loops.max(1),
    };
    let saved = state.alerting.service.update_policy(policy).await?;
    emit_cud(
        &state,
        &saved.org_id,
        ResourceKind::EscalationPolicy,
        CudAction::Updated,
        &saved.id.0,
        &saved,
    )
    .await;
    Ok(Json(serde_json::to_value(saved).unwrap()))
}
#[resource_permission(
    action = "alerts.manage",
    resource = EscalationPolicy,
    id = Id::from_string(id),
    bind = policy
)]
async fn delete_escalation(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<&'static str> {
    let rid = policy.id.clone();
    state.alerting.service.delete_policy(&rid).await?;
    emit_cud(
        &state,
        &policy.org_id,
        ResourceKind::EscalationPolicy,
        CudAction::Deleted,
        &rid.0,
        &delete_payload(&rid.0),
    )
    .await;
    Ok("deleted")
}
