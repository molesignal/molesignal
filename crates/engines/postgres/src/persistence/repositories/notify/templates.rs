// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use super::super::sqlx_err;
use crate::{
    domain::notify::{
        preference::NotifyCategory, repositories::NotifyTemplateRepository,
        template::NotifyTemplate,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyTemplateRecord {
    pub id: Id,
    pub organization_id: Id,
    pub name: String,
    pub body: String,
    pub format: String,
    #[serde(default = "default_template_category")]
    pub category: NotifyCategory,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

const fn default_template_category() -> NotifyCategory {
    NotifyCategory::Alert
}

#[async_trait]
pub trait NotifyTemplateManagementRepository: Send + Sync {
    async fn list(&self, organization_id: &Id) -> Result<Vec<NotifyTemplateRecord>>;
    async fn get(&self, organization_id: &Id, id: &Id) -> Result<NotifyTemplateRecord>;
    async fn create(&self, template: NotifyTemplateRecord) -> Result<NotifyTemplateRecord>;
    async fn update(&self, template: NotifyTemplateRecord) -> Result<NotifyTemplateRecord>;
    async fn delete(&self, organization_id: &Id, id: &Id) -> Result<()>;
}

pub struct PgNotifyTemplateRepository {
    pool: PgPool,
}

impl PgNotifyTemplateRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, name, body, format, category, created_at_micros, updated_at_micros";

fn row_to_record(row: sqlx::postgres::PgRow) -> Result<NotifyTemplateRecord> {
    let category: String = row.try_get("category").map_err(sqlx_err)?;
    Ok(NotifyTemplateRecord {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        organization_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        body: row.try_get("body").map_err(sqlx_err)?,
        format: row.try_get("format").map_err(sqlx_err)?,
        category: NotifyCategory::parse(&category)?,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl NotifyTemplateManagementRepository for PgNotifyTemplateRepository {
    async fn list(&self, organization_id: &Id) -> Result<Vec<NotifyTemplateRecord>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS}
               FROM notify_templates
              WHERE org_id = $1
           ORDER BY name"
        ))
        .bind(&organization_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_record).collect()
    }

    async fn get(&self, organization_id: &Id, id: &Id) -> Result<NotifyTemplateRecord> {
        let row = sqlx::query(&format!(
            "SELECT {COLS}
               FROM notify_templates
              WHERE org_id = $1 AND id = $2"
        ))
        .bind(&organization_id.0)
        .bind(&id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row_to_record(row)
    }

    async fn create(&self, template: NotifyTemplateRecord) -> Result<NotifyTemplateRecord> {
        sqlx::query(
            "INSERT INTO notify_templates (
                 id, org_id, name, body, format, category, created_at_micros, updated_at_micros
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&template.id.0)
        .bind(&template.organization_id.0)
        .bind(&template.name)
        .bind(&template.body)
        .bind(&template.format)
        .bind(template.category.as_str())
        .bind(template.created_at.0)
        .bind(template.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(template)
    }

    async fn update(&self, template: NotifyTemplateRecord) -> Result<NotifyTemplateRecord> {
        sqlx::query(
            "UPDATE notify_templates
                SET name = $3,
                    body = $4,
                    format = $5,
                    category = $6,
                    updated_at_micros = $7
              WHERE org_id = $1 AND id = $2",
        )
        .bind(&template.organization_id.0)
        .bind(&template.id.0)
        .bind(&template.name)
        .bind(&template.body)
        .bind(&template.format)
        .bind(template.category.as_str())
        .bind(template.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(template)
    }

    async fn delete(&self, organization_id: &Id, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM notify_templates WHERE org_id = $1 AND id = $2")
            .bind(&organization_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }
}

#[async_trait]
impl NotifyTemplateRepository for PgNotifyTemplateRepository {
    async fn get(&self, organization_id: &Id, id: &Id) -> Result<NotifyTemplate> {
        let row = sqlx::query(
            "SELECT id, org_id, body, format, category
               FROM notify_templates
              WHERE org_id = $1 AND id = $2",
        )
        .bind(&organization_id.0)
        .bind(&id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        let category: String = row.try_get("category").map_err(sqlx_err)?;
        Ok(NotifyTemplate {
            id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
            organization_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
            body: row.try_get("body").map_err(sqlx_err)?,
            format: row.try_get("format").map_err(sqlx_err)?,
            category: NotifyCategory::parse(&category)?,
        })
    }
}
