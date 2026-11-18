// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Skill storage: uploaded and repository-sourced Skills, organization-level enablement of
//! instance-level Skills (built-in and instance directories), Skill repositories, and their access
//! tokens (stored encrypted, with only the last four characters shown in lists). The files
//! themselves are in object storage.

use std::collections::HashMap;

use async_trait::async_trait;
use kernel::{Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};

use super::{FileEntry, RepoHost, SkillManifest};
use crate::scope::Scope;

/// Where a Skill comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    /// Ships with the product.
    Builtin,
    /// A server directory configured for the instance (`[agent] skill_paths`).
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

    /// Instance-level: visible to all organizations and read-only; organizations can only enable or
    /// disable it.
    pub fn instance_level(self) -> bool {
        matches!(self, Self::Builtin | Self::Path)
    }
}

/// A Skill in the database (uploaded or repository-sourced).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSkill {
    pub id: Id,
    pub org_id: Id,
    pub source: SkillSource,
    pub repository_id: Option<Id>,
    /// The directory of this Skill in the repository.
    pub repository_dir: Option<String>,
    pub scope: Scope,
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub manifest: SkillManifest,
    pub files: Vec<FileEntry>,
    pub content_hash: String,
    /// The file prefix in object storage (includes a hash; changes with the version).
    pub object_prefix: String,
    /// The original uploaded package.
    pub package_object_key: Option<String>,
    pub package_name: Option<String>,
    pub enabled: bool,
    pub created_by: Id,
    pub updated_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// The synchronization result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryStatus {
    /// Has not been synchronized yet.
    Pending,
    Ok,
    /// Some Skills are not imported (duplicate names or invalid formats); the rest are fine.
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

/// A Skill repository.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRepository {
    pub id: Id,
    pub org_id: Id,
    pub scope: Scope,
    pub url: String,
    pub host: RepoHost,
    /// The archive URL when the hosting method is `archive`, containing `{ref}`.
    pub archive_url: Option<String>,
    /// The branch or tag; empty means the default branch.
    pub git_ref: String,
    /// The directory containing Skills in the repository; empty means the entire repository.
    pub dir: String,
    /// Synchronizes automatically once a day.
    pub auto_sync: bool,
    pub token_set: bool,
    pub token_last4: Option<String>,
    pub status: RepositoryStatus,
    /// The failure reason, or Skills that are not imported and their reasons (one per line).
    pub last_error: Option<String>,
    pub last_commit: Option<String>,
    pub last_synced_at: Option<TimestampMicros>,
    pub created_by: Id,
    pub updated_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

/// Token writes: leave unchanged, replace with a new value, or clear.
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
    /// Finds a matching name in the same scope (case-insensitive).
    async fn find_by_name(
        &self,
        org_id: &Id,
        scope: &Scope,
        name: &str,
    ) -> Result<Option<StoredSkill>>;
    /// Creates or updates by ID; reports a conflict for duplicate names in the same scope.
    async fn save(&self, skill: StoredSkill) -> Result<StoredSkill>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<bool>;

    /// Organization-level enablement of instance-level Skills: keys are `builtin:<name>` or
    /// `path:<name>`; enabled if no record exists.
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
    /// Deletes the repository and all Skills it imports.
    async fn delete_repository(&self, org_id: &Id, id: &Id) -> Result<bool>;
    /// Repositories across all organizations with automatic synchronization enabled and their
    /// latest synchronization before `before`.
    async fn repositories_due(&self, before: TimestampMicros) -> Result<Vec<SkillRepository>>;
}
