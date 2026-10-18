// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Skill 的存储：上传与仓库来源的 Skill、组织对实例级 Skill（内置、实例目录）的启停、Skill
//! 仓库与它的访问令牌（加密保存，列表里只有末四位）。文件本身在对象存储里。

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{FileEntry, RepoHost, SkillManifest};
use crate::{
    scope::Scope,
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 一个 Skill 从哪来。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    /// 随产品发布。
    Builtin,
    /// 实例配置的服务器目录（`[agent] skill_paths`）。
    Path,
    Upload,
    Repository,
}

impl SkillSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Path => "path",
            Self::Upload => "upload",
            Self::Repository => "repository",
        }
    }

    /// 实例级：所有组织可见、只读，组织只能启停。
    pub fn instance_level(self) -> bool {
        matches!(self, Self::Builtin | Self::Path)
    }
}

/// 数据库里的一个 Skill（上传或仓库来源）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredSkill {
    pub id: Id,
    pub org_id: Id,
    pub source: SkillSource,
    pub repository_id: Option<Id>,
    /// 仓库里这个 Skill 的目录。
    pub repository_dir: Option<String>,
    pub scope: Scope,
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub manifest: SkillManifest,
    pub files: Vec<FileEntry>,
    pub content_hash: String,
    /// 对象存储里文件的前缀（含 hash，换版本时换前缀）。
    pub object_prefix: String,
    /// 上传的原包。
    pub package_object_key: Option<String>,
    pub package_name: Option<String>,
    pub enabled: bool,
    pub created_by: Id,
    pub updated_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// 同步的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryStatus {
    /// 还没同步过。
    Pending,
    Ok,
    /// 有的 Skill 没导入（重名、格式不对），其余正常。
    Partial,
    Failed,
}

impl RepositoryStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ok => "ok",
            Self::Partial => "partial",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "ok" => Self::Ok,
            "partial" => Self::Partial,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

/// 一个 Skill 仓库。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillRepository {
    pub id: Id,
    pub org_id: Id,
    pub scope: Scope,
    pub url: String,
    pub host: RepoHost,
    /// 托管方式为 `archive` 时的压缩包地址，含 `{ref}`。
    pub archive_url: Option<String>,
    /// 分支或标签；空是默认分支。
    pub git_ref: String,
    /// 仓库里放 Skill 的目录；空是整个仓库。
    pub dir: String,
    /// 每天自动同步一次。
    pub auto_sync: bool,
    pub token_set: bool,
    pub token_last4: Option<String>,
    pub status: RepositoryStatus,
    /// 失败原因，或没导入的 Skill 与原因（一行一个）。
    pub last_error: Option<String>,
    pub last_commit: Option<String>,
    pub last_synced_at: Option<TimestampMicros>,
    pub created_by: Id,
    pub updated_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// 写令牌：不改、换成新值、清除。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenChange {
    Keep,
    Set(String),
    Clear,
}

#[async_trait]
pub trait SkillStore: Send + Sync {
    async fn list(&self, org_id: &Id) -> Result<Vec<StoredSkill>>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<StoredSkill>;
    /// 同一作用域里同名的（不分大小写）。
    async fn find_by_name(
        &self,
        org_id: &Id,
        scope: &Scope,
        name: &str,
    ) -> Result<Option<StoredSkill>>;
    /// 新建或按 ID 更新；同一作用域里重名时报冲突。
    async fn save(&self, skill: StoredSkill) -> Result<StoredSkill>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<bool>;

    /// 组织对实例级 Skill 的启停：键是 `builtin:<名称>` 或 `path:<名称>`，没有记录的为启用。
    async fn instance_settings(&self, org_id: &Id) -> Result<HashMap<String, bool>>;
    async fn set_instance_enabled(
        &self,
        org_id: &Id,
        key: &str,
        enabled: bool,
        by: &Id,
    ) -> Result<()>;

    async fn list_repositories(&self, org_id: &Id) -> Result<Vec<SkillRepository>>;
    async fn get_repository(&self, org_id: &Id, id: &Id) -> Result<SkillRepository>;
    async fn save_repository(
        &self,
        repository: SkillRepository,
        token: TokenChange,
    ) -> Result<SkillRepository>;
    async fn repository_token(&self, org_id: &Id, id: &Id) -> Result<Option<String>>;
    async fn record_sync(
        &self,
        org_id: &Id,
        id: &Id,
        status: RepositoryStatus,
        error: Option<&str>,
        commit: Option<&str>,
    ) -> Result<()>;
    /// 删除仓库与它导入的全部 Skill。
    async fn delete_repository(&self, org_id: &Id, id: &Id) -> Result<bool>;
    /// 所有组织里开了自动同步、最近同步早于 `before` 的仓库。
    async fn repositories_due(&self, before: TimestampMicros) -> Result<Vec<SkillRepository>>;
}
