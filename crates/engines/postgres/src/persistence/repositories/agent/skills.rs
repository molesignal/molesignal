// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `agent_skills`、`agent_skill_org_settings`、`agent_skill_repositories` 与仓库令牌表。
//!
//! 作用域存成三列加一个 `scope_key`（`org`、`team:<id>`、`personal:<id>`），同一作用域里名称
//! 不分大小写唯一。仓库令牌用 [`CipherRootKey`] seal，读取列表与详情只有 `token_set` 与末四位。

use std::collections::HashMap;

use async_trait::async_trait;
use sqlx::{PgPool, Row, postgres::PgRow, types::Json};

use super::super::sqlx_err;
use crate::{
    agent::{
        scope::Scope,
        skills::{
            FileEntry, RepoHost, RepositoryStatus, SkillManifest, SkillRepository, SkillSource,
            SkillStore, StoredSkill, TokenChange,
        },
    },
    infra::cipher::CipherRootKey,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgSkillStore {
    pool: PgPool,
    cipher: CipherRootKey,
}

impl PgSkillStore {
    pub fn new(pool: PgPool, cipher: CipherRootKey) -> Self {
        Self { pool, cipher }
    }
}

const SKILL_COLS: &str = "id, org_id, source, repository_id, repository_dir, scope, team_id, \
    owner_id, name, description, version, manifest, files, content_hash, object_prefix, \
    package_object_key, package_name, enabled, created_by, updated_by, created_at_micros, \
    updated_at_micros";

const REPOSITORY_COLS: &str = "id, org_id, scope, team_id, owner_id, url, host, archive_url, \
    git_ref, dir, auto_sync, token_set, token_last4, status, last_error, last_commit, \
    last_synced_at_micros, created_by, updated_by, created_at_micros, updated_at_micros";

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
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

fn repository_row(row: PgRow) -> Result<SkillRepository> {
    let host: String = row.try_get("host").map_err(sqlx_err)?;
    let status: String = row.try_get("status").map_err(sqlx_err)?;

    Ok(SkillRepository {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        scope: scope_of(&row)?,
        url: row.try_get("url").map_err(sqlx_err)?,
        host: RepoHost::parse(&host)?,
        archive_url: row.try_get("archive_url").map_err(sqlx_err)?,
        git_ref: row.try_get("git_ref").map_err(sqlx_err)?,
        dir: row.try_get("dir").map_err(sqlx_err)?,
        auto_sync: row.try_get("auto_sync").map_err(sqlx_err)?,
        token_set: row.try_get("token_set").map_err(sqlx_err)?,
        token_last4: row.try_get("token_last4").map_err(sqlx_err)?,
        status: RepositoryStatus::parse(&status),
        last_error: row.try_get("last_error").map_err(sqlx_err)?,
        last_commit: row.try_get("last_commit").map_err(sqlx_err)?,
        last_synced_at: row
            .try_get::<Option<i64>, _>("last_synced_at_micros")
            .map_err(sqlx_err)?
            .map(TimestampMicros),
        created_by: Id(row.try_get("created_by").map_err(sqlx_err)?),
        updated_by: Id(row.try_get("updated_by").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
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

fn last4(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    chars[chars.len().saturating_sub(4)..].iter().collect()
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
                 updated_by, created_at_micros, updated_at_micros)
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
                updated_at_micros = EXCLUDED.updated_at_micros
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
        .bind(skill.created_at.0)
        .bind(skill.updated_at.0)
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
        let rows = sqlx::query(
            "SELECT skill_key, enabled FROM agent_skill_org_settings WHERE org_id = $1",
        )
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                Ok((
                    row.try_get("skill_key").map_err(sqlx_err)?,
                    row.try_get("enabled").map_err(sqlx_err)?,
                ))
            })
            .collect()
    }

    async fn set_instance_enabled(
        &self,
        org_id: &Id,
        key: &str,
        enabled: bool,
        by: &Id,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO agent_skill_org_settings
                (org_id, skill_key, enabled, updated_by, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (org_id, skill_key) DO UPDATE SET
                enabled = EXCLUDED.enabled, updated_by = EXCLUDED.updated_by,
                updated_at_micros = EXCLUDED.updated_at_micros",
        )
        .bind(&org_id.0)
        .bind(key)
        .bind(enabled)
        .bind(&by.0)
        .bind(TimestampMicros::now().0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn list_repositories(&self, org_id: &Id) -> Result<Vec<SkillRepository>> {
        let rows = sqlx::query(&format!(
            "SELECT {REPOSITORY_COLS} FROM agent_skill_repositories
             WHERE org_id = $1 ORDER BY created_at_micros"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(repository_row).collect()
    }

    async fn get_repository(&self, org_id: &Id, id: &Id) -> Result<SkillRepository> {
        let row = sqlx::query(&format!(
            "SELECT {REPOSITORY_COLS} FROM agent_skill_repositories
             WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("skill repository `{}` not found", id.0)))?;

        repository_row(row)
    }

    async fn save_repository(
        &self,
        repository: SkillRepository,
        token: TokenChange,
    ) -> Result<SkillRepository> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let (token_set, token_last4) = match &token {
            TokenChange::Keep => (repository.token_set, repository.token_last4.clone()),
            TokenChange::Set(value) => (true, Some(last4(value))),
            TokenChange::Clear => (false, None),
        };

        let row = sqlx::query(&format!(
            "INSERT INTO agent_skill_repositories
                (id, org_id, scope, team_id, owner_id, scope_key, url, host, archive_url,
                 git_ref, dir, auto_sync, token_set, token_last4, status, last_error,
                 last_commit, last_synced_at_micros, created_by, updated_by, created_at_micros,
                 updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16,
                     $17, $18, $19, $20, $21, $22)
             ON CONFLICT (id) DO UPDATE SET
                scope = EXCLUDED.scope, team_id = EXCLUDED.team_id,
                owner_id = EXCLUDED.owner_id, scope_key = EXCLUDED.scope_key,
                url = EXCLUDED.url, host = EXCLUDED.host, archive_url = EXCLUDED.archive_url,
                git_ref = EXCLUDED.git_ref, dir = EXCLUDED.dir, auto_sync = EXCLUDED.auto_sync,
                token_set = EXCLUDED.token_set, token_last4 = EXCLUDED.token_last4,
                updated_by = EXCLUDED.updated_by, updated_at_micros = EXCLUDED.updated_at_micros
             RETURNING {REPOSITORY_COLS}"
        ))
        .bind(&repository.id.0)
        .bind(&repository.org_id.0)
        .bind(repository.scope.kind())
        .bind(repository.scope.team_id().map(|id| id.0.as_str()))
        .bind(repository.scope.owner_id().map(|id| id.0.as_str()))
        .bind(repository.scope.key())
        .bind(&repository.url)
        .bind(repository.host.as_str())
        .bind(&repository.archive_url)
        .bind(&repository.git_ref)
        .bind(&repository.dir)
        .bind(repository.auto_sync)
        .bind(token_set)
        .bind(&token_last4)
        .bind(repository.status.as_str())
        .bind(&repository.last_error)
        .bind(&repository.last_commit)
        .bind(repository.last_synced_at.map(|at| at.0))
        .bind(&repository.created_by.0)
        .bind(&repository.updated_by.0)
        .bind(repository.created_at.0)
        .bind(repository.updated_at.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        match token {
            TokenChange::Keep => {}
            TokenChange::Set(value) => {
                let (nonce, ciphertext) = self
                    .cipher
                    .seal(value.as_bytes())
                    .map_err(|error| Error::internal(format!("repository token seal: {error}")))?;

                sqlx::query(
                    "INSERT INTO agent_skill_repository_secrets
                        (repository_id, org_id, ciphertext, nonce, updated_at_micros)
                     VALUES ($1, $2, $3, $4, $5)
                     ON CONFLICT (repository_id) DO UPDATE SET
                        ciphertext = EXCLUDED.ciphertext, nonce = EXCLUDED.nonce,
                        updated_at_micros = EXCLUDED.updated_at_micros",
                )
                .bind(&repository.id.0)
                .bind(&repository.org_id.0)
                .bind(ciphertext)
                .bind(nonce)
                .bind(TimestampMicros::now().0)
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;
            }
            TokenChange::Clear => {
                sqlx::query("DELETE FROM agent_skill_repository_secrets WHERE repository_id = $1")
                    .bind(&repository.id.0)
                    .execute(&mut *tx)
                    .await
                    .map_err(sqlx_err)?;
            }
        }

        tx.commit().await.map_err(sqlx_err)?;
        repository_row(row)
    }

    async fn repository_token(&self, org_id: &Id, id: &Id) -> Result<Option<String>> {
        let row = sqlx::query(
            "SELECT ciphertext, nonce FROM agent_skill_repository_secrets
             WHERE org_id = $1 AND repository_id = $2",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let Some(row) = row else {
            return Ok(None);
        };

        let ciphertext: Vec<u8> = row.try_get("ciphertext").map_err(sqlx_err)?;
        let nonce: Vec<u8> = row.try_get("nonce").map_err(sqlx_err)?;

        let plaintext = self
            .cipher
            .open(&nonce, &ciphertext)
            .map_err(|error| Error::internal(format!("repository token open: {error}")))?;

        String::from_utf8(plaintext)
            .map(Some)
            .map_err(|error| Error::internal(format!("repository token is not UTF-8: {error}")))
    }

    async fn record_sync(
        &self,
        org_id: &Id,
        id: &Id,
        status: RepositoryStatus,
        error: Option<&str>,
        commit: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE agent_skill_repositories SET
                status = $3, last_error = $4, last_commit = COALESCE($5, last_commit),
                last_synced_at_micros = $6
             WHERE org_id = $1 AND id = $2",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(status.as_str())
        .bind(error)
        .bind(commit)
        .bind(TimestampMicros::now().0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn delete_repository(&self, org_id: &Id, id: &Id) -> Result<bool> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        sqlx::query("DELETE FROM agent_skills WHERE org_id = $1 AND repository_id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;

        sqlx::query("DELETE FROM agent_skill_repository_secrets WHERE repository_id = $1")
            .bind(&id.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;

        let result =
            sqlx::query("DELETE FROM agent_skill_repositories WHERE org_id = $1 AND id = $2")
                .bind(&org_id.0)
                .bind(&id.0)
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(result.rows_affected() > 0)
    }

    async fn repositories_due(&self, before: TimestampMicros) -> Result<Vec<SkillRepository>> {
        let rows = sqlx::query(&format!(
            "SELECT {REPOSITORY_COLS} FROM agent_skill_repositories
             WHERE auto_sync
               AND (last_synced_at_micros IS NULL OR last_synced_at_micros < $1)
             ORDER BY last_synced_at_micros NULLS FIRST
             LIMIT 20"
        ))
        .bind(before.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(repository_row).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_last_four_characters_of_a_token_are_kept() {
        assert_eq!(last4("ghp_abcdef123456"), "3456");
        assert_eq!(last4("abc"), "abc");
    }
}
