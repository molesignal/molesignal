// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `scheduled_reports` + `report_deliveries` 表 Pg 实装（spec scheduled-reports）。
//!
//! 当前实装：CRUD + delivery 记录。真实渲染引擎（dashboard → SVG / PDF / PNG）
//! 与 cron 触发留 follow-up；本批次先把 schema + HTTP 落地，让 ops 能配置。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportRecipient {
    /// `email` | `webhook` | `s3`
    pub kind: String,
    /// email 地址 / webhook URL / s3 prefix
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledReport {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub dashboard_id: Option<Id>,
    pub saved_view_id: Option<Id>,
    pub cron: String,
    pub recipients: Vec<ReportRecipient>,
    /// `png` | `pdf` | `csv` | `svg` | `json`
    pub format: String,
    pub time_range_json: Value,
    pub enabled: bool,
    pub last_run_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Pending,
    Sent,
    Failed,
}
impl DeliveryStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Sent => "sent",
            Self::Failed => "failed",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "sent" => Self::Sent,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportDelivery {
    pub id: Id,
    pub report_id: Id,
    pub org_id: Id,
    pub status: DeliveryStatus,
    pub attempt: i32,
    pub recipient_kind: String,
    pub recipient_target: String,
    pub error: Option<String>,
    pub attempted_at: TimestampMicros,
}

#[async_trait]
pub trait ScheduledReportRepository: Send + Sync {
    async fn create(&self, r: ScheduledReport) -> Result<ScheduledReport>;
    async fn update(&self, r: ScheduledReport) -> Result<ScheduledReport>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<ScheduledReport>;
    async fn get_by_id(&self, id: &Id) -> Result<ScheduledReport>;
    async fn list(&self, org_id: &Id) -> Result<Vec<ScheduledReport>>;
    async fn list_enabled_all(&self) -> Result<Vec<ScheduledReport>>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;
    async fn touch_last_run(&self, id: &Id, ts: TimestampMicros) -> Result<()>;

    async fn record_delivery(&self, d: ReportDelivery) -> Result<()>;
    async fn list_deliveries(&self, org_id: &Id, report_id: &Id) -> Result<Vec<ReportDelivery>>;
}

pub struct PgScheduledReportRepository {
    pool: PgPool,
}

impl PgScheduledReportRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, name, dashboard_id, saved_view_id, cron, recipients_json,
                    format, time_range_json, enabled, last_run_at_micros,
                    created_at_micros, updated_at_micros";

fn row_to(r: sqlx::postgres::PgRow) -> ScheduledReport {
    let recipients: Json<Vec<ReportRecipient>> =
        r.try_get("recipients_json").unwrap_or(Json(Vec::new()));
    let time_range: Json<Value> = r.try_get("time_range_json").unwrap_or(Json(Value::Null));
    ScheduledReport {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        name: r.try_get::<String, _>("name").unwrap_or_default(),
        dashboard_id: r
            .try_get::<Option<String>, _>("dashboard_id")
            .unwrap_or_default()
            .map(Id),
        saved_view_id: r
            .try_get::<Option<String>, _>("saved_view_id")
            .unwrap_or_default()
            .map(Id),
        cron: r.try_get::<String, _>("cron").unwrap_or_default(),
        recipients: recipients.0,
        format: r.try_get::<String, _>("format").unwrap_or_default(),
        time_range_json: time_range.0,
        enabled: r.try_get::<bool, _>("enabled").unwrap_or(true),
        last_run_at: r
            .try_get::<Option<i64>, _>("last_run_at_micros")
            .unwrap_or_default()
            .map(TimestampMicros),
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
        updated_at: TimestampMicros(r.try_get::<i64, _>("updated_at_micros").unwrap_or_default()),
    }
}

fn delivery_row_to(r: sqlx::postgres::PgRow) -> ReportDelivery {
    ReportDelivery {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        report_id: Id(r.try_get::<String, _>("report_id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        status: DeliveryStatus::parse(&r.try_get::<String, _>("status").unwrap_or_default()),
        attempt: r.try_get::<i32, _>("attempt").unwrap_or(1),
        recipient_kind: r.try_get::<String, _>("recipient_kind").unwrap_or_default(),
        recipient_target: r
            .try_get::<String, _>("recipient_target")
            .unwrap_or_default(),
        error: r.try_get::<Option<String>, _>("error").unwrap_or_default(),
        attempted_at: TimestampMicros(
            r.try_get::<i64, _>("attempted_at_micros")
                .unwrap_or_default(),
        ),
    }
}

#[async_trait]
impl ScheduledReportRepository for PgScheduledReportRepository {
    async fn create(&self, r: ScheduledReport) -> Result<ScheduledReport> {
        sqlx::query(
            "INSERT INTO scheduled_reports
                (id, org_id, name, dashboard_id, saved_view_id, cron, recipients_json,
                 format, time_range_json, enabled, last_run_at_micros,
                 created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NULL, $11, $12)",
        )
        .bind(&r.id.0)
        .bind(&r.org_id.0)
        .bind(&r.name)
        .bind(r.dashboard_id.as_ref().map(|i| &i.0))
        .bind(r.saved_view_id.as_ref().map(|i| &i.0))
        .bind(&r.cron)
        .bind(Json(&r.recipients))
        .bind(&r.format)
        .bind(Json(&r.time_range_json))
        .bind(r.enabled)
        .bind(r.created_at.0)
        .bind(r.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(r)
    }

    async fn update(&self, r: ScheduledReport) -> Result<ScheduledReport> {
        sqlx::query(
            "UPDATE scheduled_reports SET
                name = $3, dashboard_id = $4, saved_view_id = $5, cron = $6,
                recipients_json = $7, format = $8, time_range_json = $9, enabled = $10,
                updated_at_micros = $11
             WHERE id = $1 AND org_id = $2",
        )
        .bind(&r.id.0)
        .bind(&r.org_id.0)
        .bind(&r.name)
        .bind(r.dashboard_id.as_ref().map(|i| &i.0))
        .bind(r.saved_view_id.as_ref().map(|i| &i.0))
        .bind(&r.cron)
        .bind(Json(&r.recipients))
        .bind(&r.format)
        .bind(Json(&r.time_range_json))
        .bind(r.enabled)
        .bind(r.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(r)
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<ScheduledReport> {
        let sql = format!("SELECT {COLS} FROM scheduled_reports WHERE org_id = $1 AND id = $2");
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row_to(row))
    }

    async fn get_by_id(&self, id: &Id) -> Result<ScheduledReport> {
        let sql = format!("SELECT {COLS} FROM scheduled_reports WHERE id = $1");
        let row = sqlx::query(&sql)
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row_to(row))
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<ScheduledReport>> {
        let sql = format!("SELECT {COLS} FROM scheduled_reports WHERE org_id = $1 ORDER BY name");
        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn list_enabled_all(&self) -> Result<Vec<ScheduledReport>> {
        let sql = format!(
            "SELECT {COLS} FROM scheduled_reports WHERE enabled = TRUE ORDER BY org_id, name"
        );
        let rows = sqlx::query(&sql)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM scheduled_reports WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }

    async fn touch_last_run(&self, id: &Id, ts: TimestampMicros) -> Result<()> {
        sqlx::query("UPDATE scheduled_reports SET last_run_at_micros = $2 WHERE id = $1")
            .bind(&id.0)
            .bind(ts.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }

    async fn record_delivery(&self, d: ReportDelivery) -> Result<()> {
        sqlx::query(
            "INSERT INTO report_deliveries
                (id, report_id, org_id, status, attempt, recipient_kind, recipient_target,
                 error, attempted_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(&d.id.0)
        .bind(&d.report_id.0)
        .bind(&d.org_id.0)
        .bind(d.status.as_str())
        .bind(d.attempt)
        .bind(&d.recipient_kind)
        .bind(&d.recipient_target)
        .bind(&d.error)
        .bind(d.attempted_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn list_deliveries(&self, org_id: &Id, report_id: &Id) -> Result<Vec<ReportDelivery>> {
        let rows = sqlx::query(
            "SELECT id, report_id, org_id, status, attempt, recipient_kind, recipient_target,
                    error, attempted_at_micros
             FROM report_deliveries
             WHERE org_id = $1 AND report_id = $2
             ORDER BY attempted_at_micros DESC
             LIMIT 500",
        )
        .bind(&org_id.0)
        .bind(&report_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(delivery_row_to).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivery_status_roundtrip() {
        for s in [
            DeliveryStatus::Pending,
            DeliveryStatus::Sent,
            DeliveryStatus::Failed,
        ] {
            assert_eq!(DeliveryStatus::parse(s.as_str()), s);
        }
    }
}
