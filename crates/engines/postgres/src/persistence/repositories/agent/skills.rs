// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `agent_skills`，以及 `org_settings` 里组织对实例级 Skill 的启停（`agent.skills` 一节：
//! `skill_key → {enabled, updated_by, updated_at}`，没有记录的为启用）。拉取 Skill 的仓库在 [`repositories`]。
//!
//! 作用域存成三列加一个 `scope_key`（`org`、`team:<id>`、`personal:<id>`），同一作用域里名称
//! 不分大小写唯一。

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row, postgres::PgRow, types::Json};

use super::super::{
    org_settings::{self, sections::AGENT_SKILLS},
    sqlx_err,
};
use crate::{
    agent::{
        scope::Scope,
        skills::{
            FileEntry, RepositoryStatus, SkillManifest, SkillRepository, SkillSource, SkillStore,
            StoredSkill, TokenChange,
        },
    },
    infra::cipher::CipherRootKey,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod repositories;

pub struct PgSkillStore {
    pool: PgPool,
    cipher: CipherRootKey,
}

/// `agent.skills` 里一个 Skill 的启停。
#[derive(Serialize, Deserialize)]
struct StoredSkillSetting {
    enabled: bool,
    updated_by: String,
    updated_at: TimestampMicros,
}

impl PgSkillStore {
    pub fn new(pool: PgPool, cipher: CipherRootKey) -> Self {
        Self { pool, cipher }
    }
}

const SKILL_COLS: &str = "id, org_id, source, repository_id, repository_dir, scope, team_id, \
    owner_id, name, description, version, manifest, files, content_hash, object_prefix, \
    package_object_key, package_name, enabled, created_by, updated_by, created_at, \
    updated_at";

fn scope_of(row: &PgRow) -> Result<Scope> {
    let kind: String = row.try_get("scope").map_err(sqlx_err)?;
    let team_id: Option<String> = row.try_get("team_id").map_err(sqlx_err)?;
    let owner_id: Option<String> = row.try_get("owner_id").map_err(sqlx_err)?;

    Scope::from_parts(&kind, team_id.as_deref(), owner_id.as_deref())
}

fn source_of(value: &str) -> Result<SkillSource> {
    match value {
        "upload" => Ok(SkillSource::Upload),
        "repository" => Ok(SkillSource::Repository),
        other => Err(Error::internal(format!(
            "unknown stored skill source `{other}`"
        ))),
    }
}

fn skill_row(row: PgRow) -> Result<StoredSkill> {
    let source: String = row.try_get("source").map_err(sqlx_err)?;
    let manifest: Json<SkillManifest> = row.try_get("manifest").map_err(sqlx_err)?;
    let files: Json<Vec<FileEntry>> = row.try_get("files").map_err(sqlx_err)?;

    Ok(StoredSkill {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        source: source_of(&source)?,
        repository_id: row
            .try_get::<Option<String>, _>("repository_id")
            .map_err(sqlx_err)?
            .map(Id),
        repository_dir: row.try_get("repository_dir").map_err(sqlx_err)?,
        scope: scope_of(&row)?,
        name: row.try_get("name").map_err(sqlx_err)?,
        description: row.try_get("description").map_err(sqlx_err)?,
        version: row.try_get("version").map_err(sqlx_err)?,
        manifest: manifest.0,
        files: files.0,
        content_hash: row.try_get("content_hash").map_err(sqlx_err)?,
        object_prefix: row.try_get("object_prefix").map_err(sqlx_err)?,
        package_object_key: row.try_get("package_object_key").map_err(sqlx_err)?,
        package_name: row.try_get("package_name").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        created_by: Id(row.try_get("created_by").map_err(sqlx_err)?),
        updated_by: Id(row.try_get("updated_by").map_err(sqlx_err)?),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
    })
}

/// 唯一约束冲突说成名称已被占用。
fn name_conflict(error: sqlx::Error, name: &str) -> Error {
    match &error {
        sqlx::Error::Database(database) if database.code().as_deref() == Some("23505") => {
            Error::conflict(format!(
                "a skill named `{name}` already exists in this scope"
            ))
        }
        _ => sqlx_err(error),
    }
}

#[async_trait]
impl SkillStore for PgSkillStore {
    async fn list(&self, org_id: &Id) -> Result<Vec<StoredSkill>> {
        let rows = sqlx::query(&format!(
            "SELECT {SKILL_COLS} FROM agent_skills WHERE org_id = $1 ORDER BY lower(name), id"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(skill_row).collect()
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<StoredSkill> {
        let row = sqlx::query(&format!(
            "SELECT {SKILL_COLS} FROM agent_skills WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("skill `{}` not found", id.0)))?;

        skill_row(row)
    }

    async fn find_by_name(
        &self,
        org_id: &Id,
        scope: &Scope,
        name: &str,
    ) -> Result<Option<StoredSkill>> {
        sqlx::query(&format!(
            "SELECT {SKILL_COLS} FROM agent_skills
             WHERE org_id = $1 AND scope_key = $2 AND lower(name) = lower($3)"
        ))
        .bind(&org_id.0)
        .bind(scope.key())
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(skill_row)
        .transpose()
    }

    async fn save(&self, skill: StoredSkill) -> Result<StoredSkill> {
        let row = sqlx::query(&format!(
            "INSERT INTO agent_skills
                (id, org_id, source, repository_id, repository_dir, scope, team_id, owner_id,
                 scope_key, name, description, version, manifest, files, content_hash,
                 object_prefix, package_object_key, package_name, enabled, created_by,
                 updated_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16,
                     $17, $18, $19, $20, $21, $22, $23)
             ON CONFLICT (id) DO UPDATE SET
                scope = EXCLUDED.scope, team_id = EXCLUDED.team_id,
                owner_id = EXCLUDED.owner_id, scope_key = EXCLUDED.scope_key,
                name = EXCLUDED.name, description = EXCLUDED.description,
                version = EXCLUDED.version, manifest = EXCLUDED.manifest,
                files = EXCLUDED.files, content_hash = EXCLUDED.content_hash,
                object_prefix = EXCLUDED.object_prefix,
                package_object_key = EXCLUDED.package_object_key,
                package_name = EXCLUDED.package_name, enabled = EXCLUDED.enabled,
                repository_dir = EXCLUDED.repository_dir, updated_by = EXCLUDED.updated_by,
                updated_at = EXCLUDED.updated_at
             RETURNING {SKILL_COLS}"
        ))
        .bind(&skill.id.0)
        .bind(&skill.org_id.0)
        .bind(skill.source.as_str())
        .bind(skill.repository_id.as_ref().map(|id| id.0.as_str()))
        .bind(&skill.repository_dir)
        .bind(skill.scope.kind())
        .bind(skill.scope.team_id().map(|id| id.0.as_str()))
        .bind(skill.scope.owner_id().map(|id| id.0.as_str()))
        .bind(skill.scope.key())
        .bind(&skill.name)
        .bind(&skill.description)
        .bind(&skill.version)
        .bind(Json(&skill.manifest))
        .bind(Json(&skill.files))
        .bind(&skill.content_hash)
        .bind(&skill.object_prefix)
        .bind(&skill.package_object_key)
        .bind(&skill.package_name)
        .bind(skill.enabled)
        .bind(&skill.created_by.0)
        .bind(&skill.updated_by.0)
        .bind(skill.created_at)
        .bind(skill.updated_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| name_conflict(error, &skill.name))?;

        skill_row(row)
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<bool> {
        let result = sqlx::query("DELETE FROM agent_skills WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(result.rows_affected() > 0)
    }

    async fn instance_settings(&self, org_id: &Id) -> Result<HashMap<String, bool>> {
        let section = org_settings::read::<HashMap<String, StoredSkillSetting>>(
            &self.pool,
            &org_id.0,
            AGENT_SKILLS,
        )
        .await?;

        Ok(section
            .map(|section| section.value)
            .unwrap_or_default()
            .into_iter()
            .map(|(key, setting)| (key, setting.enabled))
            .collect())
    }

    async fn set_instance_enabled(
        &self,
        org_id: &Id,
        key: &str,
        enabled: bool,
        by: &Id,
    ) -> Result<()> {
        let now = TimestampMicros::now();

        let setting = StoredSkillSetting {
            enabled,
            updated_by: by.0.clone(),
            updated_at: now,
        };

        // 只换掉这一个 Skill 的那一项，别的 Skill 的启停不动。
        sqlx::query(
            "INSERT INTO org_settings (org_id, section, value, updated_by, updated_at)
             VALUES ($1, $2, jsonb_build_object($3::TEXT, $4::JSONB), $5, $6)
             ON CONFLICT (org_id, section) DO UPDATE SET
                value = org_settings.value || EXCLUDED.value,
                updated_by = EXCLUDED.updated_by,
                updated_at = EXCLUDED.updated_at",
        )
        .bind(&org_id.0)
        .bind(AGENT_SKILLS)
        .bind(key)
        .bind(Json(&setting))
        .bind(&by.0)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn list_repositories(&self, org_id: &Id) -> Result<Vec<SkillRepository>> {
        self.repositories_in(org_id).await
    }

    async fn get_repository(&self, org_id: &Id, id: &Id) -> Result<SkillRepository> {
        self.repository(org_id, id).await
    }

    async fn save_repository(
        &self,
        repository: SkillRepository,
        token: TokenChange,
    ) -> Result<SkillRepository> {
        self.put_repository(repository, token).await
    }

    async fn repository_token(&self, org_id: &Id, id: &Id) -> Result<Option<String>> {
        self.open_repository_token(org_id, id).await
    }

    async fn record_sync(
        &self,
        org_id: &Id,
        id: &Id,
        status: RepositoryStatus,
        error: Option<&str>,
        commit: Option<&str>,
    ) -> Result<()> {
        self.record_repository_sync(org_id, id, status, error, commit)
            .await
    }

    async fn delete_repository(&self, org_id: &Id, id: &Id) -> Result<bool> {
        self.remove_repository(org_id, id).await
    }

    async fn repositories_due(&self, before: TimestampMicros) -> Result<Vec<SkillRepository>> {
        self.due_repositories(before).await
    }
}
