// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 角色启动入口：按**生效角色集**起所需的前台 server（去重）。
//!
//! 后台 loop（intake flush / compactor / alert）已在 [`crate::bootstrap::build_state`]
//! 里按角色 gate spawn；本模块负责前台 server，以及本节点在集群成员表里的登记（开始服务时
//! 登记，关停时先注销，见 [`heartbeat`]）：
//!
//! | 角色 | 前台 server |
//! |---|---|
//! | `standalone` | 完整 HTTP API 与 Web UI、内部 gRPC、OTLP gRPC、Probe gRPC |
//! | `router` | 完整 HTTP API 与 Web UI（写入转发给 intake 节点，查询扇出给 querier 节点）、OTLP gRPC |
//! | `intake` | 运维 HTTP、内部 gRPC、OTLP gRPC |
//! | `querier` | 运维 HTTP、内部 gRPC（Arrow Flight 扫描分片） |
//! | `alert` | 运维 HTTP、Probe gRPC |
//! | `compactor` | 运维 HTTP |
//!
//! `Standalone` 等价于全角色。多角色节点（如 `["intake","querier"]`）起去重后的 server 集合。
//! 运维 HTTP（[`ops_server`]）只提供探活、就绪与指标；完整 HTTP 由 `standalone` 与 `router`
//! 提供，两者同时存在时只起完整 HTTP。

use std::{sync::Arc, time::Duration};

use crate::{
    api::AppState,
    config::{Role, Settings},
    shared::drain::{DrainController, DrainPhase},
};

pub mod alert;
pub mod compactor;
pub mod health_probe;
pub mod heartbeat;
pub mod http_server;
pub mod intake;
pub mod ops_server;
pub mod profiling;
pub mod querier;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpKind {
    /// 完整 HTTP API 与 Web UI。
    Full,
    /// 仅探活、就绪与指标。
    Ops,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerPlan {
    pub http: HttpKind,
    /// 内部 gRPC（intake push、Flight 扫描分片、集群 RPC）。
    pub grpc: bool,
    /// 对外 OTLP gRPC。
    pub otlp_grpc: bool,
    /// 对外 Flight SQL（仍受 `[flight_sql].enabled` 控制）。
    pub flight_sql: bool,
    /// Probe 注册与控制 gRPC。
    pub probe_grpc: bool,
}

/// 由生效角色集推导要起哪些前台 server。`Standalone` == 全角色。
pub fn plan_servers(roles: &[Role]) -> ServerPlan {
    let has = |role: Role| roles.contains(&role) || roles.contains(&Role::Standalone);

    ServerPlan {
        http: if has(Role::Router) {
            HttpKind::Full
        } else {
            HttpKind::Ops
        },
        grpc: has(Role::Intake) || has(Role::Querier),
        otlp_grpc: has(Role::Intake) || has(Role::Router),
        flight_sql: has(Role::Querier) || has(Role::Router),
        probe_grpc: has(Role::Alert),
    }
}

/// 起前台 server 集合，任一退出/出错即 fail-loud 结束进程。
pub async fn run(state: AppState, settings: &Settings) -> anyhow::Result<()> {
    let plan = plan_servers(&settings.node.roles);
    let alive_window = settings.cluster.peer_timeout_secs as i64;
    let _sweeper = heartbeat::spawn_sweeper(state.cluster.repository.clone());

    // 开始服务时登记到集群成员表，退出前注销。
    let heartbeat = heartbeat::HeartbeatTask::new(
        state.cluster.repository.clone(),
        state.cluster.node_id.clone(),
        heartbeat::peer_roles(&settings.node.roles),
        settings.cluster.advertise_addr.clone(),
        settings.cluster.heartbeat_interval_secs,
    )
    .with_drain(state.cluster.drain.clone())
    .spawn();

    let mut set = tokio::task::JoinSet::new();

    {
        let (state, settings) = (state.clone(), settings.clone());

        match plan.http {
            HttpKind::Full => set.spawn(async move { http_server::serve(state, settings).await }),
            HttpKind::Ops => set.spawn(async move { ops_server::serve(state, settings).await }),
        };
    }

    if plan.grpc {
        let s = state.clone();
        let grpc_cfg = settings.grpc.clone();
        set.spawn(async move { crate::api::grpc::serve_grpc(s, &grpc_cfg, alive_window).await });
    }

    // 对外 OTLP gRPC（traces/logs/metrics/profiles）：与 HTTP 写入一样走 `IntakeService`，
    // 没有 intake 角色的节点（router）把批次转发给 intake 节点。
    if plan.otlp_grpc {
        let s = state.clone();
        let cfg = settings.otlp_grpc.clone();
        set.spawn(async move { crate::api::grpc::serve_otlp_grpc(s, &cfg).await });
    }

    // 对外 Flight SQL（spec flight-sql）：按配置起独立 listener。
    if plan.flight_sql && settings.flight_sql.enabled {
        let s = state.clone();
        let cfg = settings.flight_sql.clone();
        set.spawn(async move { crate::api::grpc::serve_flight_sql(s, &cfg).await });
    }

    // Probe 控制面与告警/自动发布职责同节点部署。
    if plan.probe_grpc {
        let s = state.clone();
        let cfg = settings.probe.clone();
        set.spawn(async move { crate::api::grpc::serve_probe_grpc(s, &cfg).await });
    }

    if settings.profiling.enabled {
        let state = state.clone();
        let profiling = settings.profiling.clone();
        set.spawn(async move { profiling::serve(state, profiling).await });
    }

    // 优雅退役：SIGTERM/SIGINT → 注销集群成员 → begin_drain → 等本节点缓冲的数据 flush 干净
    // （drained）后退出。作为 JoinSet 成员：信号触发即完成 → join_next 返回 → run() 收尾 → 进程退出。
    {
        let heartbeat = heartbeat.clone();
        let drain = state.cluster.drain.clone();
        let timeout = Duration::from_secs(settings.node.drain_timeout_secs.max(1) as u64);
        let self_telemetry = state.telemetry.self_telemetry_runtime.clone();
        let trace_candidates = state.telemetry.trace_candidates.clone();
        let trace_pipeline = state.telemetry.trace_pipeline.clone();
        let apm_runtime = state.telemetry.apm_runtime.clone();

        set.spawn(run_shutdown(
            heartbeat,
            drain,
            self_telemetry,
            trace_candidates,
            trace_pipeline,
            apm_runtime,
            timeout,
        ));
    }

    // 任一 server future 先返回（正常退出 / 错误 / 收到关停信号）→ 整体结束（fail-loud）。
    let outcome = match set.join_next().await {
        Some(Ok(served)) => served,
        Some(Err(join_error)) => Err(join_error.into()),
        None => Ok(()),
    };

    // 关停流程一开始就已注销；server 出错退出时在这里注销。
    heartbeat.leave().await;
    outcome
}

/// 关停看守：等首个 SIGTERM/SIGINT → 注销集群成员 → 触发 drain → 等节点 drained（仅 intake
/// 节点有待 flush 数据）后返回；超时或二次信号强制返回。返回即让 [`run`] 收尾退出进程。
#[cfg(unix)]
async fn run_shutdown(
    heartbeat: Arc<heartbeat::Heartbeat>,
    drain: Arc<DrainController>,
    self_telemetry: Option<Arc<crate::app::self_telemetry::SelfTelemetryRuntime>>,
    trace_candidates: Arc<crate::app::trace::candidate_router::TraceCandidateRouter>,
    trace_pipeline: Arc<crate::app::trace::TracePipeline>,
    apm_runtime: Option<Arc<crate::app::apm::ApmRuntime>>,
    timeout: Duration,
) -> anyhow::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate())
        .map_err(|e| anyhow::anyhow!("install SIGTERM handler: {e}"))?;
    let mut int = signal(SignalKind::interrupt())
        .map_err(|e| anyhow::anyhow!("install SIGINT handler: {e}"))?;

    tokio::select! {
        _ = term.recv() => tracing::info!("SIGTERM received; draining before exit"),
        _ = int.recv() => tracing::info!("SIGINT received; draining before exit"),
    }

    // 先离开集群：其余节点约 2 秒后不再向本节点转发写入与查询，并接手它名下的后台工作；
    // 本节点随后把手上的数据处理完再退出。
    heartbeat.leave().await;

    if let Some(runtime) = self_telemetry {
        runtime.stop_and_flush().await;
    }

    trace_candidates.shutdown().await;
    trace_pipeline.shutdown().await;

    if let Some(runtime) = apm_runtime {
        runtime.shutdown().await;
    }

    drain.begin_drain();
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);

    loop {
        if drain.phase() == DrainPhase::Drained {
            tracing::info!("node drained: pending data flushed; exiting");
            return Ok(());
        }

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
            _ = &mut deadline => {
                tracing::warn!("drain timeout reached; forcing exit (un-flushed WAL replays on restart)");
                return Ok(());
            }
            _ = term.recv() => { tracing::warn!("second signal; forcing immediate exit"); return Ok(()); }
            _ = int.recv() => { tracing::warn!("second signal; forcing immediate exit"); return Ok(()); }
        }
    }
}

/// 非 unix（Windows）：仅 ctrl_c，无 SIGTERM。
#[cfg(not(unix))]
async fn run_shutdown(
    heartbeat: Arc<heartbeat::Heartbeat>,
    drain: Arc<DrainController>,
    self_telemetry: Option<Arc<crate::app::self_telemetry::SelfTelemetryRuntime>>,
    trace_candidates: Arc<crate::app::trace::candidate_router::TraceCandidateRouter>,
    trace_pipeline: Arc<crate::app::trace::TracePipeline>,
    apm_runtime: Option<Arc<crate::app::apm::ApmRuntime>>,
    timeout: Duration,
) -> anyhow::Result<()> {
    tokio::signal::ctrl_c().await.ok();
    tracing::info!("ctrl-c received; draining before exit");
    heartbeat.leave().await;

    if let Some(runtime) = self_telemetry {
        runtime.stop_and_flush().await;
    }

    trace_candidates.shutdown().await;
    trace_pipeline.shutdown().await;

    if let Some(runtime) = apm_runtime {
        runtime.shutdown().await;
    }

    drain.begin_drain();
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);

    loop {
        if drain.phase() == DrainPhase::Drained {
            return Ok(());
        }

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
            _ = &mut deadline => return Ok(()),
            _ = tokio::signal::ctrl_c() => return Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HttpKind, ServerPlan, plan_servers};
    use crate::config::Role;

    fn plan(
        http: HttpKind,
        grpc: bool,
        otlp_grpc: bool,
        flight_sql: bool,
        probe_grpc: bool,
    ) -> ServerPlan {
        ServerPlan {
            http,
            grpc,
            otlp_grpc,
            flight_sql,
            probe_grpc,
        }
    }

    #[test]
    fn standalone_runs_every_listener() {
        assert_eq!(
            plan_servers(&[Role::Standalone]),
            plan(HttpKind::Full, true, true, true, true)
        );
    }

    #[test]
    fn router_serves_the_full_api_and_otlp_without_internal_grpc() {
        assert_eq!(
            plan_servers(&[Role::Router]),
            plan(HttpKind::Full, false, true, true, false)
        );
    }

    #[test]
    fn intake_has_ops_http_internal_grpc_and_otlp() {
        assert_eq!(
            plan_servers(&[Role::Intake]),
            plan(HttpKind::Ops, true, true, false, false)
        );
    }

    #[test]
    fn querier_has_ops_http_internal_grpc_and_flight_sql() {
        assert_eq!(
            plan_servers(&[Role::Querier]),
            plan(HttpKind::Ops, true, false, true, false)
        );
    }

    #[test]
    fn alert_has_ops_http_and_probe_grpc() {
        assert_eq!(
            plan_servers(&[Role::Alert]),
            plan(HttpKind::Ops, false, false, false, true)
        );
    }

    #[test]
    fn compactor_has_only_the_ops_http_listener() {
        assert_eq!(
            plan_servers(&[Role::Compactor]),
            plan(HttpKind::Ops, false, false, false, false)
        );
    }

    #[test]
    fn combined_roles_share_listeners() {
        assert_eq!(
            plan_servers(&[Role::Intake, Role::Querier]),
            plan(HttpKind::Ops, true, true, true, false)
        );
        assert_eq!(
            plan_servers(&[Role::Router, Role::Querier]),
            plan(HttpKind::Full, true, true, true, false)
        );
    }
}
