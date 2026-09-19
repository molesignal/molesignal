// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 仪表盘上下文。
//!
//! `model` 保存 MoleSignal Dashboard Engine 的当前 JSON Schema。

pub mod authoring;
pub mod contract_registry;
pub mod repositories;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::shared::{ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dashboard {
    pub id: Id,
    pub org_id: Id,
    /// 文件夹 ID，用于 UI 分类
    pub folder_id: Option<Id>,
    /// 不可变的对外标识
    pub uid: String,
    pub title: String,
    pub tags: Vec<String>,
    /// 完整的 MoleSignal Dashboard Engine JSON。
    pub model: Value,
    pub version: u32,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
    pub created_by: Id,
    pub updated_by: Id,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub parent_id: Option<Id>,
}
