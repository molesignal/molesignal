// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::{
        query::QueryLanguage,
        saved_view::{SavedView, SavedViewRepository},
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgSavedViewRepository {
    pool: PgPool,
}

impl PgSavedViewRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, owner_user_id, name, language, statement, time_range_secs,
     stream, tags, pinned, created_at_micros, updated_at_micros";

/// `language` is stored as the snake_case discriminant (matches the
/// `#[serde(rename_all = "snake_case")]` wire form of `QueryLanguage`).
fn lang_to_str(l: QueryLanguage) -> &'static str {
    match l {
        QueryLanguage::Sql => "sql",
        QueryLanguage::Promql => "promql",
    }
}

fn str_to_lang(s: &str) -> QueryLanguage {
    match s {
        "promql" => QueryLanguage::Promql,
        _ => QueryLanguage::Sql,
    }
}

fn row_to_view(row: sqlx::postgres::PgRow) -> Result<SavedView> {
    let tags: Json<Vec<String>> = row.try_get("tags").map_err(sqlx_err)?;
    let language: String = row.try_get("language").map_err(sqlx_err)?;
    let time_range_secs: i32 = row.try_get("time_range_secs").map_err(sqlx_err)?;
    Ok(SavedView {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        owner_user_id: Id::from_string(
            row.try_get::<String, _>("owner_user_id")
                .map_err(sqlx_err)?,
        ),
        name: row.try_get("name").map_err(sqlx_err)?,
        language: str_to_lang(&language),
        statement: row.try_get("statement").map_err(sqlx_err)?,
        time_range_secs: time_range_secs as u32,
        stream: row.try_get("stream").map_err(sqlx_err)?,
        tags: tags.0,
        pinned: row.try_get("pinned").map_err(sqlx_err)?,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl SavedViewRepository for PgSavedViewRepository {
    async fn create(&self, v: SavedView) -> Result<SavedView> {
        sqlx::query(
            "INSERT INTO saved_views
             (id, org_id, owner_user_id, name, language, statement, time_range_secs,
              stream, tags, pinned, created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(&v.id.0)
        .bind(&v.org_id.0)
        .bind(&v.owner_user_id.0)
        .bind(&v.name)
        .bind(lang_to_str(v.language))
        .bind(&v.statement)
        .bind(v.time_range_secs as i32)
        .bind(&v.stream)
        .bind(Json(&v.tags))
        .bind(v.pinned)
        .bind(v.created_at.0)
        .bind(v.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(v)
    }

    async fn update(&self, v: SavedView) -> Result<SavedView> {
        // org_id is part of the predicate so an update can never reach across orgs.
        sqlx::query(
            "UPDATE saved_views SET
               name = $3, language = $4, statement = $5, time_range_secs = $6,
               stream = $7, tags = $8, pinned = $9, updated_at_micros = $10
             WHERE id = $1 AND org_id = $2",
        )
        .bind(&v.id.0)
        .bind(&v.org_id.0)
        .bind(&v.name)
        .bind(lang_to_str(v.language))
        .bind(&v.statement)
        .bind(v.time_range_secs as i32)
        .bind(&v.stream)
        .bind(Json(&v.tags))
        .bind(v.pinned)
        .bind(v.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(v)
    }

    async fn get_by_id(&self, id: &Id) -> Result<SavedView> {
        let row = sqlx::query(&format!("SELECT {COLS} FROM saved_views WHERE id = $1"))
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        row_to_view(row)
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<SavedView> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM saved_views WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row_to_view(row)
    }

    async fn list(&self, org_id: &Id, pinned_only: bool) -> Result<Vec<SavedView>> {
        let sql = if pinned_only {
            format!(
                "SELECT {COLS} FROM saved_views
                 WHERE org_id = $1 AND pinned = TRUE
                 ORDER BY updated_at_micros DESC"
            )
        } else {
            format!(
                "SELECT {COLS} FROM saved_views
                 WHERE org_id = $1
                 ORDER BY pinned DESC, updated_at_micros DESC"
            )
        };
        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_view).collect()
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM saved_views WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }
}
