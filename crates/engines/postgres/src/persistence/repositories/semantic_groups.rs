// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `semantic_groups` 表 Pg 实装（告警分组规则）。

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::alerting::semantic_group::{LabelMatcher, SemanticGroup, SemanticGroupRepository},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgSemanticGroupRepository {
    pool: PgPool,
}

impl PgSemanticGroupRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str =
    "id, org_id, name, enabled, matchers, group_by, created_at_micros, updated_at_micros";

fn row_to(row: sqlx::postgres::PgRow) -> Result<SemanticGroup> {
    let matchers: Json<Vec<LabelMatcher>> = row.try_get("matchers").map_err(sqlx_err)?;
    let group_by: Json<Vec<String>> = row.try_get("group_by").map_err(sqlx_err)?;
    Ok(SemanticGroup {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        matchers: matchers.0,
        group_by: group_by.0,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl SemanticGroupRepository for PgSemanticGroupRepository {
    async fn create(&self, g: SemanticGroup) -> Result<SemanticGroup> {
        sqlx::query(
            "INSERT INTO semantic_groups
                (id, org_id, name, enabled, matchers, group_by,
                 created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&g.id.0)
        .bind(&g.org_id.0)
        .bind(&g.name)
        .bind(g.enabled)
        .bind(Json(&g.matchers))
        .bind(Json(&g.group_by))
        .bind(g.created_at.0)
        .bind(g.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(g)
    }

    async fn update(&self, g: SemanticGroup) -> Result<SemanticGroup> {
        sqlx::query(
            "UPDATE semantic_groups SET
               name = $2, enabled = $3, matchers = $4, group_by = $5, updated_at_micros = $6
             WHERE id = $1",
        )
        .bind(&g.id.0)
        .bind(&g.name)
        .bind(g.enabled)
        .bind(Json(&g.matchers))
        .bind(Json(&g.group_by))
        .bind(g.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(g)
    }

    async fn get(&self, id: &Id) -> Result<SemanticGroup> {
        let row = sqlx::query(&format!("SELECT {COLS} FROM semantic_groups WHERE id = $1"))
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        row_to(row)
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<SemanticGroup>> {
        // created_at 升序 = 定义顺序，与 list_enabled 的 precedence 契约一致。
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM semantic_groups WHERE org_id = $1
             ORDER BY created_at_micros, id"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to).collect()
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM semantic_groups WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }

    async fn list_enabled(&self, org_id: &Id) -> Result<Vec<SemanticGroup>> {
        // 定义顺序（created_at 升序）：evaluator 取第一条命中，precedence = 创建顺序。
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM semantic_groups
             WHERE org_id = $1 AND enabled = TRUE ORDER BY created_at_micros, id"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to).collect()
    }

    /// 批量导入：单事务内逐条 INSERT，中途任一条失败整体回滚（import 原子性）。
    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "semantic_groups")
    )]
    async fn create_many(&self, groups: Vec<SemanticGroup>) -> Result<usize> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let mut n = 0;
        for g in &groups {
            sqlx::query(
                "INSERT INTO semantic_groups
                    (id, org_id, name, enabled, matchers, group_by,
                     created_at_micros, updated_at_micros)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(&g.id.0)
            .bind(&g.org_id.0)
            .bind(&g.name)
            .bind(g.enabled)
            .bind(Json(&g.matchers))
            .bind(Json(&g.group_by))
            .bind(g.created_at.0)
            .bind(g.updated_at.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
            n += 1;
        }
        tx.commit().await.map_err(sqlx_err)?;
        Ok(n)
    }
}
