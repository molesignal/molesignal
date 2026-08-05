// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 排班相关路由：CRUD + on-call 查询。
//! 当前仅实装 on-call 查询；CRUD 等待 Section 11 后续 change 接入。

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{
        AppState,
        http::{
            federation::{delete_payload, emit_cud},
            middleware::ProtectedResource,
            routes::activity_audit,
        },
    },
    app::iam::IamContext,
    domain::{
        alerting::schedule::{Rotation, Schedule, ScheduleOverride},
        federation::{CudAction, ResourceKind},
        iam::{permission, resource_permission},
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 把保存后的 schedule 既序列化返回、又发一条 Updated 事件（override 增删也走这条）。
async fn saved_schedule_response(state: &AppState, org: &Id, s: Schedule) -> Json<Value> {
    let body = serde_json::to_value(&s).unwrap_or(Value::Null);
    emit_cud(
        state,
        org,
        ResourceKind::Schedule,
        CudAction::Updated,
        &s.id.0,
        &s,
    )
    .await;
    Json(body)
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/schedules", get(list).post(create))
        .route("/schedules/{id}", get(get_one).put(update).delete(delete))
        .route("/schedules/{id}/overrides", post(add_override))
        .route(
            "/schedules/{id}/overrides/{override_id}",
            axum::routing::put(update_override).delete(remove_override),
        )
        .route("/schedules/{id}/on-call", get(who_is_on_call))
}

#[async_trait::async_trait]
impl ProtectedResource for Schedule {
    type Id = Id;

    async fn load(state: &AppState, id: Self::Id) -> Result<Self> {
        state.alerting.service.get_schedule(&id).await
    }

    fn organization_id(&self) -> &Id {
        &self.org_id
    }

    fn resource_type(&self) -> &str {
        "schedule"
    }

    fn resource_id(&self) -> &str {
        self.id.as_str()
    }
}

#[derive(Deserialize)]
struct OnCallQuery {
    /// 不传则取当前时间（微秒 unix ts）
    pub at: Option<i64>,
}

#[derive(Deserialize)]
struct ScheduleWriteReq {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub team_id: Option<Id>,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    pub enabled: Option<bool>,
    #[serde(default)]
    pub rotations: Vec<Rotation>,
    #[serde(default)]
    pub overrides: Vec<ScheduleOverride>,
}

fn default_timezone() -> String {
    "UTC".to_string()
}

#[derive(Deserialize)]
struct OverrideReq {
    pub user_id: Id,
    pub start_at_micros: i64,
    pub end_at_micros: i64,
    #[serde(default)]
    pub reason: String,
}

#[permission("schedules.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let items = state.alerting.service.list_schedules(&ctx.org_id).await?;
    Ok(Json(
        serde_json::to_value(items).unwrap_or(Value::Array(vec![])),
    ))
}

#[permission("schedules.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<ScheduleWriteReq>,
) -> Result<Json<Value>> {
    if req.name.trim().is_empty() {
        return Err(crate::shared::Error::invalid("name cannot be empty"));
    }
    let now = TimestampMicros::now();
    let schedule = Schedule {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name: req.name,
        description: req.description,
        team_id: req.team_id,
        timezone: req.timezone,
        enabled: req.enabled.unwrap_or(true),
        rotations: req.rotations,
        overrides: req.overrides,
        created_by: Some(ctx.user_id.clone()),
        updated_by: Some(ctx.user_id.clone()),
        created_at: now,
        updated_at: now,
    };
    let saved = state.alerting.service.create_schedule(schedule).await?;
    emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::Schedule,
        CudAction::Created,
        &saved.id.0,
        &saved,
    )
    .await;
    activity_audit::record(
        &state,
        &ctx,
        "schedule.created",
        "schedule",
        &saved.id.0,
        json!({
            "name": saved.name,
            "team_id": saved.team_id,
            "timezone": saved.timezone,
            "rotation_count": saved.rotations.len(),
            "member_count": member_count(&saved),
        }),
    )
    .await;
    Ok(Json(serde_json::to_value(saved).unwrap_or(Value::Null)))
}

#[resource_permission(
    action = "schedules.read",
    resource = Schedule,
    id = Id::from_string(id),
    bind = schedule
)]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(serde_json::to_value(schedule).unwrap_or(Value::Null)))
}

#[resource_permission(
    action = "schedules.manage",
    resource = Schedule,
    id = Id::from_string(id),
    bind = existing
)]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<ScheduleWriteReq>,
) -> Result<Json<Value>> {
    if req.name.trim().is_empty() {
        return Err(crate::shared::Error::invalid("name cannot be empty"));
    }
    let enabled = req.enabled.unwrap_or(existing.enabled);
    let action = match (existing.enabled, enabled) {
        (true, false) => "schedule.paused",
        (false, true) => "schedule.resumed",
        _ => "schedule.updated",
    };
    let schedule = Schedule {
        id: existing.id,
        org_id: existing.org_id,
        name: req.name,
        description: req.description,
        team_id: req.team_id,
        timezone: req.timezone,
        enabled,
        rotations: req.rotations,
        overrides: req.overrides,
        created_by: existing.created_by,
        updated_by: Some(ctx.user_id.clone()),
        created_at: existing.created_at,
        updated_at: TimestampMicros::now(),
    };
    let saved = state.alerting.service.update_schedule(schedule).await?;
    activity_audit::record(
        &state,
        &ctx,
        action,
        "schedule",
        &saved.id.0,
        json!({
            "name": saved.name,
            "enabled": saved.enabled,
            "team_id": saved.team_id,
            "timezone": saved.timezone,
            "rotation_count": saved.rotations.len(),
            "member_count": member_count(&saved),
        }),
    )
    .await;
    let org_id = saved.org_id.clone();
    Ok(saved_schedule_response(&state, &org_id, saved).await)
}

#[resource_permission(
    action = "schedules.manage",
    resource = Schedule,
    id = Id::from_string(id),
    bind = existing
)]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<&'static str> {
    let rid = existing.id.clone();
    state.alerting.service.delete_schedule(&rid).await?;
    emit_cud(
        &state,
        &existing.org_id,
        ResourceKind::Schedule,
        CudAction::Deleted,
        &rid.0,
        &delete_payload(&rid.0),
    )
    .await;
    activity_audit::record(
        &state,
        &ctx,
        "schedule.deleted",
        "schedule",
        &rid.0,
        json!({ "name": existing.name }),
    )
    .await;
    Ok("deleted")
}

#[resource_permission(
    action = "schedules.manage",
    resource = Schedule,
    id = Id::from_string(id),
    bind = schedule
)]
async fn add_override(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<OverrideReq>,
) -> Result<Json<Value>> {
    if req.end_at_micros <= req.start_at_micros {
        return Err(crate::shared::Error::invalid(
            "end_at_micros must be greater than start_at_micros",
        ));
    }
    let mut schedule = schedule;
    validate_override_window(&schedule, None, req.start_at_micros, req.end_at_micros)?;
    let schedule_override = ScheduleOverride {
        id: Id::new(),
        user_id: req.user_id,
        start_at: TimestampMicros(req.start_at_micros),
        end_at: TimestampMicros(req.end_at_micros),
        reason: req.reason,
    };
    schedule.overrides.push(schedule_override.clone());
    schedule.updated_by = Some(ctx.user_id.clone());
    schedule.updated_at = TimestampMicros::now();
    let saved = state.alerting.service.update_schedule(schedule).await?;
    if let Err(error) = state
        .alerting
        .notify_engine
        .enqueue_event(crate::app::notify::override_created_dispatch(
            &saved,
            &schedule_override,
        ))
        .await
    {
        tracing::warn!(
            schedule_id = %saved.id,
            override_id = %schedule_override.id,
            error = %error,
            "on-call override notify event enqueue failed"
        );
    }
    activity_audit::record(
        &state,
        &ctx,
        "schedule.override_added",
        "schedule",
        &saved.id.0,
        json!({
            "override_id": schedule_override.id,
            "user_id": schedule_override.user_id,
            "start_at": schedule_override.start_at,
            "end_at": schedule_override.end_at,
            "reason": schedule_override.reason,
        }),
    )
    .await;
    let org_id = saved.org_id.clone();
    Ok(saved_schedule_response(&state, &org_id, saved).await)
}

#[resource_permission(
    action = "schedules.manage",
    resource = Schedule,
    id = Id::from_string(id),
    bind = schedule
)]
async fn update_override(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((id, override_id)): Path<(String, String)>,
    Json(req): Json<OverrideReq>,
) -> Result<Json<Value>> {
    if req.end_at_micros <= req.start_at_micros {
        return Err(crate::shared::Error::invalid(
            "end_at_micros must be greater than start_at_micros",
        ));
    }
    let mut schedule = schedule;
    validate_override_window(
        &schedule,
        Some(&override_id),
        req.start_at_micros,
        req.end_at_micros,
    )?;
    let schedule_override = schedule
        .overrides
        .iter_mut()
        .find(|ov| ov.id.0 == override_id)
        .ok_or_else(|| crate::shared::Error::not_found("schedule override not found"))?;
    schedule_override.user_id = req.user_id;
    schedule_override.start_at = TimestampMicros(req.start_at_micros);
    schedule_override.end_at = TimestampMicros(req.end_at_micros);
    schedule_override.reason = req.reason;
    let changed = schedule_override.clone();
    schedule.updated_by = Some(ctx.user_id.clone());
    schedule.updated_at = TimestampMicros::now();
    let saved = state.alerting.service.update_schedule(schedule).await?;
    activity_audit::record(
        &state,
        &ctx,
        "schedule.override_updated",
        "schedule",
        &saved.id.0,
        json!({
            "override_id": changed.id,
            "user_id": changed.user_id,
            "start_at": changed.start_at,
            "end_at": changed.end_at,
            "reason": changed.reason,
        }),
    )
    .await;
    let org_id = saved.org_id.clone();
    Ok(saved_schedule_response(&state, &org_id, saved).await)
}

#[resource_permission(
    action = "schedules.manage",
    resource = Schedule,
    id = Id::from_string(id),
    bind = schedule
)]
async fn remove_override(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((id, override_id)): Path<(String, String)>,
) -> Result<Json<Value>> {
    let mut schedule = schedule;
    let removed = schedule
        .overrides
        .iter()
        .find(|ov| ov.id.0 == override_id)
        .cloned()
        .ok_or_else(|| crate::shared::Error::not_found("schedule override not found"))?;
    schedule.overrides.retain(|ov| ov.id.0 != override_id);
    schedule.updated_by = Some(ctx.user_id.clone());
    schedule.updated_at = TimestampMicros::now();
    let saved = state.alerting.service.update_schedule(schedule).await?;
    activity_audit::record(
        &state,
        &ctx,
        "schedule.override_removed",
        "schedule",
        &saved.id.0,
        json!({
            "override_id": removed.id,
            "user_id": removed.user_id,
            "start_at": removed.start_at,
            "end_at": removed.end_at,
            "reason": removed.reason,
        }),
    )
    .await;
    let org_id = saved.org_id.clone();
    Ok(saved_schedule_response(&state, &org_id, saved).await)
}

#[resource_permission(
    action = "schedules.read",
    resource = Schedule,
    id = Id::from_string(id),
    bind = schedule
)]
async fn who_is_on_call(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Query(q): Query<OnCallQuery>,
) -> Result<Json<Value>> {
    let at =
        q.at.map(TimestampMicros)
            .unwrap_or_else(TimestampMicros::now);
    let user = schedule.who_is_on_call(at);
    Ok(Json(json!({ "user_id": user.map(|u| u.0) })))
}

fn validate_override_window(
    schedule: &Schedule,
    editing_id: Option<&str>,
    start_at_micros: i64,
    end_at_micros: i64,
) -> Result<()> {
    let overlaps = schedule.overrides.iter().any(|existing| {
        editing_id != Some(existing.id.0.as_str())
            && start_at_micros < existing.end_at.0
            && end_at_micros > existing.start_at.0
    });
    if overlaps {
        return Err(crate::shared::Error::invalid(
            "override overlaps an existing override",
        ));
    }
    Ok(())
}

fn member_count(schedule: &Schedule) -> usize {
    let mut members = std::collections::HashSet::new();
    for rotation in &schedule.rotations {
        for member in &rotation.members {
            members.insert(&member.0);
        }
    }
    members.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule_with_override() -> Schedule {
        let now = TimestampMicros::now();
        Schedule {
            id: Id::new(),
            org_id: Id::new(),
            name: "test".into(),
            description: String::new(),
            team_id: None,
            timezone: "UTC".into(),
            enabled: true,
            rotations: vec![],
            overrides: vec![ScheduleOverride {
                id: Id::from_string("override-1"),
                user_id: Id::new(),
                start_at: TimestampMicros(100),
                end_at: TimestampMicros(200),
                reason: "coverage".into(),
            }],
            created_by: None,
            updated_by: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn override_windows_must_not_overlap() {
        let schedule = schedule_with_override();
        assert!(validate_override_window(&schedule, None, 150, 250).is_err());
        assert!(validate_override_window(&schedule, None, 200, 250).is_ok());
        assert!(validate_override_window(&schedule, Some("override-1"), 150, 250).is_ok());
    }
}
