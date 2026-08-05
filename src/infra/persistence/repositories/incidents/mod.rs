// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::alerting::{
        incident::{Incident, IncidentStatus, Severity, TriggeringQuery},
        repositories::IncidentRepository,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub mod groups;
pub mod rca;

pub struct PgIncidentRepository {
    pool: PgPool,
}

impl PgIncidentRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

pub(crate) fn status_to_str(s: IncidentStatus) -> &'static str {
    match s {
        IncidentStatus::Open => "open",
        IncidentStatus::Acknowledged => "acknowledged",
        IncidentStatus::Resolved => "resolved",
        IncidentStatus::Closed => "closed",
    }
}

pub(crate) fn status_from_str(s: &str) -> Result<IncidentStatus> {
    match s {
        "open" => Ok(IncidentStatus::Open),
        "acknowledged" => Ok(IncidentStatus::Acknowledged),
        "resolved" => Ok(IncidentStatus::Resolved),
        "closed" => Ok(IncidentStatus::Closed),
        other => Err(Error::internal(format!("unknown incident status: {other}"))),
    }
}

fn severity_to_str(s: Severity) -> &'static str {
    match s {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Error => "error",
        Severity::Critical => "critical",
    }
}

fn severity_from_str(s: &str) -> Result<Severity> {
    match s {
        "info" => Ok(Severity::Info),
        "warning" => Ok(Severity::Warning),
        "error" => Ok(Severity::Error),
        "critical" => Ok(Severity::Critical),
        other => Err(Error::internal(format!("unknown severity: {other}"))),
    }
}

const COLS: &str =
    "id, org_id, rule_id, escalation_policy_id, status, severity, summary, fingerprint,
     current_step, current_loop, current_step_started_at_micros, assignees,
     labels, annotations, trace_ids, host_ids, affected_services, triggering_query,
     created_at_micros, acknowledged_at_micros, acknowledged_by,
     resolved_at_micros, resolved_by";

fn row_to_incident(row: sqlx::postgres::PgRow) -> Result<Incident> {
    let status: String = row.try_get("status").map_err(sqlx_err)?;
    let severity: String = row.try_get("severity").map_err(sqlx_err)?;
    let assignees: Json<Vec<String>> = row.try_get("assignees").map_err(sqlx_err)?;
    let labels: Json<BTreeMap<String, String>> = row.try_get("labels").map_err(sqlx_err)?;
    let annotations: Json<BTreeMap<String, String>> =
        row.try_get("annotations").map_err(sqlx_err)?;
    let trace_ids: Json<Vec<String>> = row.try_get("trace_ids").map_err(sqlx_err)?;
    let host_ids: Json<Vec<String>> = row.try_get("host_ids").map_err(sqlx_err)?;
    let affected_services: Json<Vec<String>> =
        row.try_get("affected_services").map_err(sqlx_err)?;
    let triggering_query: Option<Json<TriggeringQuery>> =
        row.try_get("triggering_query").map_err(sqlx_err)?;
    let ack_at: Option<i64> = row.try_get("acknowledged_at_micros").map_err(sqlx_err)?;
    let ack_by: Option<String> = row.try_get("acknowledged_by").map_err(sqlx_err)?;
    let res_at: Option<i64> = row.try_get("resolved_at_micros").map_err(sqlx_err)?;
    let res_by: Option<String> = row.try_get("resolved_by").map_err(sqlx_err)?;
    let current_step: i32 = row.try_get("current_step").map_err(sqlx_err)?;
    let current_loop: i32 = row.try_get("current_loop").map_err(sqlx_err)?;
    Ok(Incident {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        rule_id: Id::from_string(row.try_get::<String, _>("rule_id").map_err(sqlx_err)?),
        escalation_policy_id: Id::from_string(
            row.try_get::<String, _>("escalation_policy_id")
                .map_err(sqlx_err)?,
        ),
        status: status_from_str(&status)?,
        severity: severity_from_str(&severity)?,
        summary: row.try_get("summary").map_err(sqlx_err)?,
        fingerprint: row.try_get("fingerprint").map_err(sqlx_err)?,
        current_step: current_step as u32,
        current_loop: current_loop as u32,
        current_step_started_at: TimestampMicros(
            row.try_get("current_step_started_at_micros")
                .map_err(sqlx_err)?,
        ),
        assignees: assignees.0.into_iter().map(Id::from_string).collect(),
        labels: labels.0,
        annotations: annotations.0,
        trace_ids: trace_ids.0,
        host_ids: host_ids.0,
        affected_services: affected_services.0,
        triggering_query: triggering_query.map(|j| j.0),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        acknowledged_at: ack_at.map(TimestampMicros),
        acknowledged_by: ack_by.map(Id::from_string),
        resolved_at: res_at.map(TimestampMicros),
        resolved_by: res_by.map(Id::from_string),
    })
}

#[async_trait]
impl IncidentRepository for PgIncidentRepository {
    async fn create(&self, i: Incident) -> Result<Incident> {
        let assignees: Vec<String> = i.assignees.iter().map(|x| x.0.clone()).collect();
        sqlx::query(
            "INSERT INTO incidents
             (id, org_id, rule_id, escalation_policy_id, status, severity, summary, fingerprint,
              current_step, current_loop, current_step_started_at_micros, assignees,
              labels, annotations, trace_ids, host_ids, affected_services, triggering_query,
              created_at_micros, acknowledged_at_micros, acknowledged_by,
              resolved_at_micros, resolved_by)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23)",
        )
        .bind(&i.id.0)
        .bind(&i.org_id.0)
        .bind(&i.rule_id.0)
        .bind(&i.escalation_policy_id.0)
        .bind(status_to_str(i.status))
        .bind(severity_to_str(i.severity))
        .bind(&i.summary)
        .bind(&i.fingerprint)
        .bind(i.current_step as i32)
        .bind(i.current_loop as i32)
        .bind(i.current_step_started_at.0)
        .bind(Json(&assignees))
        .bind(Json(&i.labels))
        .bind(Json(&i.annotations))
        .bind(Json(&i.trace_ids))
        .bind(Json(&i.host_ids))
        .bind(Json(&i.affected_services))
        .bind(i.triggering_query.as_ref().map(Json))
        .bind(i.created_at.0)
        .bind(i.acknowledged_at.map(|t| t.0))
        .bind(i.acknowledged_by.as_ref().map(|x| &x.0))
        .bind(i.resolved_at.map(|t| t.0))
        .bind(i.resolved_by.as_ref().map(|x| &x.0))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(i)
    }

    async fn update(&self, i: Incident) -> Result<Incident> {
        let assignees: Vec<String> = i.assignees.iter().map(|x| x.0.clone()).collect();
        sqlx::query(
            "UPDATE incidents SET
                status = $2, severity = $3, summary = $4, fingerprint = $5,
                current_step = $6, current_step_started_at_micros = $7, assignees = $8,
                acknowledged_at_micros = $9, acknowledged_by = $10,
                resolved_at_micros = $11, resolved_by = $12,
                escalation_policy_id = $13,
                labels = $14, annotations = $15,
                trace_ids = $16, host_ids = $17, affected_services = $18,
                triggering_query = $19, current_loop = $20
             WHERE id = $1",
        )
        .bind(&i.id.0)
        .bind(status_to_str(i.status))
        .bind(severity_to_str(i.severity))
        .bind(&i.summary)
        .bind(&i.fingerprint)
        .bind(i.current_step as i32)
        .bind(i.current_step_started_at.0)
        .bind(Json(&assignees))
        .bind(i.acknowledged_at.map(|t| t.0))
        .bind(i.acknowledged_by.as_ref().map(|x| &x.0))
        .bind(i.resolved_at.map(|t| t.0))
        .bind(i.resolved_by.as_ref().map(|x| &x.0))
        .bind(&i.escalation_policy_id.0)
        .bind(Json(&i.labels))
        .bind(Json(&i.annotations))
        .bind(Json(&i.trace_ids))
        .bind(Json(&i.host_ids))
        .bind(Json(&i.affected_services))
        .bind(i.triggering_query.as_ref().map(Json))
        .bind(i.current_loop as i32)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(i)
    }

    async fn get(&self, id: &Id) -> Result<Incident> {
        let row = sqlx::query(&format!("SELECT {COLS} FROM incidents WHERE id = $1"))
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        row_to_incident(row)
    }

    async fn list_active(&self, org_id: &Id) -> Result<Vec<Incident>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM incidents
             WHERE org_id = $1 AND status IN ('open', 'acknowledged')"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_incident).collect()
    }

    async fn find_by_fingerprint(
        &self,
        org_id: &Id,
        fingerprint: &str,
    ) -> Result<Option<Incident>> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM incidents WHERE org_id = $1 AND fingerprint = $2"
        ))
        .bind(&org_id.0)
        .bind(fingerprint)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row.map(row_to_incident).transpose()
    }

    async fn list_by_status(&self, org_id: &Id, status: IncidentStatus) -> Result<Vec<Incident>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM incidents WHERE org_id = $1 AND status = $2"
        ))
        .bind(&org_id.0)
        .bind(status_to_str(status))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_incident).collect()
    }

    async fn list_since(
        &self,
        org_id: &Id,
        since: crate::shared::time::TimestampMicros,
    ) -> Result<Vec<Incident>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM incidents
             WHERE org_id = $1 AND created_at_micros >= $2"
        ))
        .bind(&org_id.0)
        .bind(since.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_incident).collect()
    }
}
