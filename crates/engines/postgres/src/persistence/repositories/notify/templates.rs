// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row, types::Json};

use super::super::sqlx_err;
use crate::{
    domain::notify::{
        connector::NotifyChannel,
        preference::NotifyCategory,
        repositories::NotifyTemplateRepository,
        template::{NotifyCard, NotifyTemplate, ProviderTemplate},
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyTemplateRecord {
    pub id: Id,
    pub organization_id: Id,
    pub name: String,
    pub body: String,
    pub format: String,
    #[serde(default = "default_template_category")]
    pub category: NotifyCategory,
    /// 空着是通用模板。
    #[serde(default)]
    pub channel: Option<NotifyChannel>,
    #[serde(default)]
    pub provider_templates: BTreeMap<String, ProviderTemplate>,
    /// 即时通讯的 App 与群机器人发成的卡片；只有即时通讯模板有。
    #[serde(default)]
    pub card: Option<NotifyCard>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

impl NotifyTemplateRecord {
    /// 引擎渲染用的那部分。
    pub fn template(&self) -> NotifyTemplate {
        NotifyTemplate {
            id: self.id.clone(),
            organization_id: self.organization_id.clone(),
            category: self.category,
            channel: self.channel,
            body: self.body.clone(),
            format: self.format.clone(),
            provider_templates: self.provider_templates.clone(),
            card: self.card.clone(),
        }
    }
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

const COLS: &str = "id, org_id, name, body, format, category, channel, provider_templates, card,
    created_at, updated_at";

fn row_to_record(row: sqlx::postgres::PgRow) -> Result<NotifyTemplateRecord> {
    let category: String = row.try_get("category").map_err(sqlx_err)?;
    let channel: Option<String> = row.try_get("channel").map_err(sqlx_err)?;
    let provider_templates: Json<BTreeMap<String, ProviderTemplate>> =
        row.try_get("provider_templates").map_err(sqlx_err)?;
    let card: Option<Json<NotifyCard>> = row.try_get("card").map_err(sqlx_err)?;

    Ok(NotifyTemplateRecord {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        organization_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        body: row.try_get("body").map_err(sqlx_err)?,
        format: row.try_get("format").map_err(sqlx_err)?,
        category: NotifyCategory::parse(&category)?,
        channel: channel.as_deref().map(NotifyChannel::parse).transpose()?,
        provider_templates: provider_templates.0,
        card: card.map(|card| card.0),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
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
                 id, org_id, name, body, format, category, channel, provider_templates, card,
                 created_at, updated_at
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(&template.id.0)
        .bind(&template.organization_id.0)
        .bind(&template.name)
        .bind(&template.body)
        .bind(&template.format)
        .bind(template.category.as_str())
        .bind(template.channel.map(NotifyChannel::as_str))
        .bind(Json(&template.provider_templates))
        .bind(template.card.as_ref().map(Json))
        .bind(template.created_at)
        .bind(template.updated_at)
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
                    channel = $7,
                    provider_templates = $8,
                    card = $9,
                    updated_at = $10
              WHERE org_id = $1 AND id = $2",
        )
        .bind(&template.organization_id.0)
        .bind(&template.id.0)
        .bind(&template.name)
        .bind(&template.body)
        .bind(&template.format)
        .bind(template.category.as_str())
        .bind(template.channel.map(NotifyChannel::as_str))
        .bind(Json(&template.provider_templates))
        .bind(template.card.as_ref().map(Json))
        .bind(template.updated_at)
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
        NotifyTemplateManagementRepository::get(self, organization_id, id)
            .await
            .map(|record| record.template())
    }

    async fn list(&self, organization_id: &Id) -> Result<Vec<NotifyTemplate>> {
        let records = NotifyTemplateManagementRepository::list(self, organization_id).await?;
        Ok(records.iter().map(NotifyTemplateRecord::template).collect())
    }
}
