// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 每个 role 都可启动的独立节点级 pprof listener。

use std::net::SocketAddr;

use crate::{
    api::{AppState, http::routes::profiling::pprof_routes},
    config::ProfilingSettings,
};

pub async fn serve(state: AppState, settings: ProfilingSettings) -> anyhow::Result<()> {
    let address: SocketAddr = format!("{}:{}", settings.bind, settings.port).parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(
        addr = %address,
        allow_remote = settings.allow_remote,
        "pprof profiling listener active"
    );
    axum::serve(listener, pprof_routes().with_state(state))
        .await
        .map_err(|error| anyhow::anyhow!("pprof listener: {error}"))
}
