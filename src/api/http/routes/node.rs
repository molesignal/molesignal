// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 节点优雅退役（drain）HTTP（读取与修改权限分离；非免鉴权白名单）。
//!
//! - `POST /node/drain` → 触发退役：停接新写入（ingest 返 503），ingester 把 pending buffer
//!   flush 干净后转 `drained`，compactor 停活。幂等（已 draining/drained 时 `newly_started=false`）。
//! - `GET  /node/drain` → 查询退役状态。`phase=drained` 表示 pending 数据已全部落盘，可安全下线。
//!   纯 querier 等无 ingester 角色的节点停在 `draining`（无待 flush 数据）即「可下线」。

use axum::{Extension, Json, Router, extract::State, routing::post};
use serde::Serialize;

use crate::{api::AppState, app::iam::IamContext, domain::iam::permission, shared::Result};

pub fn routes() -> Router<AppState> {
    Router::new().route("/node/drain", post(begin_drain).get(drain_status))
}

#[derive(Debug, Serialize)]
pub struct DrainResp {
    /// `running` / `draining` / `drained`。
    pub phase: &'static str,
    /// 是否仍接受写入（仅 `running`）。
    pub accepts_writes: bool,
    /// 本次请求是否触发了状态转移（仅 `POST` 且原为 `running` 时为 true）。
    pub newly_started: bool,
}

#[permission("org.settings.manage")]
async fn begin_drain(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<DrainResp>> {
    let newly_started = state.cluster.drain.begin_drain();
    tracing::info!(org = %ctx.org_id.0, newly_started, "node drain requested");
    Ok(Json(DrainResp {
        phase: state.cluster.drain.phase().as_str(),
        accepts_writes: state.cluster.drain.accepts_writes(),
        newly_started,
    }))
}

#[permission("org.settings.read")]
async fn drain_status(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<DrainResp>> {
    Ok(Json(DrainResp {
        phase: state.cluster.drain.phase().as_str(),
        accepts_writes: state.cluster.drain.accepts_writes(),
        newly_started: false,
    }))
}
