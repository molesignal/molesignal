// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Organization-scoped custom report templates.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportTemplate {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub description: String,
    pub target_type: String,
    pub format: String,
    pub time_range_preset: String,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait ReportTemplateRepository: Send + Sync {
    async fn list(&self, org_id: &Id) -> Result<Vec<ReportTemplate>>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<ReportTemplate>;
    async fn create(&self, template: ReportTemplate) -> Result<ReportTemplate>;
    async fn update(&self, template: ReportTemplate) -> Result<ReportTemplate>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;
}

pub struct PgReportTemplateRepository {
    pool: PgPool,
}

impl PgReportTemplateRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, name, description, target_type, format,
                    time_range_preset, created_at_micros, updated_at_micros";

fn row_to(row: sqlx::postgres::PgRow) -> ReportTemplate {
    ReportTemplate {
        id: Id(row.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(row.try_get::<String, _>("org_id").unwrap_or_default()),
        name: row.try_get::<String, _>("name").unwrap_or_default(),
        description: row.try_get::<String, _>("description").unwrap_or_default(),
        target_type: row.try_get::<String, _>("target_type").unwrap_or_default(),
        format: row.try_get::<String, _>("format").unwrap_or_default(),
        time_range_preset: row
            .try_get::<String, _>("time_range_preset")
            .unwrap_or_default(),
        created_at: TimestampMicros(
            row.try_get::<i64, _>("created_at_micros")
                .unwrap_or_default(),
        ),
        updated_at: TimestampMicros(
            row.try_get::<i64, _>("updated_at_micros")
                .unwrap_or_default(),
        ),
    }
}

#[async_trait]
impl ReportTemplateRepository for PgReportTemplateRepository {
    async fn list(&self, org_id: &Id) -> Result<Vec<ReportTemplate>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM report_templates WHERE org_id = $1
             ORDER BY updated_at_micros DESC, name ASC"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<ReportTemplate> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM report_templates WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(row_to(row))
    }

    async fn create(&self, template: ReportTemplate) -> Result<ReportTemplate> {
        sqlx::query(
            "INSERT INTO report_templates
                (id, org_id, name, description, target_type, format,
                 time_range_preset, created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(&template.id.0)
        .bind(&template.org_id.0)
        .bind(&template.name)
        .bind(&template.description)
        .bind(&template.target_type)
        .bind(&template.format)
        .bind(&template.time_range_preset)
        .bind(template.created_at.0)
        .bind(template.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(template)
    }

    async fn update(&self, template: ReportTemplate) -> Result<ReportTemplate> {
        sqlx::query(
            "UPDATE report_templates
             SET name = $3, description = $4, target_type = $5, format = $6,
                 time_range_preset = $7, updated_at_micros = $8
             WHERE org_id = $1 AND id = $2",
        )
        .bind(&template.org_id.0)
        .bind(&template.id.0)
        .bind(&template.name)
        .bind(&template.description)
        .bind(&template.target_type)
        .bind(&template.format)
        .bind(&template.time_range_preset)
        .bind(template.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(template)
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM report_templates WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }
}
