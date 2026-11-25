// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use domain::{
    iam::{IamContext, permission},
    node_agent::{NodeTask, TaskFilter, TaskQuota, TaskState},
};
use kernel::{Error, Result, ids::Id};

use crate::{
    api::AppState,
    app::node_agent::{Actor, CreateTaskInput},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/node-agents/tasks", get(list).post(create))
        .route("/node-agents/tasks/{taskId}", get(get_task))
        .route("/node-agents/tasks/{taskId}/cancel", post(cancel))
        .route("/node-agents/task-quota", get(get_quota).put(set_quota))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListQuery {
    node_id: Option<String>,
    /// Comma-separated states.
    state: Option<String>,
    limit: Option<u32>,
}

#[permission("node_agents.tasks.read")]
async fn list(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<NodeTask>>> {
    let states = query
        .state
        .as_deref()
        .unwrap_or_default()
        .split(',')
        .filter(|value| !value.is_empty())
        .map(|value| {
            TaskState::parse(value.trim())
                .ok_or_else(|| Error::invalid(format!("unknown task state `{value}`")))
        })
        .collect::<Result<Vec<_>>>()?;

    let filter = TaskFilter {
        node_id: query.node_id.map(Id),
        states,
        limit: query.limit.unwrap_or(100),
    };

    Ok(Json(
        state
            .node_agents
            .list_tasks(&context.org_id, filter)
            .await?,
    ))
}

#[permission("node_agents.tasks.run")]
async fn create(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(input): Json<CreateTaskInput>,
) -> Result<(StatusCode, Json<NodeTask>)> {
    let task = state
        .node_agents
        .create_task(&context.org_id, &Actor::from_context(&context), input)
        .await?;

    Ok((StatusCode::CREATED, Json(task)))
}

#[permission("node_agents.tasks.read")]
async fn get_task(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(task_id): Path<String>,
) -> Result<Json<NodeTask>> {
    Ok(Json(
        state
            .node_agents
            .get_task(&context.org_id, &Id(task_id))
            .await?,
    ))
}

#[permission("node_agents.tasks.run")]
async fn cancel(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(task_id): Path<String>,
) -> Result<Json<NodeTask>> {
    Ok(Json(
        state
            .node_agents
            .cancel_task(
                &context.org_id,
                &Actor::from_context(&context),
                &Id(task_id),
            )
            .await?,
    ))
}

#[permission("node_agents.tasks.read")]
async fn get_quota(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<TaskQuota>> {
    Ok(Json(state.node_agents.task_quota(&context.org_id).await?))
}

#[permission("node_agents.manage")]
async fn set_quota(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(quota): Json<TaskQuota>,
) -> Result<Json<TaskQuota>> {
    Ok(Json(
        state
            .node_agents
            .set_task_quota(&context.org_id, &Actor::from_context(&context), quota)
            .await?,
    ))
}
