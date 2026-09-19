// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::iam::{Team, TeamRepository},
    shared::{Error, Result, ids::Id},
};

pub struct PgTeamRepository {
    pool: PgPool,
}

impl PgTeamRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_team(row: sqlx::postgres::PgRow) -> Result<Team> {
    let ids: Json<Vec<String>> = row.try_get("member_ids").map_err(sqlx_err)?;
    Ok(Team {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        member_ids: ids.0.into_iter().map(Id::from_string).collect(),
    })
}

fn encode_members(t: &Team) -> Result<Json<Vec<String>>> {
    let v: Vec<String> = t.member_ids.iter().map(|i| i.0.clone()).collect();
    Ok(Json(v))
}

#[async_trait]
impl TeamRepository for PgTeamRepository {
    async fn create(&self, team: Team) -> Result<Team> {
        ensure_non_system_org(&self.pool, &team.org_id).await?;
        let members = encode_members(&team)?;
        sqlx::query("INSERT INTO teams (id, org_id, name, member_ids) VALUES ($1, $2, $3, $4)")
            .bind(&team.id.0)
            .bind(&team.org_id.0)
            .bind(&team.name)
            .bind(members)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(team)
    }

    async fn update(&self, team: Team) -> Result<Team> {
        ensure_non_system_org(&self.pool, &team.org_id).await?;
        let members = encode_members(&team)?;
        sqlx::query("UPDATE teams SET name = $2, member_ids = $3 WHERE id = $1")
            .bind(&team.id.0)
            .bind(&team.name)
            .bind(members)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(team)
    }

    async fn get(&self, id: &Id) -> Result<Team> {
        let row = sqlx::query("SELECT id, org_id, name, member_ids FROM teams WHERE id = $1")
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        row_to_team(row)
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<Team>> {
        let rows = sqlx::query("SELECT id, org_id, name, member_ids FROM teams WHERE org_id = $1")
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_team).collect()
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM teams WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }
}

async fn ensure_non_system_org(pool: &PgPool, org_id: &Id) -> Result<()> {
    let row = sqlx::query("SELECT system FROM organizations WHERE id = $1")
        .bind(&org_id.0)
        .fetch_optional(pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found("organization"))?;
    let system: bool = row.try_get("system").map_err(sqlx_err)?;
    if system {
        Err(Error::forbidden(
            "system organization does not accept teams",
        ))
    } else {
        Ok(())
    }
}

// 让 Error 不报 unused
fn _anchor(e: Error) -> Error {
    e
}
