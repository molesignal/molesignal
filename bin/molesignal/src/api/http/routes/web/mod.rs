// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/api/v1/web/*` —— 前端 shell 专用的聚合 endpoints。
//!
//! 这一层不是新业务能力，是为了让前端在一次请求里拿到 ⌘K 召回结果 /
//! 服务拓扑 / trace 树 / 跨信号关联 / 投资栈 blob。每个 handler 把现有
//! repository / service 拼起来即可，不引入新 domain trait。
//!
//! 权限：所有 endpoint 使用数据库目录中的 `streams.query`；系统上下文可由
//! `sys.telemetry.read` 进入同一只读路径。

use axum::Router;

use crate::api::AppState;

pub mod correlation;
pub mod investigation_blob;
pub mod logs;
pub mod search;
pub mod topology;
pub mod trace;

pub fn routes() -> Router<AppState> {
    Router::new().nest("/web", inner())
}

fn inner() -> Router<AppState> {
    Router::new()
        .merge(search::routes())
        .merge(logs::routes())
        .merge(topology::routes())
        .merge(trace::routes())
        .merge(correlation::routes())
        .merge(investigation_blob::routes())
}
