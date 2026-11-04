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
     last_eval_at, last_state, created_at, updated_at,
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
    let last_eval_at: Option<i64> = row
        .try_get::<Option<TimestampMicros>, _>("last_eval_at")
        .map_err(sqlx_err)?
        .map(|t| t.0);
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
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
    })
}

fn decode_alert_query(raw: Value, rule_id: &Id) -> Result<AlertQuery> {
    serde_json::from_value::<AlertQuery>(raw)
        .map_err(|error| Error::internal(format!("alert rule {rule_id} query decode: {error}")))
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

/// 写入一条新规则；规则本身和事务里的批量写入都用它。
async fn insert_rule<'e, E: sqlx::PgExecutor<'e>>(executor: E, r: &AlertRule) -> Result<()> {
    sqlx::query(
        "INSERT INTO alert_rules
         (id, org_id, name, description, enabled, kind, query, trigger,
          anomaly_params_json, escalation_policy_id, labels, annotations,
          last_eval_at, last_state, created_at, updated_at,
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
    .bind(r.last_eval_at)
    .bind(Json(&r.last_state))
    .bind(r.created_at)
    .bind(r.updated_at)
    .bind(Json(&r.thresholds))
    .bind(r.severity.map(|s| s.as_str()))
    .execute(executor)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

/// 写回一条规则的定义。
async fn update_rule<'e, E: sqlx::PgExecutor<'e>>(executor: E, r: &AlertRule) -> Result<()> {
    sqlx::query(
        "UPDATE alert_rules SET
           name = $2, description = $3, enabled = $4, kind = $5, query = $6,
           trigger = $7, anomaly_params_json = $8, escalation_policy_id = $9,
           labels = $10, annotations = $11, updated_at = $12,
           thresholds = $13, severity = $14
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
    .bind(r.updated_at)
    .bind(Json(&r.thresholds))
    .bind(r.severity.map(|s| s.as_str()))
    .execute(executor)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

#[async_trait]
impl AlertRuleRepository for PgAlertRuleRepository {
    async fn create(&self, r: AlertRule) -> Result<AlertRule> {
        insert_rule(&self.pool, &r).await?;
        Ok(r)
    }

    async fn update(&self, r: AlertRule) -> Result<AlertRule> {
        update_rule(&self.pool, &r).await?;
        Ok(r)
    }

    async fn save_all(&self, creates: Vec<AlertRule>, updates: Vec<AlertRule>) -> Result<()> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        for rule in &creates {
            insert_rule(&mut *tx, rule).await?;
        }

        for rule in &updates {
            update_rule(&mut *tx, rule).await?;
        }

        tx.commit().await.map_err(sqlx_err)
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

    async fn record_evaluation(
        &self,
        id: &Id,
        evaluated_at: TimestampMicros,
        state: RuleState,
    ) -> Result<()> {
        sqlx::query("UPDATE alert_rules SET last_eval_at = $2, last_state = $3 WHERE id = $1")
            .bind(&id.0)
            .bind(evaluated_at)
            .bind(Json(&state))
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn malformed_stream_type_placeholder_is_rejected() {
        let error = decode_alert_query(
            json!({
                "language": "sql",
                "statement": "SELECT COUNT(*) FROM app_logs",
                "periodSecs": 60,
                "stream": { "name": "app_logs", "streamType": "<logs|metrics|traces>" }
            }),
            &Id::from_string("rule-1"),
        )
        .expect_err("placeholder compatibility is intentionally removed");

        assert!(error.to_string().contains("query decode"));
    }

    #[test]
    fn open_stream_type_is_preserved_for_registry_validation() {
        let query = decode_alert_query(
            json!({
                "language": "sql",
                "statement": "SELECT COUNT(*) FROM app_logs",
                "periodSecs": 60,
                "stream": { "name": "app_logs", "streamType": "bogus" }
            }),
            &Id::from_string("rule-2"),
        )
        .expect("syntactically valid open type ids decode");

        assert_eq!(query.stream.unwrap().stream_type.as_str(), "bogus");
    }
}
