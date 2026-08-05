// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::alerting::{
        anomaly::AnomalyParams,
        incident::Severity,
        repositories::AlertRuleRepository,
        rule::{AlertQuery, AlertRule, AlertRuleKind, AlertTrigger, RuleState, SeverityThreshold},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub mod evaluation_state;

pub struct PgAlertRuleRepository {
    pool: PgPool,
}

impl PgAlertRuleRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, name, description, enabled, kind, query, trigger,
     anomaly_params_json, escalation_policy_id, labels, annotations,
     last_eval_at_micros, last_state, created_at_micros, updated_at_micros,
     thresholds, severity";

fn row_to_rule(row: sqlx::postgres::PgRow) -> Result<AlertRule> {
    let id = Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?);
    let query = decode_alert_query(row.try_get("query").map_err(sqlx_err)?, &id)?;
    let trigger: Json<AlertTrigger> = row.try_get("trigger").map_err(sqlx_err)?;
    let labels: Json<std::collections::BTreeMap<String, String>> =
        row.try_get("labels").map_err(sqlx_err)?;
    let annotations: Json<std::collections::BTreeMap<String, String>> =
        row.try_get("annotations").map_err(sqlx_err)?;
    let last_state: Json<RuleState> = row.try_get("last_state").map_err(sqlx_err)?;
    let last_eval_at: Option<i64> = row.try_get("last_eval_at_micros").map_err(sqlx_err)?;
    let kind = AlertRuleKind::parse(&row.try_get::<String, _>("kind").map_err(sqlx_err)?);
    let anomaly_params: Option<Json<AnomalyParams>> =
        row.try_get("anomaly_params_json").map_err(sqlx_err)?;
    let thresholds: Json<Vec<SeverityThreshold>> = row
        .try_get("thresholds")
        .unwrap_or_else(|_| Json(Vec::new()));
    let severity = row
        .try_get::<Option<String>, _>("severity")
        .map_err(sqlx_err)?
        .and_then(|s| Severity::from_label(&s));
    Ok(AlertRule {
        id,
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        description: row.try_get("description").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        kind,
        query,
        trigger: trigger.0,
        anomaly_params: anomaly_params.map(|j| j.0),
        thresholds: thresholds.0,
        severity,
        escalation_policy_id: Id::from_string(
            row.try_get::<String, _>("escalation_policy_id")
                .map_err(sqlx_err)?,
        ),
        labels: labels.0,
        annotations: annotations.0,
        last_eval_at: last_eval_at.map(TimestampMicros),
        last_state: last_state.0,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

fn decode_alert_query(raw: Value, rule_id: &Id) -> Result<AlertQuery> {
    match serde_json::from_value::<AlertQuery>(raw.clone()) {
        Ok(query) => Ok(query),
        Err(err) if has_legacy_stream_type_placeholder(&raw) => {
            tracing::warn!(
                rule_id = %rule_id,
                error = %err,
                "alert rule query contains legacy stream_type placeholder; treating stream as unset"
            );
            let mut sanitized = raw;
            if let Some(obj) = sanitized.as_object_mut() {
                obj.remove("stream");
            }
            serde_json::from_value::<AlertQuery>(sanitized)
                .map_err(|e| Error::internal(format!("alert rule query decode after cleanup: {e}")))
        }
        Err(err) => Err(Error::internal(format!("alert rule query decode: {err}"))),
    }
}

fn has_legacy_stream_type_placeholder(raw: &Value) -> bool {
    raw.get("stream")
        .and_then(|stream| stream.get("stream_type"))
        .and_then(Value::as_str)
        == Some("<logs|metrics|traces>")
}

fn collect_valid_rules(rows: Vec<sqlx::postgres::PgRow>, context: &str) -> Vec<AlertRule> {
    rows.into_iter()
        .filter_map(|row| match row_to_rule(row) {
            Ok(rule) => Some(rule),
            Err(e) => {
                tracing::warn!(error = %e, context, "skipping malformed alert rule row");
                None
            }
        })
        .collect()
}

#[async_trait]
impl AlertRuleRepository for PgAlertRuleRepository {
    async fn create(&self, r: AlertRule) -> Result<AlertRule> {
        sqlx::query(
            "INSERT INTO alert_rules
             (id, org_id, name, description, enabled, kind, query, trigger,
              anomaly_params_json, escalation_policy_id, labels, annotations,
              last_eval_at_micros, last_state, created_at_micros, updated_at_micros,
              thresholds, severity)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)",
        )
        .bind(&r.id.0)
        .bind(&r.org_id.0)
        .bind(&r.name)
        .bind(&r.description)
        .bind(r.enabled)
        .bind(r.kind.as_str())
        .bind(Json(&r.query))
        .bind(Json(&r.trigger))
        .bind(r.anomaly_params.as_ref().map(Json))
        .bind(&r.escalation_policy_id.0)
        .bind(Json(&r.labels))
        .bind(Json(&r.annotations))
        .bind(r.last_eval_at.map(|t| t.0))
        .bind(Json(&r.last_state))
        .bind(r.created_at.0)
        .bind(r.updated_at.0)
        .bind(Json(&r.thresholds))
        .bind(r.severity.map(|s| s.as_str()))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(r)
    }

    async fn update(&self, r: AlertRule) -> Result<AlertRule> {
        sqlx::query(
            "UPDATE alert_rules SET
               name = $2, description = $3, enabled = $4, kind = $5, query = $6,
               trigger = $7, anomaly_params_json = $8, escalation_policy_id = $9,
               labels = $10, annotations = $11, last_eval_at_micros = $12,
               last_state = $13, updated_at_micros = $14,
               thresholds = $15, severity = $16
             WHERE id = $1",
        )
        .bind(&r.id.0)
        .bind(&r.name)
        .bind(&r.description)
        .bind(r.enabled)
        .bind(r.kind.as_str())
        .bind(Json(&r.query))
        .bind(Json(&r.trigger))
        .bind(r.anomaly_params.as_ref().map(Json))
        .bind(&r.escalation_policy_id.0)
        .bind(Json(&r.labels))
        .bind(Json(&r.annotations))
        .bind(r.last_eval_at.map(|t| t.0))
        .bind(Json(&r.last_state))
        .bind(r.updated_at.0)
        .bind(Json(&r.thresholds))
        .bind(r.severity.map(|s| s.as_str()))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(r)
    }

    async fn get(&self, id: &Id) -> Result<AlertRule> {
        let row = sqlx::query(&format!("SELECT {COLS} FROM alert_rules WHERE id = $1"))
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        row_to_rule(row)
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<AlertRule>> {
        let rows = sqlx::query(&format!("SELECT {COLS} FROM alert_rules WHERE org_id = $1"))
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(collect_valid_rules(rows, "list"))
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM alert_rules WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }

    async fn list_enabled(&self) -> Result<Vec<AlertRule>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM alert_rules WHERE enabled = TRUE"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(collect_valid_rules(rows, "list_enabled"))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn legacy_stream_type_placeholder_is_treated_as_missing_stream() {
        let query = decode_alert_query(
            json!({
                "language": "sql",
                "statement": "SELECT COUNT(*) FROM app_logs",
                "period_secs": 60,
                "stream": { "name": "app_logs", "stream_type": "<logs|metrics|traces>" }
            }),
            &Id::from_string("rule-1"),
        )
        .expect("legacy placeholder should not poison repository reads");

        assert!(query.stream.is_none());
        assert_eq!(query.statement, "SELECT COUNT(*) FROM app_logs");
    }

    #[test]
    fn unknown_stream_type_still_fails_decode() {
        let err = decode_alert_query(
            json!({
                "language": "sql",
                "statement": "SELECT COUNT(*) FROM app_logs",
                "period_secs": 60,
                "stream": { "name": "app_logs", "stream_type": "bogus" }
            }),
            &Id::from_string("rule-2"),
        )
        .expect_err("non-placeholder invalid stream type must remain invalid");

        assert!(err.to_string().contains("alert rule query decode"));
    }
}
