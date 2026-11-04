// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::alerting::{
        escalation::{EscalationPolicy, EscalationStep},
        repositories::EscalationPolicyRepository,
        urgency::UrgencyRule,
    },
    shared::{Result, ids::Id},
};

pub struct PgEscalationPolicyRepository {
    pool: PgPool,
}

impl PgEscalationPolicyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = r#"id, org_id, name, steps, "repeat", max_loops, ack_retrigger_secs, urgency_rule,
    escalate_low_urgency"#;

fn row_to_policy(row: sqlx::postgres::PgRow) -> Result<EscalationPolicy> {
    let steps: Json<Vec<EscalationStep>> = row.try_get("steps").map_err(sqlx_err)?;
    let max_loops: i32 = row.try_get("max_loops").map_err(sqlx_err)?;
    let ack_retrigger_secs: Option<i32> = row.try_get("ack_retrigger_secs").map_err(sqlx_err)?;
    let urgency_rule: Json<UrgencyRule> = row.try_get("urgency_rule").map_err(sqlx_err)?;

    Ok(EscalationPolicy {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        steps: steps.0,
        repeat: row.try_get("repeat").map_err(sqlx_err)?,
        max_loops: max_loops as u32,
        ack_retrigger_secs: ack_retrigger_secs.and_then(|secs| u32::try_from(secs).ok()),
        urgency_rule: urgency_rule.0,
        escalate_low_urgency: row.try_get("escalate_low_urgency").map_err(sqlx_err)?,
    })
}

#[async_trait]
impl EscalationPolicyRepository for PgEscalationPolicyRepository {
    async fn create(&self, p: EscalationPolicy) -> Result<EscalationPolicy> {
        sqlx::query(
            r#"INSERT INTO escalation_policies
               (id, org_id, name, steps, "repeat", max_loops, ack_retrigger_secs, urgency_rule,
                escalate_low_urgency)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
        )
        .bind(&p.id.0)
        .bind(&p.org_id.0)
        .bind(&p.name)
        .bind(Json(&p.steps))
        .bind(p.repeat)
        .bind(p.max_loops as i32)
        .bind(p.ack_retrigger_secs.map(|secs| secs as i32))
        .bind(Json(&p.urgency_rule))
        .bind(p.escalate_low_urgency)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(p)
    }

    async fn update(&self, p: EscalationPolicy) -> Result<EscalationPolicy> {
        sqlx::query(
            r#"UPDATE escalation_policies SET
                 name = $2, steps = $3, "repeat" = $4, max_loops = $5, ack_retrigger_secs = $6,
                 urgency_rule = $7, escalate_low_urgency = $8
               WHERE id = $1"#,
        )
        .bind(&p.id.0)
        .bind(&p.name)
        .bind(Json(&p.steps))
        .bind(p.repeat)
        .bind(p.max_loops as i32)
        .bind(p.ack_retrigger_secs.map(|secs| secs as i32))
        .bind(Json(&p.urgency_rule))
        .bind(p.escalate_low_urgency)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(p)
    }

    async fn get(&self, id: &Id) -> Result<EscalationPolicy> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM escalation_policies WHERE id = $1"
        ))
        .bind(&id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row_to_policy(row)
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<EscalationPolicy>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM escalation_policies WHERE org_id = $1"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to_policy).collect()
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM escalation_policies WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }
}
