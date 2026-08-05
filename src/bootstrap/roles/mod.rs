// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 角色启动入口：按**生效角色集**起所需的前台 server（去重）。
//!
//! 后台 loop（ingester flush / compactor / alert_manager）已在 [`crate::bootstrap::build_state`]
//! 里按角色 gate spawn；本模块只负责前台 server：
//! - `Standalone` → [`http_server::serve`]（HTTP + gRPC 同进程）
//! - `Router` → [`router::run`]（反代 HTTP）
//! - `Ingester` / `Querier` → gRPC（ingest + Arrow Flight scan + cluster RPC）
//!
//! `Standalone` 等价于全角色。多角色节点（如 `["ingester","querier"]`）会起一份去重后的
//! server 集合；只承担后台角色（compactor / alert_manager）的节点无前台 server，靠 keepalive
//! 保活、让 build_state 起的后台 loop 持续运行。

use std::{sync::Arc, time::Duration};

use crate::{
    api::AppState,
    config::{Role, Settings},
    shared::drain::{DrainController, DrainPhase},
};

pub mod alert_manager;
pub mod compactor;
pub mod health_probe;
pub mod heartbeat;
pub mod http_server;
pub mod ingester;
pub mod profiling;
pub mod querier;
pub mod router;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpKind {
    /// 不起 HTTP server。
    None,
    /// router 反代 HTTP。
    Proxy,
    /// 完整 app（含 gRPC）。
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerPlan {
    pub http: HttpKind,
    /// 是否单独起 gRPC server（`Full` 已自带 gRPC，故此处为 false）。
    pub grpc: bool,
}

/// 由生效角色集推导要起哪些前台 server。`Standalone` == 全角色。
pub fn plan_servers(roles: &[Role]) -> ServerPlan {
    let has = |r: Role| roles.contains(&r) || roles.contains(&Role::Standalone);
    if has(Role::Standalone) {
        // http_server::serve 已同时起 HTTP + gRPC，避免重复绑 gRPC 端口。
        return ServerPlan {
            http: HttpKind::Full,
            grpc: false,
        };
    }
    let http = if has(Role::Router) {
        HttpKind::Proxy
    } else {
        HttpKind::None
    };
    // ingester / querier 对外暴露 gRPC（ingest push、Flight scan shard、cluster）。
    let grpc = has(Role::Ingester) || has(Role::Querier);
    ServerPlan { http, grpc }
}

/// 起前台 server 集合，任一退出/出错即 fail-loud 结束进程。
pub async fn run(state: AppState, settings: &Settings) -> anyhow::Result<()> {
    let plan = plan_servers(&settings.node.roles);
    let alive_window = settings.cluster.peer_timeout_secs as i64;
    let mut set = tokio::task::JoinSet::new();

    match plan.http {
        HttpKind::Full => {
            let (s, c) = (state.clone(), settings.clone());
            set.spawn(async move { http_server::serve(s, c).await });
        }
        HttpKind::Proxy => {
            let (s, c) = (state.clone(), settings.clone());
            set.spawn(async move { router::run(s, c).await });
        }
        HttpKind::None => {}
    }
    if plan.grpc {
        let s = state.clone();
        let grpc_cfg = settings.grpc.clone();
        set.spawn(async move { crate::api::grpc::serve_grpc(s, &grpc_cfg, alive_window).await });
    }
    // 对外 Flight SQL（spec flight-sql）：querier 角色节点按配置起独立 listener。
    // `Full`（standalone）路径由 http_server::serve 内部接管，这里跳过避免双绑端口。
    if settings.flight_sql.enabled
        && plan.http != HttpKind::Full
        && settings.node.roles.contains(&Role::Querier)
    {
        let s = state.clone();
        let cfg = settings.flight_sql.clone();
        set.spawn(async move { crate::api::grpc::serve_flight_sql(s, &cfg).await });
    }
    // 对外 OTLP gRPC（traces/logs/metrics/profiles）：ingester 角色节点按配置起独立 listener。
    // `Full`（standalone）路径由 http_server::serve 内部接管，这里跳过避免双绑端口。
    if plan.http != HttpKind::Full && settings.node.roles.contains(&Role::Ingester) {
        let s = state.clone();
        let cfg = settings.otlp_grpc.clone();
        set.spawn(async move { crate::api::grpc::serve_otlp_grpc(s, &cfg).await });
    }
    if settings.profiling.enabled {
        let state = state.clone();
        let profiling = settings.profiling.clone();
        set.spawn(async move { profiling::serve(state, profiling).await });
    }
    // 优雅退役：SIGTERM/SIGINT → begin_drain → 等 ingester flush 干净（drained）后退出。
    // 作为 JoinSet 成员：信号触发即完成 → join_next 返回 → run() 收尾 → 进程退出。无前台
    // server 的纯后台角色也靠它保活（替代 keepalive），收到信号才退出。
    {
        let drain = state.cluster.drain.clone();
        let has = |r: Role| {
            settings.node.roles.contains(&r) || settings.node.roles.contains(&Role::Standalone)
        };
        let has_ingester = has(Role::Ingester);
        let timeout = Duration::from_secs(settings.node.drain_timeout_secs.max(1) as u64);
        let self_telemetry = state.telemetry.self_telemetry_runtime.clone();
        let trace_candidates = state.telemetry.trace_candidates.clone();
        let trace_pipeline = state.telemetry.trace_pipeline.clone();
        let apm_runtime = state.telemetry.apm_runtime.clone();
        set.spawn(run_shutdown(
            drain,
            self_telemetry,
            trace_candidates,
            trace_pipeline,
            apm_runtime,
            has_ingester,
            timeout,
        ));
    }

    // 任一 server future 先返回（正常退出 / 错误 / 收到关停信号）→ 整体结束（fail-loud）。
    match set.join_next().await {
        Some(joined) => joined?,
        None => Ok(()),
    }
}

/// 关停看守：等首个 SIGTERM/SIGINT → 触发 drain → 等节点 drained（仅 ingester 节点有待 flush
/// 数据）后返回；超时或二次信号强制返回。返回即让 [`run`] 收尾退出进程。
#[cfg(unix)]
async fn run_shutdown(
    drain: Arc<DrainController>,
    self_telemetry: Option<Arc<crate::app::self_telemetry::SelfTelemetryRuntime>>,
    trace_candidates: Arc<crate::app::trace::candidate_router::TraceCandidateRouter>,
    trace_pipeline: Arc<crate::app::trace::TracePipeline>,
    apm_runtime: Option<Arc<crate::app::apm::ApmRuntime>>,
    has_ingester: bool,
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
    if let Some(runtime) = self_telemetry {
        runtime.stop_and_flush().await;
    }
    trace_candidates.shutdown().await;
    trace_pipeline.shutdown().await;
    if let Some(runtime) = apm_runtime {
        runtime.shutdown().await;
    }
    drain.begin_drain();
    if !has_ingester {
        // 无 ingester（纯 querier/router/compactor）：无待 flush 数据；drain 标记 + heartbeat
        // 注销已让集群停止路由。短暂 grace 让 in-flight 收尾后退出。
        tokio::time::sleep(Duration::from_millis(500)).await;
        return Ok(());
    }
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
    drain: Arc<DrainController>,
    self_telemetry: Option<Arc<crate::app::self_telemetry::SelfTelemetryRuntime>>,
    trace_candidates: Arc<crate::app::trace::candidate_router::TraceCandidateRouter>,
    trace_pipeline: Arc<crate::app::trace::TracePipeline>,
    apm_runtime: Option<Arc<crate::app::apm::ApmRuntime>>,
    has_ingester: bool,
    timeout: Duration,
) -> anyhow::Result<()> {
    tokio::signal::ctrl_c().await.ok();
    tracing::info!("ctrl-c received; draining before exit");
    if let Some(runtime) = self_telemetry {
        runtime.stop_and_flush().await;
    }
    trace_candidates.shutdown().await;
    trace_pipeline.shutdown().await;
    if let Some(runtime) = apm_runtime {
        runtime.shutdown().await;
    }
    drain.begin_drain();
    if !has_ingester {
        tokio::time::sleep(Duration::from_millis(500)).await;
        return Ok(());
    }
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

    #[test]
    fn standalone_runs_full_http_with_bundled_grpc() {
        assert_eq!(
            plan_servers(&[Role::Standalone]),
            ServerPlan {
                http: HttpKind::Full,
                grpc: false
            }
        );
    }

    #[test]
    fn router_only_proxies_http_no_grpc() {
        assert_eq!(
            plan_servers(&[Role::Router]),
            ServerPlan {
                http: HttpKind::Proxy,
                grpc: false
            }
        );
    }

    #[test]
    fn querier_only_runs_grpc_no_http() {
        assert_eq!(
            plan_servers(&[Role::Querier]),
            ServerPlan {
                http: HttpKind::None,
                grpc: true
            }
        );
    }

    #[test]
    fn ingester_querier_share_one_grpc_no_http() {
        assert_eq!(
            plan_servers(&[Role::Ingester, Role::Querier]),
            ServerPlan {
                http: HttpKind::None,
                grpc: true
            }
        );
    }

    #[test]
    fn router_plus_querier_proxies_and_serves_grpc() {
        assert_eq!(
            plan_servers(&[Role::Router, Role::Querier]),
            ServerPlan {
                http: HttpKind::Proxy,
                grpc: true
            }
        );
    }

    #[test]
    fn compactor_only_has_no_foreground_server() {
        assert_eq!(
            plan_servers(&[Role::Compactor]),
            ServerPlan {
                http: HttpKind::None,
                grpc: false
            }
        );
    }
}
