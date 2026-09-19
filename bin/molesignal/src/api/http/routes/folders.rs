// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 仪表盘文件夹 CRUD（`/api/v1/folders`）。
//!
//! 之前文件夹只能从 dashboards 的 `folder_id` 反推（前端 "Manage folders" 仅做查看/
//! 过滤）；这里把真正的新建 / 改名 / 删除暴露成 REST 接口。权限复用 dashboard 读写。
//!
//! - `GET    /folders`        列出当前 org 全部文件夹
//! - `POST   /folders`        新建（name + 可选 parent_id）
//! - `PUT    /folders/{id}`   改名 / 移动（整体替换 name + parent_id）
//! - `DELETE /folders/{id}`   删除（含子文件夹 / 仪表盘时 409，避免悬挂引用）

use std::collections::HashMap;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, put},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{AppState, http::middleware::ProtectedResource},
    app::iam::IamContext,
    domain::{
        dashboard::Folder,
        iam::{permission, resource_permission},
    },
    shared::{Error, Result, ids::Id},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/folders", get(list).post(create))
        .route("/folders/{id}", put(update).delete(delete))
}

const MAX_NAME_LEN: usize = 255;
const MAX_FOLDER_LEVELS: usize = 3;

#[async_trait::async_trait]
impl ProtectedResource for Folder {
    type Id = Id;

    async fn load(state: &AppState, id: Self::Id) -> Result<Self> {
        state.dashboard.folders().get_by_id(&id).await
    }

    fn organization_id(&self) -> &Id {
        &self.org_id
    }

    fn resource_type(&self) -> &str {
        "folder"
    }

    fn resource_id(&self) -> &str {
        self.id.as_str()
    }
}

#[derive(Deserialize)]
struct CreateFolderReq {
    name: String,
    #[serde(default)]
    parent_id: Option<String>,
}

#[derive(Deserialize)]
struct UpdateFolderReq {
    name: String,
    #[serde(default)]
    parent_id: Option<String>,
}

#[permission(any("dashboards.read", "sys.dashboards.read"))]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<Folder>>> {
    Ok(Json(state.dashboard.folders().list(&ctx.org_id).await?))
}

#[permission("dashboards.create")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateFolderReq>,
) -> Result<Json<Folder>> {
    let name = clean_name(&req.name)?;
    let parent_id = normalize_parent(req.parent_id);
    if let Some(parent) = &parent_id {
        let all = state.dashboard.folders().list(&ctx.org_id).await?;
        ensure_folder_depth(&all, None, Some(parent))?;
    }
    let folder = Folder {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name,
        parent_id,
    };
    Ok(Json(state.dashboard.folders().create(folder).await?))
}

#[resource_permission(
    action = "dashboards.edit",
    resource = Folder,
    id = Id::from_string(id),
    bind = folder
)]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdateFolderReq>,
) -> Result<Json<Folder>> {
    let mut folder = folder;
    let id = folder.id.clone();
    let org_id = folder.org_id.clone();
    let name = clean_name(&req.name)?;
    let parent_id = normalize_parent(req.parent_id);
    let all = state.dashboard.folders().list(&org_id).await?;
    if let Some(parent) = &parent_id {
        if parent == &id {
            return Err(Error::invalid("folder cannot be its own parent"));
        }
        if !all.iter().any(|f| &f.id == parent) {
            return Err(Error::not_found(format!("parent folder {}", parent.0)));
        }
        if creates_cycle(&all, &id, parent) {
            return Err(Error::invalid("folder move would create a cycle"));
        }
    }
    ensure_folder_depth(&all, Some(&id), parent_id.as_ref())?;
    folder.name = name;
    folder.parent_id = parent_id;
    Ok(Json(state.dashboard.folders().update(folder).await?))
}

#[resource_permission(
    action = "dashboards.delete",
    resource = Folder,
    id = Id::from_string(id),
    bind = folder
)]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let id = folder.id;
    let org_id = folder.org_id;
    // 非空保护：有子文件夹或仪表盘时拒删，避免悬挂引用（folders / dashboards 表无外键级联）。
    let has_child_folder = state
        .dashboard
        .folders()
        .list(&org_id)
        .await?
        .iter()
        .any(|f| f.parent_id.as_ref() == Some(&id));
    let has_dashboards = !state.dashboard.list(&org_id, Some(&id)).await?.is_empty();
    if has_child_folder || has_dashboards {
        return Err(Error::conflict(
            "folder is not empty: move or delete its dashboards and sub-folders first",
        ));
    }
    state.dashboard.folders().delete(&id).await?;
    Ok(Json(json!({ "deleted": id.0 })))
}

fn clean_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(Error::invalid("folder name must not be empty"));
    }
    if trimmed.chars().count() > MAX_NAME_LEN {
        return Err(Error::invalid("folder name must be at most 255 characters"));
    }
    Ok(trimmed.to_string())
}

/// 空字符串 / 纯空白的 parent_id 当作 None（根目录）。
fn normalize_parent(parent: Option<String>) -> Option<Id> {
    parent
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(Id::from_string)
}

/// `proposed_parent` 是否等于 `folder_id` 或它的后代 —— 若是，把 folder 挂过去会成环。
/// 从 proposed_parent 沿 parent 链上溯，途经 folder_id 即成环。
fn creates_cycle(all: &[Folder], folder_id: &Id, proposed_parent: &Id) -> bool {
    let parent_of: HashMap<&str, Option<&Id>> = all
        .iter()
        .map(|f| (f.id.0.as_str(), f.parent_id.as_ref()))
        .collect();
    let mut cur = Some(proposed_parent);
    let mut steps = 0;
    while let Some(c) = cur {
        if c == folder_id {
            return true;
        }
        steps += 1;
        if steps > all.len() + 1 {
            // 防御：数据里已存在环时不至于死循环。
            return true;
        }
        cur = parent_of.get(c.0.as_str()).copied().flatten();
    }
    false
}

fn ensure_folder_depth(
    all: &[Folder],
    moving_folder_id: Option<&Id>,
    proposed_parent: Option<&Id>,
) -> Result<()> {
    let parent_depth = proposed_parent
        .map(|parent| folder_depth(all, parent))
        .transpose()?
        .unwrap_or(0);
    let subtree_height = moving_folder_id
        .map(|folder_id| folder_subtree_height(all, folder_id))
        .transpose()?
        .unwrap_or(1);
    if parent_depth + subtree_height > MAX_FOLDER_LEVELS {
        return Err(Error::invalid("folders support at most 3 levels"));
    }
    Ok(())
}

fn folder_depth(all: &[Folder], folder_id: &Id) -> Result<usize> {
    let parent_of: HashMap<&str, Option<&Id>> = all
        .iter()
        .map(|folder| (folder.id.as_str(), folder.parent_id.as_ref()))
        .collect();
    let mut current = Some(folder_id);
    let mut depth = 0;
    while let Some(folder) = current {
        depth += 1;
        if depth > all.len() + 1 {
            return Err(Error::invalid("folder hierarchy contains a cycle"));
        }
        current = *parent_of
            .get(folder.as_str())
            .ok_or_else(|| Error::not_found(format!("parent folder {}", folder.0)))?;
    }
    Ok(depth)
}

fn folder_subtree_height(all: &[Folder], folder_id: &Id) -> Result<usize> {
    let parent_of: HashMap<&str, Option<&Id>> = all
        .iter()
        .map(|folder| (folder.id.as_str(), folder.parent_id.as_ref()))
        .collect();
    let mut height = 1;
    for candidate in all {
        if candidate.id == *folder_id {
            continue;
        }
        let mut current = &candidate.id;
        let mut distance = 0;
        loop {
            distance += 1;
            if distance > all.len() + 1 {
                return Err(Error::invalid("folder hierarchy contains a cycle"));
            }
            let Some(parent) = parent_of.get(current.as_str()).copied().flatten() else {
                break;
            };
            if parent == folder_id {
                height = height.max(distance + 1);
                break;
            }
            current = parent;
        }
    }
    Ok(height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(id: &str, parent_id: Option<&str>) -> Folder {
        Folder {
            id: Id::from_string(id),
            org_id: Id::from_string("org"),
            name: id.to_string(),
            parent_id: parent_id.map(Id::from_string),
        }
    }

    #[test]
    fn folder_depth_accepts_three_levels_and_rejects_a_fourth() {
        let all = vec![
            folder("root", None),
            folder("child", Some("root")),
            folder("grandchild", Some("child")),
        ];

        assert!(ensure_folder_depth(&all, None, Some(&Id::from_string("child"))).is_ok());
        assert!(matches!(
            ensure_folder_depth(&all, None, Some(&Id::from_string("grandchild"))),
            Err(Error::InvalidArgument(_))
        ));
    }

    #[test]
    fn moving_a_subtree_cannot_push_descendants_below_level_three() {
        let all = vec![
            folder("root", None),
            folder("child", Some("root")),
            folder("grandchild", Some("child")),
            folder("destination", Some("root")),
        ];

        assert!(ensure_folder_depth(&all, Some(&Id::from_string("child")), None).is_ok());
        assert!(matches!(
            ensure_folder_depth(
                &all,
                Some(&Id::from_string("child")),
                Some(&Id::from_string("destination")),
            ),
            Err(Error::InvalidArgument(_))
        ));
    }
}
