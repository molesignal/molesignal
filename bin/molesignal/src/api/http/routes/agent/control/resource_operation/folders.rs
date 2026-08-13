// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::{Value, json};

use super::super::OperationOutcome;
use crate::{
    agent::model::ApprovalRequest,
    api::{AppState, http::middleware::Permission},
    app::iam::IamContext,
    domain::dashboard::Folder,
    shared::{Error, Result, ids::Id},
};

const MAX_FOLDER_LEVELS: usize = 3;

pub(super) async fn execute(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    match approval.action.as_str() {
        "create_folder" => create(state, ctx, approval).await,
        "update_folder" => update(state, ctx, approval).await,
        "delete_folder" => delete(state, ctx, approval).await,
        _ => unreachable!("folder operation received unrelated action"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FolderWrite {
    name: String,
    #[serde(default)]
    parent_id: Option<String>,
}

async fn create(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let parameters: FolderWrite = parse(&approval.parameters)?;
    let parent_id = optional_id(parameters.parent_id);
    let all = state.dashboard.folders().list(&ctx.org_id).await?;
    ensure_depth(&all, None, parent_id.as_ref())?;
    let folder = state
        .dashboard
        .folders()
        .create(Folder {
            id: Id(approval.target.clone()),
            org_id: ctx.org_id.clone(),
            name: clean_name(&parameters.name)?,
            parent_id,
        })
        .await?;
    outcome("folder created", folder)
}

async fn update(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let parameters: FolderWrite = parse(&approval.parameters)?;
    let mut folder = load_authorized(state, ctx, &approval.target, "dashboards.edit").await?;
    let parent_id = optional_id(parameters.parent_id);
    let all = state.dashboard.folders().list(&folder.org_id).await?;
    if let Some(parent_id) = &parent_id {
        if parent_id == &folder.id {
            return Err(Error::invalid("folder cannot be its own parent"));
        }
        if creates_cycle(&all, &folder.id, parent_id) {
            return Err(Error::invalid("folder move would create a cycle"));
        }
    }
    ensure_depth(&all, Some(&folder.id), parent_id.as_ref())?;
    folder.name = clean_name(&parameters.name)?;
    folder.parent_id = parent_id;
    let folder = state.dashboard.folders().update(folder).await?;
    outcome("folder updated", folder)
}

async fn delete(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let _: Empty = parse(&approval.parameters)?;
    let folder = load_authorized(state, ctx, &approval.target, "dashboards.delete").await?;
    let all = state.dashboard.folders().list(&folder.org_id).await?;
    let has_child = all
        .iter()
        .any(|candidate| candidate.parent_id.as_ref() == Some(&folder.id));
    let has_dashboards = !state
        .dashboard
        .list(&folder.org_id, Some(&folder.id))
        .await?
        .is_empty();
    if has_child || has_dashboards {
        return Err(Error::conflict(
            "folder is not empty: move or delete its dashboards and sub-folders first",
        ));
    }
    state.dashboard.folders().delete(&folder.id).await?;
    Ok(OperationOutcome {
        summary: "folder deleted".into(),
        verification: json!({"verified": true, "folder_id": folder.id, "deleted": true}),
    })
}

async fn load_authorized(
    state: &AppState,
    ctx: &IamContext,
    id: &str,
    permission: &str,
) -> Result<Folder> {
    let folder = state
        .dashboard
        .folders()
        .get(&ctx.org_id, &Id(id.to_string()))
        .await?;
    Permission::require_resource(
        state,
        ctx,
        permission,
        &folder.org_id,
        "folder",
        &folder.id.0,
    )
    .await?;
    Ok(folder)
}

fn clean_name(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 255 {
        Err(Error::invalid(
            "folder name must contain between 1 and 255 characters",
        ))
    } else {
        Ok(value.to_string())
    }
}

fn optional_id(value: Option<String>) -> Option<Id> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(Id)
}

fn creates_cycle(all: &[Folder], folder_id: &Id, proposed_parent: &Id) -> bool {
    let parents = all
        .iter()
        .map(|folder| (folder.id.as_str(), folder.parent_id.as_ref()))
        .collect::<HashMap<_, _>>();
    let mut current = Some(proposed_parent);
    for _ in 0..=all.len() {
        let Some(candidate) = current else {
            return false;
        };
        if candidate == folder_id {
            return true;
        }
        current = parents.get(candidate.as_str()).copied().flatten();
    }
    true
}

fn ensure_depth(
    all: &[Folder],
    moving_id: Option<&Id>,
    proposed_parent: Option<&Id>,
) -> Result<()> {
    let parent_depth = proposed_parent
        .map(|parent| depth(all, parent))
        .transpose()?
        .unwrap_or(0);
    let subtree_height = moving_id
        .map(|id| subtree_height(all, id))
        .transpose()?
        .unwrap_or(1);
    if parent_depth + subtree_height > MAX_FOLDER_LEVELS {
        Err(Error::invalid("folders support at most 3 levels"))
    } else {
        Ok(())
    }
}

fn depth(all: &[Folder], folder_id: &Id) -> Result<usize> {
    let parents = all
        .iter()
        .map(|folder| (folder.id.as_str(), folder.parent_id.as_ref()))
        .collect::<HashMap<_, _>>();
    let mut current = Some(folder_id);
    let mut depth = 0;
    while let Some(candidate) = current {
        depth += 1;
        if depth > all.len() + 1 {
            return Err(Error::invalid("folder hierarchy contains a cycle"));
        }
        current = *parents
            .get(candidate.as_str())
            .ok_or_else(|| Error::not_found(format!("parent folder {}", candidate.0)))?;
    }
    Ok(depth)
}

fn subtree_height(all: &[Folder], folder_id: &Id) -> Result<usize> {
    let parents = all
        .iter()
        .map(|folder| (folder.id.as_str(), folder.parent_id.as_ref()))
        .collect::<HashMap<_, _>>();
    let mut height = 1;
    for candidate in all.iter().filter(|candidate| candidate.id != *folder_id) {
        let mut current = &candidate.id;
        for distance in 1..=all.len() + 1 {
            let Some(parent) = parents.get(current.as_str()).copied().flatten() else {
                break;
            };
            if parent == folder_id {
                height = height.max(distance + 1);
                break;
            }
            current = parent;
            if distance > all.len() {
                return Err(Error::invalid("folder hierarchy contains a cycle"));
            }
        }
    }
    Ok(height)
}

fn outcome(summary: &str, folder: Folder) -> Result<OperationOutcome> {
    Ok(OperationOutcome {
        summary: summary.into(),
        verification: json!({"verified": true, "folder": folder}),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("invalid operation parameters: {error}")))
}
