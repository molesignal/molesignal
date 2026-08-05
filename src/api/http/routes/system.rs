// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    api::AppState,
    app::{
        iam::IamContext,
        trace::{TracePipelineHealthSnapshot, candidate_router::TraceCandidateRouterHealth},
    },
    domain::{
        iam::{IamPlatformAdministrator, permission},
        trace_policy::TraceDebugToken,
    },
    infra::persistence::repositories::audit_events::AuditEvent,
    shared::{
        Error, Result,
        ids::Id,
        tail_sampling::{TailSamplerMetricSnapshot, TraceRuntimePolicy},
        time::TimestampMicros,
    },
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/system/platform-admins", get(list_platform_administrators))
        .route(
            "/system/telemetry",
            get(get_trace_telemetry).put(update_trace_telemetry),
        )
        .route("/system/telemetry/policies", get(list_trace_policies))
        .route(
            "/system/telemetry/debug-tokens",
            get(list_trace_debug_tokens).post(issue_trace_debug_token),
        )
        .route(
            "/system/telemetry/debug-tokens/{id}",
            delete(revoke_trace_debug_token),
        )
}

#[derive(Debug, Serialize)]
struct PlatformAdministratorView {
    user_id: String,
    active: bool,
    granted_by: Option<String>,
    granted_at_micros: i64,
    revoked_by: Option<String>,
    revoked_at_micros: Option<i64>,
}

impl From<IamPlatformAdministrator> for PlatformAdministratorView {
    fn from(value: IamPlatformAdministrator) -> Self {
        Self {
            user_id: value.user_id.0,
            active: value.active,
            granted_by: value.granted_by.map(|id| id.0),
            granted_at_micros: value.granted_at.0,
            revoked_by: value.revoked_by.map(|id| id.0),
            revoked_at_micros: value.revoked_at.map(|time| time.0),
        }
    }
}

#[permission("sys.administrators.manage")]
async fn list_platform_administrators(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<PlatformAdministratorView>>> {
    Ok(Json(
        state
            .iam
            .platform_administrators
            .list()
            .await?
            .into_iter()
            .filter(|assignment| assignment.active)
            .map(Into::into)
            .collect(),
    ))
}

#[derive(Debug, Serialize)]
struct TraceTelemetryView {
    deployment_force_disabled: bool,
    runtime_enabled: bool,
    effective_enabled: bool,
    precedence: &'static [&'static str],
    policy: TraceRuntimePolicy,
    metrics: TailSamplerMetricSnapshot,
    candidate_routing: TraceCandidateRouterHealth,
    pipeline: TracePipelineHealthSnapshot,
    health: TraceHealthView,
}

#[derive(Debug, Serialize)]
struct TraceHealthView {
    status: &'static str,
    detail: String,
    default_alerts: Vec<TraceDefaultAlertView>,
}

#[derive(Debug, Serialize)]
struct TraceDefaultAlertView {
    name: &'static str,
    active: bool,
    threshold: &'static str,
    summary: &'static str,
}

#[permission("sys.telemetry.read")]
async fn get_trace_telemetry(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<TraceTelemetryView>> {
    let policy = (*state.telemetry.tail_sampler.active_policy()).clone();
    let effective_enabled = state.telemetry.tail_sampler.effective_enabled();
    let metrics = state.telemetry.tail_sampler.metrics();
    let candidate_routing = state.telemetry.trace_candidates.health();
    let pipeline = state.telemetry.trace_pipeline.health();
    let alerts = trace_default_alerts(
        &metrics,
        &candidate_routing,
        &pipeline,
        state.telemetry.system_load_health,
    );
    let degraded = alerts.iter().any(|alert| alert.active);
    let detail = if degraded {
        alerts
            .iter()
            .filter(|alert| alert.active)
            .map(|alert| alert.name)
            .collect::<Vec<_>>()
            .join(", ")
    } else if effective_enabled {
        "tail sampler and configured sinks are healthy".into()
    } else {
        "Trace generation/storage is disabled by effective policy".into()
    };
    Ok(Json(TraceTelemetryView {
        deployment_force_disabled: state.telemetry.tail_sampler.deployment_force_disabled(),
        runtime_enabled: policy.enabled,
        effective_enabled,
        precedence: &[
            "deployment_force_disabled",
            "persisted_runtime_policy",
            "code_default",
        ],
        policy,
        metrics,
        candidate_routing,
        pipeline,
        health: TraceHealthView {
            status: if degraded {
                "degraded"
            } else if effective_enabled {
                "ok"
            } else {
                "disabled"
            },
            detail,
            default_alerts: alerts,
        },
    }))
}

fn trace_default_alerts(
    metrics: &TailSamplerMetricSnapshot,
    routing: &TraceCandidateRouterHealth,
    pipeline: &TracePipelineHealthSnapshot,
    system: crate::api::state::TraceSystemLoadHealth,
) -> Vec<TraceDefaultAlertView> {
    let exporter_failure = pipeline
        .self_ingest
        .iter()
        .chain(pipeline.external.iter())
        .any(|sink| sink.degraded && sink.failed_batches > 0);
    let queue_high = occupancy(routing.queue_depth, routing.queue_capacity) > 0.8
        || occupancy(
            pipeline.candidate_queue_depth,
            pipeline.candidate_queue_capacity,
        ) > 0.8
        || pipeline
            .self_ingest
            .iter()
            .chain(pipeline.external.iter())
            .any(|sink| occupancy(sink.queue_depth, sink.queue_capacity) > 0.8)
        || occupancy(metrics.pending_traces, metrics.capacity_traces) > 0.8
        || occupancy(metrics.pending_bytes, metrics.capacity_bytes) > 0.8;
    let delivered = routing
        .delivered_local
        .saturating_add(routing.delivered_remote)
        .saturating_add(metrics.accepted);
    let drops = routing
        .queue_full
        .saturating_add(routing.no_owner)
        .saturating_add(routing.transport_failed)
        .saturating_add(routing.owner_overloaded)
        .saturating_add(routing.expired)
        .saturating_add(pipeline.candidate_drops)
        .saturating_add(
            pipeline
                .self_ingest
                .iter()
                .chain(pipeline.external.iter())
                .map(|sink| sink.dropped_spans)
                .sum::<u64>(),
        );
    let drop_rate_high = drops > 0 && drops as f64 / delivered.saturating_add(drops) as f64 > 0.01;
    let system_load_failure = !(system.system_org && system.license && system.trace_policy);

    vec![
        TraceDefaultAlertView {
            name: "trace_exporter_failure",
            active: exporter_failure,
            threshold: "sustained failed batches",
            summary: "A configured Trace sink is degraded after bounded retries.",
        },
        TraceDefaultAlertView {
            name: "trace_capacity_high",
            active: queue_high,
            threshold: ">80% queue or tail-cache occupancy",
            summary: "A Trace queue or the tail cache is near its soft capacity.",
        },
        TraceDefaultAlertView {
            name: "trace_drop_rate_high",
            active: drop_rate_high,
            threshold: ">1% observed drops",
            summary: "Trace candidates or retained spans are being dropped above the default gate.",
        },
        TraceDefaultAlertView {
            name: "trace_system_load_failure",
            active: system_load_failure,
            threshold: "any required component unhealthy",
            summary: "_sys, License, or dynamic Trace policy failed to load cleanly.",
        },
    ]
}

fn occupancy(value: usize, capacity: usize) -> f64 {
    if capacity == 0 {
        0.0
    } else {
        value as f64 / capacity as f64
    }
}

#[permission("sys.telemetry.manage")]
async fn update_trace_telemetry(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(mut policy): Json<TraceRuntimePolicy>,
) -> Result<Json<TraceTelemetryView>> {
    // 版本只由持久层事务分配，拒绝客户端借版本号影响正在处理的 Trace。
    policy.version = 0;
    policy.validate().map_err(Error::invalid)?;
    let persisted = state
        .telemetry
        .trace_policies
        .publish(&state.iam.system_org_id, policy, &context.user_id)
        .await?;
    // `publish` 已验证；内存发布不会再失败，因此 DB active 与运行快照原子同向推进。
    state
        .telemetry
        .tail_sampler
        .publish_policy(persisted.policy.clone())
        .map_err(Error::invalid)?;
    record_system_audit(
        &state,
        &context,
        "trace_policy.update",
        "trace_runtime_policy",
        Some(&persisted.id),
        json!({
            "version": persisted.policy.version,
            "enabled": persisted.policy.enabled,
            "normal_sample_ratio": persisted.policy.normal_sample_ratio,
            "rule_count": persisted.policy.rules.len(),
        }),
    )
    .await;
    get_trace_telemetry(State(state), Extension(context)).await
}

#[derive(Debug, Serialize)]
struct TracePolicyHistoryView {
    id: String,
    policy: TraceRuntimePolicy,
    created_by: Option<String>,
    created_at_micros: i64,
    active: bool,
}

#[permission("sys.telemetry.read")]
async fn list_trace_policies(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<TracePolicyHistoryView>>> {
    let active_version = state.telemetry.tail_sampler.active_policy().version;
    Ok(Json(
        state
            .telemetry
            .trace_policies
            .history()
            .await?
            .into_iter()
            .map(|persisted| TracePolicyHistoryView {
                id: persisted.id.0,
                active: persisted.policy.version == active_version,
                policy: persisted.policy,
                created_by: persisted.created_by.map(|id| id.0),
                created_at_micros: persisted.created_at.0,
            })
            .collect(),
    ))
}

#[derive(Debug, Serialize)]
struct TraceDebugTokenView {
    id: String,
    organization_id: Option<String>,
    route_pattern: Option<String>,
    expires_at_micros: i64,
    max_uses: u64,
    used_count: u64,
    revoked_at_micros: Option<i64>,
    created_by: String,
    created_at_micros: i64,
}

impl From<TraceDebugToken> for TraceDebugTokenView {
    fn from(token: TraceDebugToken) -> Self {
        Self {
            id: token.id.0,
            organization_id: token.organization_id.map(|id| id.0),
            route_pattern: token.route_pattern,
            expires_at_micros: token.expires_at.0,
            max_uses: token.max_uses,
            used_count: token.used_count,
            revoked_at_micros: token.revoked_at.map(|time| time.0),
            created_by: token.created_by.0,
            created_at_micros: token.created_at.0,
        }
    }
}

#[permission("sys.trace_debug.manage")]
async fn list_trace_debug_tokens(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<TraceDebugTokenView>>> {
    Ok(Json(
        state
            .telemetry
            .trace_debug_tokens
            .list()
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

#[derive(Debug, Deserialize)]
struct IssueTraceDebugTokenRequest {
    organization_id: Option<String>,
    route_pattern: Option<String>,
    ttl_seconds: u64,
    #[serde(default = "default_debug_token_max_uses")]
    max_uses: u64,
}

fn default_debug_token_max_uses() -> u64 {
    100
}

#[derive(Debug, Serialize)]
struct IssuedTraceDebugTokenView {
    #[serde(flatten)]
    metadata: TraceDebugTokenView,
    /// 只在签发响应出现一次；持久层和审计仅保存摘要。
    token: String,
}

#[permission("sys.trace_debug.manage")]
async fn issue_trace_debug_token(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(request): Json<IssueTraceDebugTokenRequest>,
) -> Result<(StatusCode, Json<IssuedTraceDebugTokenView>)> {
    if !(1..=3_600).contains(&request.ttl_seconds) {
        return Err(Error::invalid(
            "Trace debug token ttl_seconds must be between 1 and 3600",
        ));
    }
    if !(1..=10_000).contains(&request.max_uses) {
        return Err(Error::invalid(
            "Trace debug token max_uses must be between 1 and 10000",
        ));
    }
    if request
        .route_pattern
        .as_ref()
        .is_some_and(|route| route.is_empty() || route.len() > 255 || route.contains('%'))
    {
        return Err(Error::invalid(
            "route_pattern must be a non-empty exact/underscore LIKE pattern without `%`",
        ));
    }
    let plaintext = format!(
        "mstd_{}{}",
        uuid::Uuid::now_v7().simple(),
        uuid::Uuid::now_v7().simple()
    );
    let now = TimestampMicros::now();
    let token = TraceDebugToken {
        id: Id::new(),
        token_hash: blake3::hash(plaintext.as_bytes()).to_hex().to_string(),
        organization_id: request.organization_id.map(Id),
        route_pattern: request.route_pattern,
        expires_at: TimestampMicros(
            now.0
                .saturating_add((request.ttl_seconds as i64).saturating_mul(1_000_000)),
        ),
        max_uses: request.max_uses,
        used_count: 0,
        revoked_at: None,
        created_by: context.user_id.clone(),
        created_at: now,
    };
    let created = state.telemetry.trace_debug_tokens.create(token).await?;
    record_system_audit(
        &state,
        &context,
        "trace_debug_token.issue",
        "trace_debug_token",
        Some(&created.id),
        json!({
            "organization_id": created.organization_id.as_ref().map(|id| &id.0),
            "route_pattern": created.route_pattern,
            "expires_at_micros": created.expires_at.0,
            "max_uses": created.max_uses,
        }),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(IssuedTraceDebugTokenView {
            metadata: created.into(),
            token: plaintext,
        }),
    ))
}

#[permission("sys.trace_debug.manage")]
async fn revoke_trace_debug_token(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let id = Id(id);
    state
        .telemetry
        .trace_debug_tokens
        .revoke(&id, TimestampMicros::now())
        .await?;
    record_system_audit(
        &state,
        &context,
        "trace_debug_token.revoke",
        "trace_debug_token",
        Some(&id),
        json!({}),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

async fn record_system_audit(
    state: &AppState,
    context: &IamContext,
    action: &str,
    target_kind: &str,
    target_id: Option<&Id>,
    payload: serde_json::Value,
) {
    let _ = state
        .iam
        .audit_events
        .record(AuditEvent {
            id: Id::new(),
            org_id: state.iam.system_org_id.clone(),
            actor_kind: "user".into(),
            actor_id: context.user_id.0.clone(),
            action: action.into(),
            target_kind: Some(target_kind.into()),
            target_id: target_id.map(|id| id.0.clone()),
            ip: None,
            user_agent: None,
            payload,
            ts: TimestampMicros::now(),
        })
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        api::{http::middleware::Permission, state::TraceSystemLoadHealth},
        app::trace::{
            TracePipelineHealthSnapshot, TraceSinkHealthSnapshot,
            candidate_router::TraceCandidateRouterHealth,
        },
        domain::iam::IamScope,
        shared::tail_sampling::TailSamplerMetricSnapshot,
    };

    #[test]
    fn platform_permission_uses_database_resolved_snapshot() {
        let tenant = IamContext {
            user_id: Id("u".into()),
            org_id: Id("o".into()),
            display_role: "Tenant Administrator".into(),
            roles: Vec::new(),
            credential_role_id: None,
            credential_application_id: None,
            scope: IamScope::Organization,
            permissions: ["org.settings.manage".to_string()].into_iter().collect(),
            features: std::collections::BTreeSet::new(),
            policy_version: 1,
        };
        assert!(Permission::require_key(&tenant, "sys.licenses.read").is_err());

        let system = IamContext {
            scope: IamScope::System,
            permissions: ["sys.licenses.read".to_string()].into_iter().collect(),
            ..tenant
        };
        Permission::require_key(&system, "sys.licenses.read").unwrap();
    }

    #[test]
    fn default_alerts_enforce_export_capacity_drop_and_system_load_gates() {
        let metrics = TailSamplerMetricSnapshot {
            accepted: 99,
            kept: 0,
            dropped: 0,
            duplicates: 0,
            conflicts: 0,
            late_kept: 0,
            late_dropped: 0,
            pressure_decisions: 0,
            unresolved_loss: 0,
            pending_traces: 81,
            pending_bytes: 0,
            capacity_traces: 100,
            capacity_bytes: 100,
        };
        let routing = TraceCandidateRouterHealth {
            accepting: true,
            queue_depth: 0,
            queue_capacity: 100,
            delivered_local: 0,
            delivered_remote: 0,
            queue_full: 2,
            no_owner: 0,
            transport_failed: 0,
            owner_overloaded: 0,
            expired: 0,
            invalid: 0,
        };
        let failed_sink = TraceSinkHealthSnapshot {
            queue_depth: 0,
            queue_capacity: 100,
            queued_spans: 0,
            in_flight_spans: 0,
            exported_spans: 0,
            failed_batches: 1,
            dropped_spans: 0,
            retries: 3,
            degraded: true,
            last_error: Some("collector unavailable".into()),
        };
        let pipeline = TracePipelineHealthSnapshot {
            accepting: true,
            candidate_queue_depth: 0,
            candidate_queue_capacity: 100,
            candidate_drops: 0,
            self_ingest: None,
            external: Some(failed_sink),
        };
        let alerts = trace_default_alerts(
            &metrics,
            &routing,
            &pipeline,
            TraceSystemLoadHealth {
                system_org: true,
                license: false,
                trace_policy: true,
            },
        );
        assert_eq!(alerts.len(), 4);
        assert!(alerts.iter().all(|alert| alert.active));
    }

    #[test]
    fn default_alert_thresholds_are_strictly_above_eighty_and_one_percent() {
        let metrics = TailSamplerMetricSnapshot {
            accepted: 99,
            kept: 0,
            dropped: 0,
            duplicates: 0,
            conflicts: 0,
            late_kept: 0,
            late_dropped: 0,
            pressure_decisions: 0,
            unresolved_loss: 0,
            pending_traces: 80,
            pending_bytes: 80,
            capacity_traces: 100,
            capacity_bytes: 100,
        };
        let routing = TraceCandidateRouterHealth {
            accepting: true,
            queue_depth: 80,
            queue_capacity: 100,
            delivered_local: 0,
            delivered_remote: 0,
            queue_full: 1,
            no_owner: 0,
            transport_failed: 0,
            owner_overloaded: 0,
            expired: 0,
            invalid: 0,
        };
        let pipeline = TracePipelineHealthSnapshot {
            accepting: true,
            candidate_queue_depth: 80,
            candidate_queue_capacity: 100,
            candidate_drops: 0,
            self_ingest: None,
            external: None,
        };
        let alerts = trace_default_alerts(
            &metrics,
            &routing,
            &pipeline,
            TraceSystemLoadHealth {
                system_org: true,
                license: true,
                trace_policy: true,
            },
        );
        assert!(alerts.iter().all(|alert| !alert.active));
    }
}
