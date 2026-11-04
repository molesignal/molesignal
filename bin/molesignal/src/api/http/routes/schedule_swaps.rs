// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 换班申请：
//!
//! - `GET /schedules/swaps`：我发起的和请我的；
//! - `GET/POST /schedules/{id}/swaps`：一个排班的申请，发起一份；
//! - `POST /schedules/{id}/swaps/{swap_id}/{accept|decline|cancel}`：被请的人答复，发起的人撤回；
//! - `POST /schedules/{id}/swaps/{swap_id}/{approve|reject}`：排班要批准时，有管理权限的人批准或驳回。
//!
//! 能看排班就能发起和答复，生效的替班由对方的接受（和管理员的批准）带来。

use std::collections::HashMap;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{
        AppState,
        http::{federation::emit_cud, routes::activity_audit},
    },
    app::{
        alerting::oncall::{NewSwap, SwapOutcome},
        iam::IamContext,
    },
    domain::{
        alerting::{schedule::Schedule, swap::SwapRequest},
        federation::{CudAction, ResourceKind},
        iam::{permission, resource_permission},
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/schedules/swaps", get(mine))
        .route("/schedules/{id}/swaps", get(list).post(request))
        .route("/schedules/{id}/swaps/{swapId}/accept", post(accept))
        .route("/schedules/{id}/swaps/{swapId}/decline", post(decline))
        .route("/schedules/{id}/swaps/{swapId}/cancel", post(cancel))
        .route("/schedules/{id}/swaps/{swapId}/approve", post(approve))
        .route("/schedules/{id}/swaps/{swapId}/reject", post(reject))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwapReq {
    target_id: Id,
    start_at: i64,
    end_at: i64,
    #[serde(default)]
    note: String,
}

#[permission("schedules.read")]
async fn mine(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let swaps = state
        .alerting
        .swaps
        .for_user(&ctx.org_id, &ctx.user_id)
        .await?;

    let names: HashMap<Id, String> = state
        .alerting
        .service
        .list_schedules(&ctx.org_id)
        .await?
        .into_iter()
        .map(|schedule| (schedule.id, schedule.name))
        .collect();

    let swaps: Vec<Value> = swaps
        .iter()
        .map(|swap| swap_json(swap, names.get(&swap.schedule_id)))
        .collect();

    Ok(Json(json!({ "swaps": swaps })))
}

#[resource_permission(
    action = "schedules.read",
    resource = Schedule,
    id = Id::from_string(id),
    bind = schedule
)]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let swaps = state.alerting.swaps.for_schedule(&schedule).await?;

    let swaps: Vec<Value> = swaps
        .iter()
        .map(|swap| swap_json(swap, Some(&schedule.name)))
        .collect();

    Ok(Json(json!({ "swaps": swaps })))
}

#[resource_permission(
    action = "schedules.swaps.request",
    resource = Schedule,
    id = Id::from_string(id),
    bind = schedule
)]
async fn request(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<SwapReq>,
) -> Result<Json<Value>> {
    let swap = state
        .alerting
        .swaps
        .request(
            &schedule,
            &ctx.user_id,
            NewSwap {
                target_id: req.target_id,
                start_at: TimestampMicros(req.start_at),
                end_at: TimestampMicros(req.end_at),
                note: req.note,
            },
        )
        .await?;

    audit(&state, &ctx, "schedule.swap_requested", &swap).await;

    Ok(Json(swap_json(&swap, Some(&schedule.name))))
}

#[resource_permission(
    action = "schedules.swaps.request",
    resource = Schedule,
    id = Id::from_string(id),
    bind = schedule
)]
async fn accept(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((id, swap_id)): Path<(String, String)>,
) -> Result<Json<Value>> {
    let outcome = state
        .alerting
        .swaps
        .respond(&schedule, &Id::from_string(swap_id), &ctx.user_id, true)
        .await?;

    finish(&state, &ctx, &schedule, outcome, "schedule.swap_accepted").await
}

#[resource_permission(
    action = "schedules.swaps.request",
    resource = Schedule,
    id = Id::from_string(id),
    bind = schedule
)]
async fn decline(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((id, swap_id)): Path<(String, String)>,
) -> Result<Json<Value>> {
    let outcome = state
        .alerting
        .swaps
        .respond(&schedule, &Id::from_string(swap_id), &ctx.user_id, false)
        .await?;

    finish(&state, &ctx, &schedule, outcome, "schedule.swap_declined").await
}

#[resource_permission(
    action = "schedules.swaps.request",
    resource = Schedule,
    id = Id::from_string(id),
    bind = schedule
)]
async fn cancel(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((id, swap_id)): Path<(String, String)>,
) -> Result<Json<Value>> {
    let swap = state
        .alerting
        .swaps
        .cancel(&schedule, &Id::from_string(swap_id), &ctx.user_id)
        .await?;

    audit(&state, &ctx, "schedule.swap_cancelled", &swap).await;

    Ok(Json(swap_json(&swap, Some(&schedule.name))))
}

#[resource_permission(
    action = "schedules.swaps.approve",
    resource = Schedule,
    id = Id::from_string(id),
    bind = schedule
)]
async fn approve(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((id, swap_id)): Path<(String, String)>,
) -> Result<Json<Value>> {
    let outcome = state
        .alerting
        .swaps
        .decide(&schedule, &Id::from_string(swap_id), &ctx.user_id, true)
        .await?;

    finish(&state, &ctx, &schedule, outcome, "schedule.swap_approved").await
}

#[resource_permission(
    action = "schedules.swaps.approve",
    resource = Schedule,
    id = Id::from_string(id),
    bind = schedule
)]
async fn reject(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((id, swap_id)): Path<(String, String)>,
) -> Result<Json<Value>> {
    let outcome = state
        .alerting
        .swaps
        .decide(&schedule, &Id::from_string(swap_id), &ctx.user_id, false)
        .await?;

    finish(&state, &ctx, &schedule, outcome, "schedule.swap_rejected").await
}

/// 记一笔审计；生效时排班变了，再发一条排班更新。
async fn finish(
    state: &AppState,
    ctx: &IamContext,
    schedule: &Schedule,
    outcome: SwapOutcome,
    action: &str,
) -> Result<Json<Value>> {
    audit(state, ctx, action, &outcome.request).await;

    if let Some(saved) = &outcome.schedule {
        emit_cud(
            state,
            &saved.org_id,
            ResourceKind::Schedule,
            CudAction::Updated,
            &saved.id.0,
            saved,
        )
        .await;
    }

    Ok(Json(swap_json(&outcome.request, Some(&schedule.name))))
}

async fn audit(state: &AppState, ctx: &IamContext, action: &str, swap: &SwapRequest) {
    activity_audit::record(
        state,
        ctx,
        action,
        "schedule",
        &swap.schedule_id.0,
        json!({
            "swapId": swap.id,
            "status": swap.status,
            "requesterId": swap.requester_id,
            "targetId": swap.target_id,
            "startAt": swap.start_at,
            "endAt": swap.end_at,
            "overrideId": swap.override_id,
            "note": swap.note,
        }),
    )
    .await;
}

fn swap_json(swap: &SwapRequest, schedule_name: Option<&String>) -> Value {
    let mut value = serde_json::to_value(swap).unwrap_or(Value::Null);

    if let Some(object) = value.as_object_mut() {
        object.insert("scheduleName".into(), json!(schedule_name));
    }

    value
}
