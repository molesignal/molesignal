// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::{Value, json};

use super::super::OperationOutcome;
use crate::{
    agent::model::ApprovalRequest,
    api::{AppState, http::middleware::Permission},
    app::iam::IamContext,
    domain::{query::QueryLanguage, saved_view::SavedView},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const MAX_RANGE_SECS: u32 = 366 * 24 * 3600;

pub(super) async fn execute(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    match approval.action.as_str() {
        "create_saved_view" => create(state, ctx, approval).await,
        "update_saved_view" => update(state, ctx, approval).await,
        "delete_saved_view" => delete(state, ctx, approval).await,
        _ => unreachable!("saved-view operation received unrelated action"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedViewWrite {
    name: String,
    language: QueryLanguage,
    statement: String,
    time_range_secs: u32,
    #[serde(default)]
    stream: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    pinned: bool,
}

async fn create(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let parameters: SavedViewWrite = parse(&approval.parameters)?;
    validate(&parameters)?;
    let now = TimestampMicros::now();
    let view = state
        .platform
        .saved_view
        .create(SavedView {
            id: Id(approval.target.clone()),
            org_id: ctx.org_id.clone(),
            owner_user_id: approval.requested_by.clone(),
            name: parameters.name,
            language: parameters.language,
            statement: parameters.statement,
            time_range_secs: parameters.time_range_secs,
            stream: parameters.stream,
            tags: parameters.tags,
            pinned: parameters.pinned,
            created_at: now,
            updated_at: now,
        })
        .await?;
    outcome("saved view created", view)
}

async fn update(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let parameters: SavedViewWrite = parse(&approval.parameters)?;
    validate(&parameters)?;
    let existing = load_authorized(state, ctx, &approval.target, "saved_views.edit").await?;
    let view = state
        .platform
        .saved_view
        .update(SavedView {
            id: existing.id,
            org_id: existing.org_id,
            owner_user_id: existing.owner_user_id,
            name: parameters.name,
            language: parameters.language,
            statement: parameters.statement,
            time_range_secs: parameters.time_range_secs,
            stream: parameters.stream,
            tags: parameters.tags,
            pinned: parameters.pinned,
            created_at: existing.created_at,
            updated_at: TimestampMicros::now(),
        })
        .await?;
    outcome("saved view updated", view)
}

async fn delete(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let _: Empty = parse(&approval.parameters)?;
    let view = load_authorized(state, ctx, &approval.target, "saved_views.delete").await?;
    state
        .platform
        .saved_view
        .delete(&view.org_id, &view.id)
        .await?;
    Ok(OperationOutcome {
        summary: "saved view deleted".into(),
        verification: json!({"verified": true, "view_id": view.id, "deleted": true}),
    })
}

async fn load_authorized(
    state: &AppState,
    ctx: &IamContext,
    id: &str,
    permission: &str,
) -> Result<SavedView> {
    let view = state
        .platform
        .saved_view
        .get(&ctx.org_id, &Id(id.to_string()))
        .await?;
    Permission::require_resource(
        state,
        ctx,
        permission,
        &view.org_id,
        "saved_view",
        &view.id.0,
    )
    .await?;
    Ok(view)
}

fn validate(value: &SavedViewWrite) -> Result<()> {
    if value.name.trim().is_empty() || value.name.chars().count() > 255 {
        return Err(Error::invalid(
            "name must contain between 1 and 255 characters",
        ));
    }
    if value.statement.trim().is_empty() {
        return Err(Error::invalid("statement cannot be empty"));
    }
    if value
        .stream
        .as_ref()
        .is_some_and(|stream| stream.chars().count() > 255)
    {
        return Err(Error::invalid("stream must be at most 255 characters"));
    }
    if value.time_range_secs == 0 || value.time_range_secs > MAX_RANGE_SECS {
        return Err(Error::invalid(
            "time_range_secs must be between 1 and 31622400",
        ));
    }
    Ok(())
}

fn outcome(summary: &str, view: SavedView) -> Result<OperationOutcome> {
    Ok(OperationOutcome {
        summary: summary.into(),
        verification: json!({"verified": true, "saved_view": view}),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("invalid operation parameters: {error}")))
}
