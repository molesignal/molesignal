// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 重大事件与内部复盘：
//!
//! - `GET /alerts/incidents/{id}/response`：宣布了没有、角色、作战室、会议、任务；
//! - `POST /alerts/incidents/{id}/declare`、`PUT /alerts/incidents/{id}/roles`、
//!   `POST /alerts/incidents/{id}/tasks`、`PUT/DELETE /alerts/incidents/{id}/tasks/{task_id}`、
//!   `POST /alerts/incidents/{id}/status-updates`：处置（宣布要 `alerts.incidents.escalate`，其余要 `alerts.incidents.update`）；
//! - `GET/PUT /alerts/response/settings`：组织的设置（看要 `alerts.response.read`，改要 `alerts.response.manage`）。会议的密钥
//!   随 `secrets` 写入，只写不读，读出来只有 `secrets_set`；还带上能建作战室的连接器类型 `war_room_types`；
//! - `GET /alerts/postmortems`、`GET/POST /alerts/incidents/{id}/postmortem`、`PUT /alerts/postmortems/{id}`、
//!   `POST /alerts/postmortems/{id}/actions`、`PUT/DELETE /alerts/postmortems/{id}/actions/{action_id}`、
//!   `POST /alerts/postmortems/{id}/actions/{action_id}/ticket`：复盘（看要 `postmortems.read`，改要
//!   `postmortems.manage`）。

use std::collections::BTreeMap;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{
        alerting::{
            postmortems::{ActionPatch, PostmortemPatch},
            response::war_room::WAR_ROOM_TYPES,
        },
        iam::IamContext,
    },
    domain::{
        alerting::{
            incident::Incident,
            postmortem::{ActionItemStatus, Postmortem, PostmortemStatus},
            response::{ResponseRole, ResponseSettings, StatusUpdate},
        },
        iam::{permission, resource_permission},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/alerts/incidents/{id}/response", get(get_response))
        .route("/alerts/incidents/{id}/declare", post(declare))
        .route("/alerts/incidents/{id}/roles", put(assign_role))
        .route("/alerts/incidents/{id}/tasks", post(add_task))
        .route(
            "/alerts/incidents/{id}/tasks/{taskId}",
            put(update_task).delete(delete_task),
        )
        .route("/alerts/incidents/{id}/status-updates", post(status_update))
        .route(
            "/alerts/response/settings",
            get(get_settings).put(save_settings),
        )
        .route("/alerts/postmortems", get(list_postmortems))
        .route(
            "/alerts/incidents/{id}/postmortem",
            get(incident_postmortem).post(create_postmortem),
        )
        .route("/alerts/postmortems/{id}", put(update_postmortem))
        .route("/alerts/postmortems/{id}/actions", post(add_action))
        .route(
            "/alerts/postmortems/{id}/actions/{actionId}",
            put(update_action).delete(delete_action),
        )
        .route(
            "/alerts/postmortems/{id}/actions/{actionId}/ticket",
            post(action_ticket),
        )
}

/// 分清没传与传了 null：没传是 `None`，null 是 `Some(None)`。
fn present<'de, D, T>(deserializer: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

async fn response_json(state: &AppState, incident: &Incident) -> Result<Value> {
    let (response, tasks) = state.alerting.response.get(incident).await?;

    Ok(json!({ "response": response, "tasks": tasks }))
}

#[resource_permission(action = "alerts.incidents.read", resource = Incident, id = Id::from_string(id), bind = incident)]
async fn get_response(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(response_json(&state, &incident).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeclareReq {
    #[serde(default)]
    commander_id: Option<Id>,
}

#[resource_permission(action = "alerts.incidents.escalate", resource = Incident, id = Id::from_string(id), bind = incident)]
async fn declare(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<DeclareReq>,
) -> Result<Json<Value>> {
    state
        .alerting
        .response
        .declare(&incident, Some(ctx.principal_id()), req.commander_id)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "incident.declared_major",
        "incident",
        &incident.id.0,
        json!({}),
    )
    .await;

    Ok(Json(response_json(&state, &incident).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoleReq {
    role: ResponseRole,
    #[serde(default)]
    user_id: Option<Id>,
}

#[resource_permission(action = "alerts.incidents.update", resource = Incident, id = Id::from_string(id), bind = incident)]
async fn assign_role(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<RoleReq>,
) -> Result<Json<Value>> {
    state
        .alerting
        .response
        .assign_role(&incident, req.role, req.user_id, ctx.principal_id())
        .await?;

    Ok(Json(response_json(&state, &incident).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskReq {
    #[serde(default)]
    title: Option<String>,
    /// 不传不改；传 null 撤下负责人。
    #[serde(default, deserialize_with = "present")]
    assignee_id: Option<Option<Id>>,
    #[serde(default)]
    done: Option<bool>,
}

#[resource_permission(action = "alerts.incidents.update", resource = Incident, id = Id::from_string(id), bind = incident)]
async fn add_task(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<TaskReq>,
) -> Result<Json<Value>> {
    let task = state
        .alerting
        .response
        .add_task(
            &incident,
            req.title.as_deref().unwrap_or_default(),
            req.assignee_id.flatten(),
            ctx.principal_id(),
        )
        .await?;

    Ok(Json(serde_json::to_value(task).unwrap_or(Value::Null)))
}

#[resource_permission(action = "alerts.incidents.update", resource = Incident, id = Id::from_string(id), bind = incident)]
async fn update_task(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((id, task_id)): Path<(String, String)>,
    Json(req): Json<TaskReq>,
) -> Result<Json<Value>> {
    let task = state
        .alerting
        .response
        .update_task(
            &incident,
            &Id::from_string(task_id),
            req.title.as_deref(),
            req.assignee_id,
            req.done,
            ctx.principal_id(),
        )
        .await?;

    Ok(Json(serde_json::to_value(task).unwrap_or(Value::Null)))
}

#[resource_permission(action = "alerts.incidents.update", resource = Incident, id = Id::from_string(id), bind = incident)]
async fn delete_task(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((id, task_id)): Path<(String, String)>,
) -> Result<Json<Value>> {
    if !state
        .alerting
        .response
        .repository()
        .delete_task(&incident.org_id, &incident.id, &Id::from_string(task_id))
        .await?
    {
        return Err(Error::not_found("task not found"));
    }

    Ok(Json(json!({ "deleted": true })))
}

#[resource_permission(action = "alerts.incidents.update", resource = Incident, id = Id::from_string(id), bind = incident)]
async fn status_update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<StatusUpdate>,
) -> Result<Json<Value>> {
    state
        .alerting
        .response
        .status_update(&incident, req, ctx.principal_id())
        .await?;

    Ok(Json(json!({ "sent": true })))
}

#[permission("alerts.response.read")]
async fn get_settings(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let settings = state
        .alerting
        .response
        .repository()
        .settings(&ctx.org_id)
        .await?;

    Ok(Json(settings_json(&settings)))
}

/// 给页面的设置：密钥不出库，另附能建作战室的连接器类型。
fn settings_json(settings: &ResponseSettings) -> Value {
    let mut value = serde_json::to_value(settings).unwrap_or(Value::Null);

    if let Some(object) = value.as_object_mut() {
        object.insert("warRoomTypes".into(), json!(WAR_ROOM_TYPES));
    }

    value
}

#[derive(Deserialize)]
struct SettingsReq {
    #[serde(flatten)]
    settings: ResponseSettings,
    /// 新填的会议密钥；留空的保持原样。
    #[serde(default)]
    secrets: BTreeMap<String, String>,
}

#[permission("alerts.response.manage")]
async fn save_settings(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<SettingsReq>,
) -> Result<Json<Value>> {
    let saved = state
        .alerting
        .response
        .save_settings(&ctx.org_id, req.settings, req.secrets, ctx.principal_id())
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "incident_response.settings_updated",
        "organization",
        &ctx.org_id.0,
        json!({}),
    )
    .await;

    Ok(Json(settings_json(&saved)))
}

#[permission("postmortems.read")]
async fn list_postmortems(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let postmortems = state.alerting.postmortems.list(&ctx.org_id).await?;

    Ok(Json(json!({ "postmortems": postmortems })))
}

async fn postmortem_json(state: &AppState, postmortem: &Postmortem) -> Result<Value> {
    let actions = state
        .alerting
        .postmortems
        .repository()
        .list_actions(&postmortem.org_id, &postmortem.id)
        .await?;

    Ok(json!({ "postmortem": postmortem, "actions": actions }))
}

#[resource_permission(action = "alerts.incidents.read", resource = Incident, id = Id::from_string(id), bind = incident)]
async fn incident_postmortem(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    crate::api::http::middleware::Permission::require_key(&ctx, "postmortems.read")?;

    match state
        .alerting
        .postmortems
        .repository()
        .for_incident(&incident.org_id, &incident.id)
        .await?
    {
        Some(postmortem) => Ok(Json(postmortem_json(&state, &postmortem).await?)),
        None => Ok(Json(json!({ "postmortem": null, "actions": [] }))),
    }
}

#[derive(Deserialize)]
struct CreatePostmortemReq {
    #[serde(default)]
    title: String,
    #[serde(default)]
    body: String,
}

#[resource_permission(action = "alerts.incidents.read", resource = Incident, id = Id::from_string(id), bind = incident)]
async fn create_postmortem(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<CreatePostmortemReq>,
) -> Result<Json<Value>> {
    crate::api::http::middleware::Permission::require_key(&ctx, "postmortems.manage")?;

    let postmortem = state
        .alerting
        .postmortems
        .create(&incident, &req.title, &req.body, ctx.principal_id())
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "postmortem.created",
        "incident",
        &incident.id.0,
        json!({ "postmortemId": postmortem.id }),
    )
    .await;

    Ok(Json(postmortem_json(&state, &postmortem).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdatePostmortemReq {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    status: Option<PostmortemStatus>,
    #[serde(default, deserialize_with = "present")]
    owner_id: Option<Option<Id>>,
    #[serde(default)]
    reviewer_ids: Option<Vec<Id>>,
}

async fn own_postmortem(state: &AppState, ctx: &IamContext, id: &str) -> Result<Postmortem> {
    state
        .alerting
        .postmortems
        .repository()
        .get(&ctx.org_id, &Id::from_string(id))
        .await?
        .ok_or_else(|| Error::not_found("postmortem not found"))
}

#[permission("postmortems.manage")]
async fn update_postmortem(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdatePostmortemReq>,
) -> Result<Json<Value>> {
    let postmortem = state
        .alerting
        .postmortems
        .update(
            &ctx.org_id,
            &Id::from_string(id),
            PostmortemPatch {
                title: req.title,
                body: req.body,
                status: req.status,
                owner_id: req.owner_id,
                reviewer_ids: req.reviewer_ids,
            },
        )
        .await?;

    Ok(Json(postmortem_json(&state, &postmortem).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActionReq {
    #[serde(default)]
    title: Option<String>,
    #[serde(default, deserialize_with = "present")]
    owner_id: Option<Option<Id>>,
    #[serde(default, deserialize_with = "present")]
    due_at: Option<Option<i64>>,
    #[serde(default)]
    status: Option<ActionItemStatus>,
}

#[permission("postmortems.manage")]
async fn add_action(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<ActionReq>,
) -> Result<Json<Value>> {
    let postmortem = own_postmortem(&state, &ctx, &id).await?;
    let item = state
        .alerting
        .postmortems
        .add_action(
            &postmortem,
            req.title.as_deref().unwrap_or_default(),
            req.owner_id.flatten(),
            req.due_at.flatten().map(TimestampMicros),
        )
        .await?;

    Ok(Json(serde_json::to_value(item).unwrap_or(Value::Null)))
}

#[permission("postmortems.manage")]
async fn update_action(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((id, action_id)): Path<(String, String)>,
    Json(req): Json<ActionReq>,
) -> Result<Json<Value>> {
    let postmortem = own_postmortem(&state, &ctx, &id).await?;
    let item = state
        .alerting
        .postmortems
        .update_action(
            &postmortem,
            &Id::from_string(action_id),
            ActionPatch {
                title: req.title,
                owner_id: req.owner_id,
                due_at: req.due_at.map(|due| due.map(TimestampMicros)),
                status: req.status,
            },
        )
        .await?;

    Ok(Json(serde_json::to_value(item).unwrap_or(Value::Null)))
}

#[permission("postmortems.manage")]
async fn delete_action(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((id, action_id)): Path<(String, String)>,
) -> Result<Json<Value>> {
    let postmortem = own_postmortem(&state, &ctx, &id).await?;

    if !state
        .alerting
        .postmortems
        .repository()
        .delete_action(
            &postmortem.org_id,
            &postmortem.id,
            &Id::from_string(action_id),
        )
        .await?
    {
        return Err(Error::not_found("action item not found"));
    }

    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActionTicketReq {
    connector_id: Id,
}

#[permission("postmortems.manage")]
async fn action_ticket(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((id, action_id)): Path<(String, String)>,
    Json(req): Json<ActionTicketReq>,
) -> Result<Json<Value>> {
    let postmortem = own_postmortem(&state, &ctx, &id).await?;
    let incident = state
        .alerting
        .response
        .incident(&ctx.org_id, &postmortem.incident_id)
        .await?;
    let item = state
        .alerting
        .postmortems
        .ticket_for_action(
            &postmortem,
            &incident,
            &Id::from_string(action_id),
            &req.connector_id,
            ctx.principal_id(),
        )
        .await?;

    Ok(Json(serde_json::to_value(item).unwrap_or(Value::Null)))
}
