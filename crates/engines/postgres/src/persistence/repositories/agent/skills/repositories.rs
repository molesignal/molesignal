// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `agent_skill_repositories`：拉取 Skill 的仓库。访问令牌用 [`CipherRootKey`] seal 后存在
//! `sealed_secrets`（`owner_kind = 'skill_repository'`），列表与详情只有 `token_set` 与末四位。
//!
//! [`CipherRootKey`]: crate::infra::cipher::CipherRootKey

use sqlx::{Row, postgres::PgRow};

use super::{super::super::sqlx_err, PgSkillStore, scope_of};
use crate::{
    agent::skills::{RepoHost, RepositoryStatus, SkillRepository, TokenChange},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const REPOSITORY_COLS: &str = "id, org_id, scope, team_id, owner_id, url, host, archive_url, \
    git_ref, dir, auto_sync, token_set, token_last4, status, last_error, last_commit, \
    last_synced_at, created_by, updated_by, created_at, updated_at";

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
            .try_get::<Option<TimestampMicros>, _>("last_synced_at")
            .map_err(sqlx_err)?,
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

fn last4(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    chars[chars.len().saturating_sub(4)..].iter().collect()
}

impl PgSkillStore {
    pub(super) async fn repositories_in(&self, org_id: &Id) -> Result<Vec<SkillRepository>> {
        let rows = sqlx::query(&format!(
            "SELECT {REPOSITORY_COLS} FROM agent_skill_repositories
             WHERE org_id = $1 ORDER BY created_at"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(repository_row).collect()
    }

    pub(super) async fn repository(&self, org_id: &Id, id: &Id) -> Result<SkillRepository> {
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

    pub(super) async fn put_repository(
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
                 last_commit, last_synced_at, created_by, updated_by, created_at,
                 updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16,
                     $17, $18, $19, $20, $21, $22)
             ON CONFLICT (id) DO UPDATE SET
                scope = EXCLUDED.scope, team_id = EXCLUDED.team_id,
                owner_id = EXCLUDED.owner_id, scope_key = EXCLUDED.scope_key,
                url = EXCLUDED.url, host = EXCLUDED.host, archive_url = EXCLUDED.archive_url,
                git_ref = EXCLUDED.git_ref, dir = EXCLUDED.dir, auto_sync = EXCLUDED.auto_sync,
                token_set = EXCLUDED.token_set, token_last4 = EXCLUDED.token_last4,
                updated_by = EXCLUDED.updated_by, updated_at = EXCLUDED.updated_at
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
        .bind(repository.last_synced_at)
        .bind(&repository.created_by.0)
        .bind(&repository.updated_by.0)
        .bind(repository.created_at)
        .bind(repository.updated_at)
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
                    "INSERT INTO sealed_secrets
                        (owner_id, org_id, owner_kind, ciphertext, nonce, created_at, updated_at)
                     VALUES ($1, $2, 'skill_repository', $3, $4, $5, $5)
                     ON CONFLICT (org_id, owner_kind, owner_id, slot) DO UPDATE SET
                        ciphertext = EXCLUDED.ciphertext, nonce = EXCLUDED.nonce,
                        updated_at = EXCLUDED.updated_at",
                )
                .bind(&repository.id.0)
                .bind(&repository.org_id.0)
                .bind(ciphertext)
                .bind(nonce)
                .bind(TimestampMicros::now())
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;
            }
            TokenChange::Clear => {
                sqlx::query(
                    "DELETE FROM sealed_secrets
                     WHERE owner_id = $1 AND org_id = $2 AND owner_kind = 'skill_repository'",
                )
                .bind(&repository.id.0)
                .bind(&repository.org_id.0)
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;
            }
        }

        tx.commit().await.map_err(sqlx_err)?;
        repository_row(row)
    }

    pub(super) async fn open_repository_token(
        &self,
        org_id: &Id,
        id: &Id,
    ) -> Result<Option<String>> {
        let row = sqlx::query(
            "SELECT ciphertext, nonce FROM sealed_secrets
             WHERE org_id = $1 AND owner_kind = 'skill_repository' AND owner_id = $2",
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

    pub(super) async fn record_repository_sync(
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
                last_synced_at = $6
             WHERE org_id = $1 AND id = $2",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(status.as_str())
        .bind(error)
        .bind(commit)
        .bind(TimestampMicros::now())
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    pub(super) async fn remove_repository(&self, org_id: &Id, id: &Id) -> Result<bool> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        sqlx::query("DELETE FROM agent_skills WHERE org_id = $1 AND repository_id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;

        sqlx::query(
            "DELETE FROM sealed_secrets
             WHERE owner_id = $1 AND org_id = $2 AND owner_kind = 'skill_repository'",
        )
        .bind(&id.0)
        .bind(&org_id.0)
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

    pub(super) async fn due_repositories(
        &self,
        before: TimestampMicros,
    ) -> Result<Vec<SkillRepository>> {
        let rows = sqlx::query(&format!(
            "SELECT {REPOSITORY_COLS} FROM agent_skill_repositories
             WHERE auto_sync
               AND (last_synced_at IS NULL OR last_synced_at < $1)
             ORDER BY last_synced_at NULLS FIRST
             LIMIT 20"
        ))
        .bind(before)
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
