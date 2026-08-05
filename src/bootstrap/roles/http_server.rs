// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use crate::{
    api::{AppState, grpc, http, http::client_ip::ClientIpResolverHandle},
    config::Settings,
};

const CLIENT_IP_SETTINGS_REFRESH_SECS: u64 = 30;

/// 启动 HTTP（axum）+ gRPC（tonic）服务器：同进程，`tokio::try_join!` 并行。
/// standalone / router 角色默认走这条路径。
///
/// 当 `cfg.http.tls.enabled=true`（仅  feature 实际可达）时，HTTP 分支
/// 切到双绑：80 端口纯 plain（健康检查 + ACME challenge + 301 redirect），443 端口
/// 跑完整 rustls 服务器配 SNI cert resolver（change `domain-acme-tls`）。
pub async fn serve(state: AppState, cfg: Settings) -> anyhow::Result<()> {
    let grpc_cfg = cfg.grpc.clone();
    let alive_window = cfg.cluster.peer_timeout_secs as i64;
    let client_ip = load_client_ip_resolver(&state).await;

    if cfg.http.tls.enabled {
        return serve_tls(state, cfg, grpc_cfg, alive_window, client_ip).await;
    }

    let app = http::build_router_with_client_ip(state.clone(), client_ip.clone());
    let addr: std::net::SocketAddr = format!("{}:{}", cfg.http.bind, cfg.http.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "http server listening");

    let http_fut = async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("http serve: {e}"))
    };
    let client_ip_refresh_fut = refresh_client_ip_resolver(state.clone(), client_ip);
    let flight_sql_fut = flight_sql_future(state.clone(), cfg.flight_sql.clone());
    let otlp_grpc_fut = otlp_grpc_future(state.clone(), cfg.otlp_grpc.clone());
    let grpc_fut = async move { grpc::serve_grpc(state, &grpc_cfg, alive_window).await };

    tokio::try_join!(
        http_fut,
        grpc_fut,
        flight_sql_fut,
        otlp_grpc_fut,
        client_ip_refresh_fut
    )?;
    Ok(())
}

/// 对外 Flight SQL listener（spec flight-sql）：`flight_sql.enabled = false`
/// （默认）时立即返回 Ok，`try_join!` 继续等其它 server。
async fn flight_sql_future(
    state: AppState,
    cfg: crate::config::FlightSqlSettings,
) -> anyhow::Result<()> {
    if !cfg.enabled {
        return Ok(());
    }
    grpc::serve_flight_sql(state, &cfg).await
}

/// 对外 OTLP gRPC listener（标准 :4317）。
async fn otlp_grpc_future(
    state: AppState,
    cfg: crate::config::OtlpGrpcSettings,
) -> anyhow::Result<()> {
    grpc::serve_otlp_grpc(state, &cfg).await
}

async fn serve_tls(
    state: AppState,
    cfg: Settings,
    grpc_cfg: crate::config::GrpcSettings,
    alive_window: i64,
    client_ip: ClientIpResolverHandle,
) -> anyhow::Result<()> {
    use std::sync::Arc;

    use axum::{Router, response::Redirect, routing::get};

    use crate::bootstrap::{acme::AcmeClient, tls::SniCertResolver, workers::acme::AcmeRunner};

    let tls = cfg.http.tls.clone();
    std::fs::create_dir_all(&tls.key_storage_dir)?;

    // SNI resolver 从 state.platform.domains 实时查
    let runtime = tokio::runtime::Handle::current();
    let resolver = Arc::new(SniCertResolver::new(
        state.platform.domains.clone(),
        std::path::PathBuf::from(&tls.key_storage_dir),
        runtime.clone(),
    ));

    // ACME runner（背景 issue / renewal）
    let acme_client = AcmeClient::new(
        tls.directory_url().to_string(),
        tls.account_email.clone(),
        std::path::PathBuf::from(&tls.key_storage_dir),
        state.platform.domains.clone(),
    );
    let runner = Arc::new(AcmeRunner::new(
        acme_client,
        state.platform.domains.clone(),
        resolver.clone(),
        tls.issue_poll_secs,
        tls.renewal_retry_secs,
    ));
    let _runner_handles = runner.spawn();

    // 443: full router + rustls
    let full_app = http::build_router_with_client_ip(state.clone(), client_ip.clone());
    let https_addr: std::net::SocketAddr = format!("{}:{}", cfg.http.bind, tls.port).parse()?;
    let rustls_config = {
        use rustls::ServerConfig;
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default(); // idempotent
        let mut sc = ServerConfig::builder()
            .with_no_client_auth()
            .with_cert_resolver(resolver.clone());
        sc.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        sc
    };
    let acceptor = axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(rustls_config));

    // 80: 仅 healthz + acme-challenge + 301 redirect
    let plain_addr: std::net::SocketAddr =
        format!("{}:{}", cfg.http.bind, tls.plain_port).parse()?;
    let challenge_state = state.clone();
    let acme_challenge_handler = move |axum::extract::Path(token): axum::extract::Path<String>| {
        let domains = challenge_state.platform.domains.clone();
        async move {
            match domains.get_challenge(&token).await {
                Ok(Some(c)) => (axum::http::StatusCode::OK, c.key_authorization).into_response(),
                _ => (axum::http::StatusCode::NOT_FOUND, "not found").into_response(),
            }
        }
    };
    let redirect_handler = |req: axum::extract::Request| async move {
        let host = req
            .headers()
            .get(axum::http::header::HOST)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("localhost")
            .to_string();
        let path = req
            .uri()
            .path_and_query()
            .map(|p| p.as_str().to_string())
            .unwrap_or_else(|| "/".into());
        Redirect::permanent(&format!("https://{host}{path}"))
    };
    let plain_app: Router = Router::new()
        .route(
            "/.well-known/acme-challenge/{token}",
            get(acme_challenge_handler),
        )
        .route("/healthz", get(|| async { "ok" }))
        .fallback(get(redirect_handler));

    let plain_listener = tokio::net::TcpListener::bind(plain_addr).await?;
    tracing::info!(addr=%plain_addr, "http server (plain :80 challenge+redirect) listening");
    tracing::info!(addr=%https_addr, "http server (rustls :443 SNI) listening");

    let plain_fut = async move {
        axum::serve(plain_listener, plain_app)
            .await
            .map_err(|e| anyhow::anyhow!("plain serve: {e}"))
    };
    let https_fut = async move {
        axum_server::bind_rustls(https_addr, acceptor)
            .serve(full_app.into_make_service_with_connect_info::<std::net::SocketAddr>())
            .await
            .map_err(|e| anyhow::anyhow!("rustls serve: {e}"))
    };
    let client_ip_refresh_fut = refresh_client_ip_resolver(state.clone(), client_ip);
    let flight_sql_fut = flight_sql_future(state.clone(), cfg.flight_sql.clone());
    let otlp_grpc_fut = otlp_grpc_future(state.clone(), cfg.otlp_grpc.clone());
    let grpc_fut = async move { grpc::serve_grpc(state, &grpc_cfg, alive_window).await };

    tokio::try_join!(
        plain_fut,
        https_fut,
        grpc_fut,
        flight_sql_fut,
        otlp_grpc_fut,
        client_ip_refresh_fut
    )?;
    Ok(())
}

async fn load_client_ip_resolver(state: &AppState) -> ClientIpResolverHandle {
    match state.iam.instance_settings.get().await {
        Ok(settings) => match ClientIpResolverHandle::new(&settings.rum_client_ip_resolver) {
            Ok(resolver) => resolver,
            Err(error) => {
                tracing::warn!(%error, "invalid RUM client IP resolver in database; using peer mode");
                ClientIpResolverHandle::peer()
            }
        },
        Err(error) => {
            tracing::warn!(%error, "cannot load RUM client IP resolver; using peer mode");
            ClientIpResolverHandle::peer()
        }
    }
}

async fn refresh_client_ip_resolver(
    state: AppState,
    resolver: ClientIpResolverHandle,
) -> anyhow::Result<()> {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(
        CLIENT_IP_SETTINGS_REFRESH_SECS,
    ));
    interval.tick().await;
    loop {
        interval.tick().await;
        match state.iam.instance_settings.get().await {
            Ok(settings) => {
                if let Err(error) = resolver.replace(&settings.rum_client_ip_resolver) {
                    tracing::warn!(%error, "invalid RUM client IP resolver refresh ignored");
                }
            }
            Err(error) => {
                tracing::warn!(%error, "RUM client IP resolver refresh failed");
            }
        }
    }
}

use axum::response::IntoResponse as _;
