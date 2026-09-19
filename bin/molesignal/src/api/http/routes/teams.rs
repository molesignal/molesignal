// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Teams routes.
//!
//! Reads are open to authenticated org members (escalation-policy editors need
//! to populate team pickers); writes require `iam.policies.manage`.
//! `member_ids` is a JSONB array of user ids on the team row. Org isolation is
//! enforced by re-checking `team.org_id` after the by-id load (the repository
//! keys on id alone).

use std::collections::HashSet;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use serde::Deserialize;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::{Team, permission},
    shared::{Error, Result, ids::Id},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/teams", get(list_teams).post(create_team))
        .route(
            "/teams/{id}",
            get(get_team).put(update_team).delete(delete_team),
        )
}

#[derive(Deserialize)]
struct TeamWriteReq {
    name: String,
    #[serde(default)]
    member_ids: Vec<String>,
}

fn validate_team(req: &TeamWriteReq) -> Result<()> {
    if req.name.trim().is_empty() {
        return Err(Error::invalid("name cannot be empty"));
    }
    if req.name.chars().count() > 255 {
        return Err(Error::invalid("name must be at most 255 characters"));
    }
    Ok(())
}

/// Reject any member id that is not a current member of the org. `users` are a
/// global table (org association lives in `iam_memberships`), so without this an
/// admin could embed arbitrary / cross-org user ids — which then become silent
/// escalation targets. Returns the validated ids.
async fn resolve_team_members(
    state: &AppState,
    org_id: &Id,
    raw_member_ids: Vec<String>,
) -> Result<Vec<Id>> {
    if raw_member_ids.is_empty() {
        return Ok(Vec::new());
    }
    let org_members: HashSet<String> = state
        .iam
        .service
        .iam_memberships
        .list_for_org(org_id)
        .await?
        .into_iter()
        .map(|m| m.user_id.0)
        .collect();
    let mut ids = Vec::with_capacity(raw_member_ids.len());
    for raw in raw_member_ids {
        if !org_members.contains(&raw) {
            return Err(Error::invalid(
                "member_ids must reference users in this organization",
            ));
        }
        ids.push(Id::from_string(raw));
    }
    Ok(ids)
}

async fn list_teams(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
) -> Result<Json<Vec<Team>>> {
    Ok(Json(state.iam.teams.list(&ctx.org_id).await?))
}

async fn get_team(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Team>> {
    let team = state.iam.teams.get(&Id::from_string(id)).await?;
    if team.org_id != ctx.org_id {
        return Err(Error::forbidden("team belongs to another org"));
    }
    Ok(Json(team))
}

#[permission("iam.policies.manage")]
async fn create_team(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Json(req): Json<TeamWriteReq>,
) -> Result<Json<Team>> {
    validate_team(&req)?;
    let member_ids = resolve_team_members(&state, &ctx.org_id, req.member_ids).await?;
    let team = Team {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name: req.name,
        member_ids,
    };
    Ok(Json(state.iam.teams.create(team).await?))
}

#[permission("iam.policies.manage")]
async fn update_team(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<TeamWriteReq>,
) -> Result<Json<Team>> {
    validate_team(&req)?;
    let existing = state.iam.teams.get(&Id::from_string(id)).await?;
    if existing.org_id != ctx.org_id {
        return Err(Error::forbidden("team belongs to another org"));
    }
    let member_ids = resolve_team_members(&state, &ctx.org_id, req.member_ids).await?;
    let team = Team {
        id: existing.id,
        org_id: ctx.org_id.clone(),
        name: req.name,
        member_ids,
    };
    Ok(Json(state.iam.teams.update(team).await?))
}

#[permission("iam.policies.manage")]
async fn delete_team(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let existing = state.iam.teams.get(&Id::from_string(id)).await?;
    if existing.org_id != ctx.org_id {
        return Err(Error::forbidden("team belongs to another org"));
    }
    state.iam.teams.delete(&existing.id).await?;
    Ok(StatusCode::NO_CONTENT)
}
