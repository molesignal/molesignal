// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `agent_knowledge_files`：组织与各团队的知识文件。份额（文件数、合计大小）在写入的事务里按
//! 写完之后的结果检查，同一份知识的并发写入靠锁住这份知识的已有行排队。

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row, postgres::PgRow};

use crate::{
    agent::{
        knowledge::{
            ENTRY_FILE, KnowledgeBase, KnowledgeFile, KnowledgeFileMeta, KnowledgeHit,
            KnowledgeStore, check_content, check_quota, matching_lines, normalize_path,
        },
        scope::Scope,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
    sqlx_err,
};

/// 搜索时一次最多读这么多个文件。
const SEARCH_FILES: i64 = 1_000;

pub struct PgKnowledgeStore {
    pool: PgPool,
}

impl PgKnowledgeStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn knowledge_scope(scope: &Scope) -> Result<(&'static str, Option<&str>)> {
    match scope {
        Scope::Org => Ok(("org", None)),
        Scope::Team { team_id } => Ok(("team", Some(team_id.0.as_str()))),
        Scope::Personal { .. } => Err(Error::invalid(
            "knowledge belongs to the organization or a team",
        )),
    }
}

fn row_scope(row: &PgRow) -> Result<Scope> {
    let kind: String = row.try_get("scope").map_err(sqlx_err)?;
    let team_id: Option<String> = row.try_get("team_id").map_err(sqlx_err)?;

    Scope::from_parts(&kind, team_id.as_deref(), None)
}

fn file_row(row: PgRow) -> Result<KnowledgeFile> {
    Ok(KnowledgeFile {
        scope: row_scope(&row)?,
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        path: row.try_get("path").map_err(sqlx_err)?,
        content: row.try_get("content").map_err(sqlx_err)?,
        size_bytes: row.try_get("size_bytes").map_err(sqlx_err)?,
        content_hash: row.try_get("content_hash").map_err(sqlx_err)?,
        created_by: Id(row.try_get("created_by").map_err(sqlx_err)?),
        updated_by: Id(row.try_get("updated_by").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl KnowledgeStore for PgKnowledgeStore {
    async fn bases(&self, org_id: &Id) -> Result<Vec<KnowledgeBase>> {
        let rows = sqlx::query(
            "SELECT scope, team_id, COUNT(*) AS files, SUM(size_bytes)::BIGINT AS bytes,
                    MAX(updated_at_micros) AS updated_at,
                    MAX(updated_at_micros) FILTER (WHERE path = $2) AS entry_updated_at,
                    COUNT(*) FILTER (
                        WHERE path <> $2 AND updated_at_micros > COALESCE(
                            (SELECT e.updated_at_micros FROM agent_knowledge_files e
                             WHERE e.org_id = f.org_id AND e.scope_key = f.scope_key
                               AND e.path = $2), 0)
                    ) AS changed_since_entry
             FROM agent_knowledge_files f
             WHERE org_id = $1
             GROUP BY org_id, scope_key, scope, team_id",
        )
        .bind(&org_id.0)
        .bind(ENTRY_FILE)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                let entry: Option<i64> = row.try_get("entry_updated_at").map_err(sqlx_err)?;

                Ok(KnowledgeBase {
                    scope: row_scope(&row)?,
                    files: row.try_get("files").map_err(sqlx_err)?,
                    bytes: row.try_get("bytes").map_err(sqlx_err)?,
                    entry_updated_at: entry.map(TimestampMicros),
                    changed_since_entry: if entry.is_some() {
                        row.try_get("changed_since_entry").map_err(sqlx_err)?
                    } else {
                        0
                    },
                    updated_at: row
                        .try_get::<Option<i64>, _>("updated_at")
                        .map_err(sqlx_err)?
                        .map(TimestampMicros),
                })
            })
            .collect()
    }

    async fn list(&self, org_id: &Id, scope: &Scope) -> Result<Vec<KnowledgeFileMeta>> {
        knowledge_scope(scope)?;

        let rows = sqlx::query(
            "SELECT path, size_bytes, updated_by, updated_at_micros FROM agent_knowledge_files
             WHERE org_id = $1 AND scope_key = $2
             ORDER BY path",
        )
        .bind(&org_id.0)
        .bind(scope.key())
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                Ok(KnowledgeFileMeta {
                    path: row.try_get("path").map_err(sqlx_err)?,
                    size_bytes: row.try_get("size_bytes").map_err(sqlx_err)?,
                    updated_by: Id(row.try_get("updated_by").map_err(sqlx_err)?),
                    updated_at: TimestampMicros(
                        row.try_get("updated_at_micros").map_err(sqlx_err)?,
                    ),
                })
            })
            .collect()
    }

    async fn get(&self, org_id: &Id, scope: &Scope, path: &str) -> Result<Option<KnowledgeFile>> {
        knowledge_scope(scope)?;
        let path = normalize_path(path)?;

        sqlx::query(
            "SELECT * FROM agent_knowledge_files
             WHERE org_id = $1 AND scope_key = $2 AND path = $3",
        )
        .bind(&org_id.0)
        .bind(scope.key())
        .bind(&path)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(file_row)
        .transpose()
    }

    async fn put(
        &self,
        org_id: &Id,
        scope: &Scope,
        files: &[(String, String)],
        actor: &Id,
    ) -> Result<()> {
        let (kind, team_id) = knowledge_scope(scope)?;
        let mut normalized = Vec::with_capacity(files.len());

        for (path, content) in files {
            check_content(content)?;
            normalized.push((normalize_path(path)?, content.as_str()));
        }

        let now = TimestampMicros::now();
        let key = scope.key();
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        // 锁住这份知识的已有行：同时往同一份知识里写的请求排队，份额才算得准。
        let existing = sqlx::query(
            "SELECT path, size_bytes FROM agent_knowledge_files
             WHERE org_id = $1 AND scope_key = $2
             FOR UPDATE",
        )
        .bind(&org_id.0)
        .bind(&key)
        .fetch_all(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        let mut sizes = existing
            .iter()
            .map(|row| {
                Ok((
                    row.try_get::<String, _>("path").map_err(sqlx_err)?,
                    row.try_get::<i64, _>("size_bytes").map_err(sqlx_err)? as usize,
                ))
            })
            .collect::<Result<std::collections::HashMap<_, _>>>()?;

        for (path, content) in &normalized {
            sizes.insert(path.clone(), content.len());
        }

        check_quota(sizes.len(), sizes.values().sum())?;

        for (path, content) in normalized {
            let hash = hex::encode(Sha256::digest(content.as_bytes()));

            sqlx::query(
                "INSERT INTO agent_knowledge_files
                    (org_id, scope, team_id, scope_key, path, content, size_bytes, content_hash,
                     created_by, updated_by, created_at_micros, updated_at_micros)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9, $10, $10)
                 ON CONFLICT (org_id, scope_key, path) DO UPDATE SET
                    content = EXCLUDED.content, size_bytes = EXCLUDED.size_bytes,
                    content_hash = EXCLUDED.content_hash, updated_by = EXCLUDED.updated_by,
                    updated_at_micros = EXCLUDED.updated_at_micros
                 WHERE agent_knowledge_files.content_hash <> EXCLUDED.content_hash",
            )
            .bind(&org_id.0)
            .bind(kind)
            .bind(team_id)
            .bind(&key)
            .bind(&path)
            .bind(content)
            .bind(content.len() as i64)
            .bind(hash)
            .bind(&actor.0)
            .bind(now.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }

        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn delete(&self, org_id: &Id, scope: &Scope, path: &str) -> Result<bool> {
        knowledge_scope(scope)?;
        let path = normalize_path(path)?;

        let deleted = sqlx::query(
            "DELETE FROM agent_knowledge_files
             WHERE org_id = $1 AND scope_key = $2 AND (path = $3 OR path LIKE $4)",
        )
        .bind(&org_id.0)
        .bind(scope.key())
        .bind(&path)
        .bind(format!(
            "{}/%",
            path.replace('%', "\\%").replace('_', "\\_")
        ))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(deleted.rows_affected() > 0)
    }

    async fn rename(
        &self,
        org_id: &Id,
        scope: &Scope,
        from: &str,
        to: &str,
        actor: &Id,
    ) -> Result<()> {
        knowledge_scope(scope)?;
        let (from, to) = (normalize_path(from)?, normalize_path(to)?);

        if from == to {
            return Ok(());
        }

        let renamed = sqlx::query(
            "UPDATE agent_knowledge_files SET path = $4, updated_by = $5, updated_at_micros = $6
             WHERE org_id = $1 AND scope_key = $2 AND path = $3",
        )
        .bind(&org_id.0)
        .bind(scope.key())
        .bind(&from)
        .bind(&to)
        .bind(&actor.0)
        .bind(TimestampMicros::now().0)
        .execute(&self.pool)
        .await
        .map_err(|error| match sqlx_err(error) {
            Error::Conflict(_) => Error::conflict(format!("`{to}` already exists")),
            other => other,
        })?;

        if renamed.rows_affected() == 0 {
            return Err(Error::not_found(format!(
                "knowledge file `{from}` not found"
            )));
        }

        Ok(())
    }

    async fn search(
        &self,
        org_id: &Id,
        scopes: &[Scope],
        query: &str,
        limit: usize,
    ) -> Result<Vec<KnowledgeHit>> {
        let keys = scopes.iter().map(Scope::key).collect::<Vec<_>>();
        let first = query.split_whitespace().next().unwrap_or_default();

        if keys.is_empty() || first.is_empty() {
            return Ok(Vec::new());
        }

        // 先用第一个词在库里粗筛，再逐行按全部词匹配。
        let rows = sqlx::query(
            "SELECT * FROM agent_knowledge_files
             WHERE org_id = $1 AND scope_key = ANY($2) AND strpos(lower(content), lower($3)) > 0
             ORDER BY scope_key, path
             LIMIT $4",
        )
        .bind(&org_id.0)
        .bind(&keys)
        .bind(first)
        .bind(SEARCH_FILES)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let mut hits = Vec::new();

        for row in rows {
            let file = file_row(row)?;

            for (line, text) in matching_lines(&file.content, query, limit - hits.len()) {
                hits.push(KnowledgeHit {
                    scope: file.scope.clone(),
                    path: file.path.clone(),
                    line,
                    text,
                });
            }

            if hits.len() >= limit {
                break;
            }
        }

        Ok(hits)
    }
}
