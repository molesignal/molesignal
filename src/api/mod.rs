// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MoleSignal 表现层：HTTP + gRPC。
//!
//! HTTP 用 axum，按 domain 上下文拆 router：
//! - `/api/v1/ingest/*`     -> ingestion
//! - `/api/v1/query`        -> query
//! - `/api/v1/streams/*`    -> stream
//! - `/api/v1/dashboards/*` -> dashboard
//! - `/api/v1/alerts/*`     -> alerting（规则）
//! - `/api/v1/incidents/*`  -> alerting（事件）
//! - `/api/v1/schedules/*`  -> alerting（排班）
//! - `/api/v1/escalations/*`-> alerting（升级策略）
//! - `/api/v1/channels/*`   -> alerting（通知渠道）
//! - `/api/v1/orgs/*`       -> IAM
//! - `/api/v1/users/*`      -> IAM
//! - `/api/v1/auth/*`       -> IAM（登录、登出、token）

pub mod grpc;
pub mod http;
pub mod rca;
pub mod state;

// B3 heap profiling：Linux + jemalloc 下经 mallctl dump 堆采样的能力，提到 crate 根方便
// HTTP 端点之外（诊断 example、未来信号处理器等）复用。
pub use state::AppState;

#[cfg(all(target_os = "linux", target_env = "gnu", feature = "jemalloc"))]
pub use crate::app::profiling::dump_heap_profile_native as dump_heap_profile;
