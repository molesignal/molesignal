// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;

use super::{Dashboard, Folder};
use crate::shared::{Result, ids::Id};

#[async_trait]
pub trait DashboardRepository: Send + Sync {
    async fn create(&self, dashboard: Dashboard) -> Result<Dashboard>;
    async fn update(&self, dashboard: Dashboard) -> Result<Dashboard>;
    async fn get(&self, id: &Id) -> Result<Dashboard>;
    async fn get_by_uid(&self, org_id: &Id, uid: &str) -> Result<Dashboard>;
    async fn list(&self, org_id: &Id, folder_id: Option<&Id>) -> Result<Vec<Dashboard>>;
    async fn delete(&self, id: &Id) -> Result<()>;
}

#[async_trait]
pub trait FolderRepository: Send + Sync {
    async fn create(&self, folder: Folder) -> Result<Folder>;
    /// Load the canonical folder before resource-scoped IAM evaluation.
    async fn get_by_id(&self, id: &Id) -> Result<Folder>;
    /// 按 (org, id) 取单个文件夹；不存在（或不属于该 org）→ `Error::NotFound`。
    async fn get(&self, org_id: &Id, id: &Id) -> Result<Folder>;
    async fn list(&self, org_id: &Id) -> Result<Vec<Folder>>;
    /// 改名 / 移动（更新 name + parent_id）；行不存在 → `Error::NotFound`。
    async fn update(&self, folder: Folder) -> Result<Folder>;
    async fn delete(&self, id: &Id) -> Result<()>;
}
