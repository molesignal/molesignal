// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Saved view bounded context：保存的 SQL/PromQL 查询。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    domain::query::QueryLanguage,
    shared::{Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedView {
    pub id: Id,
    pub org_id: Id,
    pub owner_user_id: Id,
    pub name: String,
    pub language: QueryLanguage,
    pub statement: String,
    pub time_range_secs: u32,
    pub stream: Option<String>,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait SavedViewRepository: Send + Sync {
    async fn create(&self, v: SavedView) -> Result<SavedView>;
    async fn update(&self, v: SavedView) -> Result<SavedView>;
    async fn get_by_id(&self, id: &Id) -> Result<SavedView>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<SavedView>;
    async fn list(&self, org_id: &Id, pinned_only: bool) -> Result<Vec<SavedView>>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;
}
