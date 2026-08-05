// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::{
    domain::dashboard::{Folder, repositories::FolderRepository},
    shared::{Error, Result, ids::Id},
};

pub struct PgFolderRepository {
    pool: PgPool,
}

impl PgFolderRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_folder(row: sqlx::postgres::PgRow) -> Result<Folder> {
    let parent: Option<String> = row.try_get("parent_id").map_err(sqlx_err)?;
    Ok(Folder {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        parent_id: parent.map(Id::from_string),
    })
}

#[async_trait]
impl FolderRepository for PgFolderRepository {
    async fn create(&self, folder: Folder) -> Result<Folder> {
        sqlx::query("INSERT INTO folders (id, org_id, name, parent_id) VALUES ($1, $2, $3, $4)")
            .bind(&folder.id.0)
            .bind(&folder.org_id.0)
            .bind(&folder.name)
            .bind(folder.parent_id.as_ref().map(|i| &i.0))
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(folder)
    }

    async fn get_by_id(&self, id: &Id) -> Result<Folder> {
        let row = sqlx::query("SELECT id, org_id, name, parent_id FROM folders WHERE id = $1")
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found(format!("folder {}", id.0)))?;
        row_to_folder(row)
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<Folder> {
        let row = sqlx::query(
            "SELECT id, org_id, name, parent_id FROM folders WHERE org_id = $1 AND id = $2",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("folder {}", id.0)))?;
        row_to_folder(row)
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<Folder>> {
        let rows = sqlx::query("SELECT id, org_id, name, parent_id FROM folders WHERE org_id = $1")
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_folder).collect()
    }

    async fn update(&self, folder: Folder) -> Result<Folder> {
        let res = sqlx::query("UPDATE folders SET name = $2, parent_id = $3 WHERE id = $1")
            .bind(&folder.id.0)
            .bind(&folder.name)
            .bind(folder.parent_id.as_ref().map(|i| &i.0))
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found(format!("folder {}", folder.id.0)));
        }
        Ok(folder)
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM folders WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }
}
