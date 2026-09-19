// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use serde_json::json;

use crate::{api::AppState, shared::build_info};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/version", get(version))
}

/// 构建/部署信息：product version + git commit/branch + build ID + 运行时发布通道 + 授权版本。
/// 前端「关于」弹窗读取此处，状态栏用它显示 build 版本并兼作心跳（响应=已连接）。
async fn version() -> impl IntoResponse {
    Json(json!({
        "version": env!("MOLESIGNAL_PRODUCT_VERSION"),
        "commit": option_env!("MOLESIGNAL_GIT_COMMIT").unwrap_or("unknown"),
        "branch": option_env!("MOLESIGNAL_GIT_BRANCH").unwrap_or("unknown"),
        "build_epoch_secs": option_env!("MOLESIGNAL_BUILD_EPOCH")
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0),
        "build_id": option_env!("MOLESIGNAL_BUILD_ID").unwrap_or("unknown"),
        "release_channel": build_info::release_channel(),
        "edition": if cfg!(feature = "enterprise") { "enterprise" } else { "oss" },
    }))
    .into_response()
}

/// 综合健康：`probe.is_healthy()`；任意 sub-system degraded → 503 + JSON body。
async fn healthz(State(state): State<AppState>) -> impl IntoResponse {
    let (healthy, reason) = state.telemetry.probe.snapshot();
    if healthy {
        (StatusCode::OK, Json(json!({"status": "ok"}))).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "degraded",
                "reason": reason.unwrap_or("unknown"),
            })),
        )
            .into_response()
    }
}

/// `readyz` 单独检查 replay：未 replay 完返 503，已 replay 完即使 object_store degraded 也返 ready
/// （便于 k8s 在写路径降级时仍认为 pod 可被读流量使用）。
async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    if state.telemetry.probe.is_replay_done() {
        (StatusCode::OK, Json(json!({"status": "ready"}))).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "not-ready", "reason": "wal replay in progress"})),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn product_version_is_independent_from_the_rust_package_version() {
        assert_eq!(env!("MOLESIGNAL_PRODUCT_VERSION"), "26.0.0.0");
        assert_ne!(
            env!("MOLESIGNAL_PRODUCT_VERSION"),
            env!("CARGO_PKG_VERSION")
        );
    }

    #[test]
    fn build_id_is_injected_by_cargo() {
        assert!(!env!("MOLESIGNAL_BUILD_ID").is_empty());
    }
}
