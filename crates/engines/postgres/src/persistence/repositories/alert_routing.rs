// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `alert_routing_rules`（路由规则）与 `alert_team_policies`（团队的值班策略）的 Pg 实装。

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::alerting::{
        incident::Severity,
        priority::Priority,
        routing::{
            IncidentOrigin, LabelCondition, RoutingRule, RoutingRuleRepository, RoutingTarget,
            TeamPolicy, TeamPolicyRepository,
        },
        time_window::TimeCondition,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgRoutingRuleRepository {
    pool: PgPool,
}

impl PgRoutingRuleRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, name, enabled, position, conditions, min_severity, origins,
     time_condition, priorities, target, created_by, created_at, updated_at";

fn row_to(row: sqlx::postgres::PgRow) -> Result<RoutingRule> {
    let conditions: Json<Vec<LabelCondition>> = row.try_get("conditions").map_err(sqlx_err)?;
    let origins: Json<Vec<IncidentOrigin>> = row.try_get("origins").map_err(sqlx_err)?;
    let target: Json<RoutingTarget> = row.try_get("target").map_err(sqlx_err)?;
    let min_severity: Option<String> = row.try_get("min_severity").map_err(sqlx_err)?;
    let time: Option<Json<TimeCondition>> = row.try_get("time_condition").map_err(sqlx_err)?;
    let priorities: Json<Vec<Priority>> = row.try_get("priorities").map_err(sqlx_err)?;

    Ok(RoutingRule {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        position: row.try_get("position").map_err(sqlx_err)?,
        conditions: conditions.0,
        min_severity: min_severity
            .map(|word| {
                Severity::from_label(&word)
                    .ok_or_else(|| Error::internal(format!("unknown severity `{word}`")))
            })
            .transpose()?,
        origins: origins.0,
        time: time.map(|time| time.0),
        priorities: priorities.0,
        target: target.0,
        created_by: row
            .try_get::<Option<String>, _>("created_by")
            .map_err(sqlx_err)?
            .map(Id::from_string),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl RoutingRuleRepository for PgRoutingRuleRepository {
    async fn create(&self, rule: RoutingRule) -> Result<RoutingRule> {
        self.upsert(rule).await
    }

    async fn update(&self, rule: RoutingRule) -> Result<RoutingRule> {
        self.upsert(rule).await
    }

    async fn upsert(&self, rule: RoutingRule) -> Result<RoutingRule> {
        sqlx::query(
            "INSERT INTO alert_routing_rules
               (id, org_id, name, enabled, position, conditions, min_severity, origins, target,
                created_by, created_at, updated_at, time_condition, priorities)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
             ON CONFLICT (id) DO UPDATE SET
               name = EXCLUDED.name, enabled = EXCLUDED.enabled, position = EXCLUDED.position,
               conditions = EXCLUDED.conditions, min_severity = EXCLUDED.min_severity,
               origins = EXCLUDED.origins, target = EXCLUDED.target,
               updated_at = EXCLUDED.updated_at,
               time_condition = EXCLUDED.time_condition, priorities = EXCLUDED.priorities",
        )
        .bind(&rule.id.0)
        .bind(&rule.org_id.0)
        .bind(&rule.name)
        .bind(rule.enabled)
        .bind(rule.position)
        .bind(Json(&rule.conditions))
        .bind(rule.min_severity.map(|severity| severity.as_str()))
        .bind(Json(&rule.origins))
        .bind(Json(&rule.target))
        .bind(rule.created_by.as_ref().map(|id| id.0.clone()))
        .bind(rule.created_at)
        .bind(rule.updated_at)
        .bind(rule.time.as_ref().map(Json))
        .bind(Json(&rule.priorities))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(rule)
    }

    async fn get(&self, id: &Id) -> Result<RoutingRule> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM alert_routing_rules WHERE id = $1"
        ))
        .bind(&id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row_to(row)
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<RoutingRule>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM alert_routing_rules WHERE org_id = $1
             ORDER BY position, created_at, id"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to).collect()
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM alert_routing_rules WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }

    async fn reorder(&self, org_id: &Id, ids: &[Id]) -> Result<Vec<RoutingRule>> {
        let current = self.list(org_id).await?;
        let listed: Vec<String> = ids.iter().map(|id| id.0.clone()).collect();

        // 列出的按给出的顺序在前，没列出的保持原来的先后排在后面。
        let ordered: Vec<String> = listed
            .iter()
            .filter(|id| current.iter().any(|rule| rule.id.0 == **id))
            .cloned()
            .chain(
                current
                    .iter()
                    .map(|rule| rule.id.0.clone())
                    .filter(|id| !listed.contains(id)),
            )
            .collect();

        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        for (position, id) in ordered.iter().enumerate() {
            sqlx::query(
                "UPDATE alert_routing_rules SET position = $3 WHERE org_id = $1 AND id = $2",
            )
            .bind(&org_id.0)
            .bind(id)
            .bind(position as i32)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }

        tx.commit().await.map_err(sqlx_err)?;

        self.list(org_id).await
    }
}

pub struct PgTeamPolicyRepository {
    pool: PgPool,
}

impl PgTeamPolicyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TeamPolicyRepository for PgTeamPolicyRepository {
    async fn list(&self, org_id: &Id) -> Result<Vec<TeamPolicy>> {
        let rows = sqlx::query(
            "SELECT org_id, team_id, escalation_policy_id, updated_by, updated_at
             FROM alert_team_policies WHERE org_id = $1 ORDER BY team_id",
        )
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                Ok(TeamPolicy {
                    org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
                    team_id: Id::from_string(
                        row.try_get::<String, _>("team_id").map_err(sqlx_err)?,
                    ),
                    escalation_policy_id: Id::from_string(
                        row.try_get::<String, _>("escalation_policy_id")
                            .map_err(sqlx_err)?,
                    ),
                    updated_by: row
                        .try_get::<Option<String>, _>("updated_by")
                        .map_err(sqlx_err)?
                        .map(Id::from_string),
                    updated_at: row
                        .try_get::<TimestampMicros, _>("updated_at")
                        .map_err(sqlx_err)?,
                })
            })
            .collect()
    }

    async fn upsert(&self, policy: TeamPolicy) -> Result<TeamPolicy> {
        sqlx::query(
            "INSERT INTO alert_team_policies
               (org_id, team_id, escalation_policy_id, updated_by, updated_at)
             VALUES ($1,$2,$3,$4,$5)
             ON CONFLICT (org_id, team_id) DO UPDATE SET
               escalation_policy_id = EXCLUDED.escalation_policy_id,
               updated_by = EXCLUDED.updated_by,
               updated_at = EXCLUDED.updated_at",
        )
        .bind(&policy.org_id.0)
        .bind(&policy.team_id.0)
        .bind(&policy.escalation_policy_id.0)
        .bind(policy.updated_by.as_ref().map(|id| id.0.clone()))
        .bind(policy.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(policy)
    }

    async fn delete(&self, org_id: &Id, team_id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM alert_team_policies WHERE org_id = $1 AND team_id = $2")
            .bind(&org_id.0)
            .bind(&team_id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }
}
