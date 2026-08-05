// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence 调查、自动化、审批、执行与 Agent Profile 的 Postgres 实现。

use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    intelligence::model::{
        AgentProfile, ApprovalRequest, ApprovalStatus, Automation, Execution,
        IntelligenceRepository, Investigation, InvestigationDetail, InvestigationEvidence,
        InvestigationHypothesis, InvestigationStep, NetworkAccess, ToolCallRecord,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub mod chat_archives;
pub mod chats;
pub mod model_providers;
pub mod prompts;
pub mod tool_control;
pub mod toolsets;

pub struct PgIntelligenceRepository {
    pool: PgPool,
}

impl PgIntelligenceRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn enum_string(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_default()
}

fn parse_enum<T: DeserializeOwned>(value: String, field: &str) -> Result<T> {
    serde_json::from_value(Value::String(value))
        .map_err(|error| Error::internal(format!("invalid {field} in database: {error}")))
}

fn optional_ts(value: Option<i64>) -> Option<TimestampMicros> {
    value.map(TimestampMicros)
}

fn investigation_row(row: sqlx::postgres::PgRow) -> Result<Investigation> {
    let context: Json<Value> = row.try_get("context").map_err(sqlx_err)?;
    let confidence = row
        .try_get::<Option<String>, _>("confidence")
        .map_err(sqlx_err)?
        .map(|value| parse_enum(value, "investigation confidence"))
        .transpose()?;
    Ok(Investigation {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        created_by: Id(row.try_get("created_by").map_err(sqlx_err)?),
        chat_id: row
            .try_get::<Option<String>, _>("chat_id")
            .map_err(sqlx_err)?
            .map(Id),
        title: row.try_get("title").map_err(sqlx_err)?,
        status: parse_enum(
            row.try_get("status").map_err(sqlx_err)?,
            "investigation status",
        )?,
        context: context.0,
        summary: row.try_get("summary").map_err(sqlx_err)?,
        confidence,
        current_step: row.try_get("current_step").map_err(sqlx_err)?,
        started_at: optional_ts(row.try_get("started_at_micros").map_err(sqlx_err)?),
        completed_at: optional_ts(row.try_get("completed_at_micros").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

fn step_row(row: sqlx::postgres::PgRow) -> Result<InvestigationStep> {
    let input: Json<Value> = row.try_get("input").map_err(sqlx_err)?;
    Ok(InvestigationStep {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        investigation_id: Id(row.try_get("investigation_id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        position: row.try_get("position").map_err(sqlx_err)?,
        title: row.try_get("title").map_err(sqlx_err)?,
        status: parse_enum(
            row.try_get("status").map_err(sqlx_err)?,
            "investigation step status",
        )?,
        tool_name: row.try_get("tool_name").map_err(sqlx_err)?,
        input: input.0,
        output_summary: row.try_get("output_summary").map_err(sqlx_err)?,
        conclusion_impact: row.try_get("conclusion_impact").map_err(sqlx_err)?,
        error: row.try_get("error").map_err(sqlx_err)?,
        started_at: optional_ts(row.try_get("started_at_micros").map_err(sqlx_err)?),
        ended_at: optional_ts(row.try_get("ended_at_micros").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
    })
}

fn evidence_row(row: sqlx::postgres::PgRow) -> Result<InvestigationEvidence> {
    let source_ref: Json<Value> = row.try_get("source_ref").map_err(sqlx_err)?;
    let parameters: Json<Value> = row.try_get("parameters").map_err(sqlx_err)?;
    Ok(InvestigationEvidence {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        investigation_id: Id(row.try_get("investigation_id").map_err(sqlx_err)?),
        step_id: row
            .try_get::<Option<String>, _>("step_id")
            .map_err(sqlx_err)?
            .map(Id),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        kind: row.try_get("kind").map_err(sqlx_err)?,
        label: row.try_get("label").map_err(sqlx_err)?,
        fact_status: parse_enum(
            row.try_get("fact_status").map_err(sqlx_err)?,
            "evidence fact status",
        )?,
        source_ref: source_ref.0,
        query: row.try_get("query").map_err(sqlx_err)?,
        parameters: parameters.0,
        summary: row.try_get("summary").map_err(sqlx_err)?,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
    })
}

fn hypothesis_row(row: sqlx::postgres::PgRow) -> Result<InvestigationHypothesis> {
    let evidence_ids: Json<Vec<Id>> = row.try_get("evidence_ids").map_err(sqlx_err)?;
    Ok(InvestigationHypothesis {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        investigation_id: Id(row.try_get("investigation_id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        statement: row.try_get("statement").map_err(sqlx_err)?,
        confidence: parse_enum(
            row.try_get("confidence").map_err(sqlx_err)?,
            "hypothesis confidence",
        )?,
        status: parse_enum(
            row.try_get("status").map_err(sqlx_err)?,
            "hypothesis status",
        )?,
        evidence_ids: evidence_ids.0,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

fn automation_row(row: sqlx::postgres::PgRow) -> Result<Automation> {
    let trigger: Json<Value> = row.try_get("trigger").map_err(sqlx_err)?;
    let input_context: Json<Value> = row.try_get("input_context").map_err(sqlx_err)?;
    let steps: Json<Value> = row.try_get("steps").map_err(sqlx_err)?;
    let allowed_tools: Json<Vec<String>> = row.try_get("allowed_tools").map_err(sqlx_err)?;
    let approval_policy: Json<Value> = row.try_get("approval_policy").map_err(sqlx_err)?;
    let output_actions: Json<Value> = row.try_get("output_actions").map_err(sqlx_err)?;
    let failure_policy: Json<Value> = row.try_get("failure_policy").map_err(sqlx_err)?;
    let notification: Json<Value> = row.try_get("notification").map_err(sqlx_err)?;
    Ok(Automation {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        description: row.try_get("description").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        trigger: trigger.0,
        input_context: input_context.0,
        steps: steps.0,
        allowed_tools: allowed_tools.0,
        approval_policy: approval_policy.0,
        output_actions: output_actions.0,
        failure_policy: failure_policy.0,
        notification: notification.0,
        created_by: Id(row.try_get("created_by").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

fn approval_row(row: sqlx::postgres::PgRow) -> Result<ApprovalRequest> {
    let parameters: Json<Value> = row.try_get("parameters").map_err(sqlx_err)?;
    let reviews: Json<Value> = row.try_get("reviews").map_err(sqlx_err)?;
    Ok(ApprovalRequest {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        investigation_id: row
            .try_get::<Option<String>, _>("investigation_id")
            .map_err(sqlx_err)?
            .map(Id),
        action: row.try_get("action").map_err(sqlx_err)?,
        target: row.try_get("target").map_err(sqlx_err)?,
        parameters: parameters.0,
        reason: row.try_get("reason").map_err(sqlx_err)?,
        impact: row.try_get("impact").map_err(sqlx_err)?,
        risk: parse_enum(row.try_get("risk").map_err(sqlx_err)?, "approval risk")?,
        status: parse_enum(row.try_get("status").map_err(sqlx_err)?, "approval status")?,
        requested_by: Id(row.try_get("requested_by").map_err(sqlx_err)?),
        required_approvals: row.try_get("required_approvals").map_err(sqlx_err)?,
        reviews: reviews.0,
        expires_at: optional_ts(row.try_get("expires_at_micros").map_err(sqlx_err)?),
        decided_at: optional_ts(row.try_get("decided_at_micros").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

fn execution_row(row: sqlx::postgres::PgRow) -> Result<Execution> {
    let parameters: Json<Value> = row.try_get("parameters").map_err(sqlx_err)?;
    let approved_by: Json<Vec<Id>> = row.try_get("approved_by").map_err(sqlx_err)?;
    let verification: Json<Value> = row.try_get("verification").map_err(sqlx_err)?;
    Ok(Execution {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        approval_request_id: Id(row.try_get("approval_request_id").map_err(sqlx_err)?),
        investigation_id: row
            .try_get::<Option<String>, _>("investigation_id")
            .map_err(sqlx_err)?
            .map(Id),
        action: row.try_get("action").map_err(sqlx_err)?,
        target: row.try_get("target").map_err(sqlx_err)?,
        parameters: parameters.0,
        idempotency_key: row.try_get("idempotency_key").map_err(sqlx_err)?,
        requested_by: Id(row.try_get("requested_by").map_err(sqlx_err)?),
        approved_by: approved_by.0,
        status: parse_enum(row.try_get("status").map_err(sqlx_err)?, "execution status")?,
        output_summary: row.try_get("output_summary").map_err(sqlx_err)?,
        error: row.try_get("error").map_err(sqlx_err)?,
        verification: verification.0,
        started_at: optional_ts(row.try_get("started_at_micros").map_err(sqlx_err)?),
        finished_at: optional_ts(row.try_get("finished_at_micros").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

fn tool_call_row(row: sqlx::postgres::PgRow) -> Result<ToolCallRecord> {
    let input: Json<Value> = row.try_get("input").map_err(sqlx_err)?;
    let policy_decision: Json<Value> = row.try_get("policy_decision").map_err(sqlx_err)?;
    Ok(ToolCallRecord {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        chat_id: row
            .try_get::<Option<String>, _>("chat_id")
            .map_err(sqlx_err)?
            .map(Id),
        investigation_id: row
            .try_get::<Option<String>, _>("investigation_id")
            .map_err(sqlx_err)?
            .map(Id),
        step_id: row
            .try_get::<Option<String>, _>("step_id")
            .map_err(sqlx_err)?
            .map(Id),
        tool_name: row.try_get("tool_name").map_err(sqlx_err)?,
        risk: parse_enum(row.try_get("risk").map_err(sqlx_err)?, "tool call risk")?,
        input: input.0,
        output_summary: row.try_get("output_summary").map_err(sqlx_err)?,
        status: row.try_get("status").map_err(sqlx_err)?,
        error: row.try_get("error").map_err(sqlx_err)?,
        duration_ms: row.try_get("duration_ms").map_err(sqlx_err)?,
        called_by: Id(row.try_get("called_by").map_err(sqlx_err)?),
        call_source: row.try_get("call_source").map_err(sqlx_err)?,
        profile_id: row
            .try_get::<Option<String>, _>("profile_id")
            .map_err(sqlx_err)?
            .map(Id),
        approval_id: row
            .try_get::<Option<String>, _>("approval_id")
            .map_err(sqlx_err)?
            .map(Id),
        policy_decision: policy_decision.0,
        audit_id: row
            .try_get::<Option<String>, _>("audit_id")
            .map_err(sqlx_err)?
            .map(Id),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
    })
}

fn profile_row(row: sqlx::postgres::PgRow) -> Result<AgentProfile> {
    let allowed_tools: Json<Vec<String>> = row.try_get("allowed_tools").map_err(sqlx_err)?;
    let data_scope: Json<Value> = row.try_get("data_scope").map_err(sqlx_err)?;
    let risk_policy: Json<Value> = row.try_get("risk_policy").map_err(sqlx_err)?;
    Ok(AgentProfile {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        description: row.try_get("description").map_err(sqlx_err)?,
        model_provider_id: row
            .try_get::<Option<String>, _>("model_provider_id")
            .map_err(sqlx_err)?
            .map(Id),
        model: row.try_get("model").map_err(sqlx_err)?,
        allowed_tools: allowed_tools.0,
        data_scope: data_scope.0,
        risk_policy: risk_policy.0,
        network_access: parse_enum::<NetworkAccess>(
            row.try_get("network_access").map_err(sqlx_err)?,
            "agent profile network access",
        )?,
        max_context_tokens: row.try_get("max_context_tokens").map_err(sqlx_err)?,
        max_investigation_secs: row.try_get("max_investigation_secs").map_err(sqlx_err)?,
        max_tool_calls: row.try_get("max_tool_calls").map_err(sqlx_err)?,
        is_default: row.try_get("is_default").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        created_by: Id(row.try_get("created_by").map_err(sqlx_err)?),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(sqlx_err)?),
    })
}

#[async_trait]
impl IntelligenceRepository for PgIntelligenceRepository {
    async fn list_investigations(&self, org_id: &Id) -> Result<Vec<Investigation>> {
        let rows = sqlx::query(
            "SELECT * FROM intelligence_investigations
             WHERE org_id = $1 ORDER BY updated_at_micros DESC",
        )
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(investigation_row).collect()
    }

    async fn get_investigation(&self, org_id: &Id, id: &Id) -> Result<InvestigationDetail> {
        let investigation =
            sqlx::query("SELECT * FROM intelligence_investigations WHERE org_id = $1 AND id = $2")
                .bind(&org_id.0)
                .bind(&id.0)
                .fetch_optional(&self.pool)
                .await
                .map_err(sqlx_err)?
                .ok_or_else(|| Error::not_found(format!("investigation `{}` not found", id.0)))
                .and_then(investigation_row)?;
        let steps = sqlx::query(
            "SELECT * FROM intelligence_investigation_steps
             WHERE org_id = $1 AND investigation_id = $2 ORDER BY position ASC",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?
        .into_iter()
        .map(step_row)
        .collect::<Result<Vec<_>>>()?;
        let evidence = sqlx::query(
            "SELECT * FROM intelligence_investigation_evidence
             WHERE org_id = $1 AND investigation_id = $2 ORDER BY created_at_micros ASC",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?
        .into_iter()
        .map(evidence_row)
        .collect::<Result<Vec<_>>>()?;
        let hypotheses = sqlx::query(
            "SELECT * FROM intelligence_investigation_hypotheses
             WHERE org_id = $1 AND investigation_id = $2 ORDER BY updated_at_micros DESC",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?
        .into_iter()
        .map(hypothesis_row)
        .collect::<Result<Vec<_>>>()?;
        Ok(InvestigationDetail {
            investigation,
            steps,
            evidence,
            hypotheses,
        })
    }

    async fn create_investigation(&self, item: Investigation) -> Result<Investigation> {
        sqlx::query(
            "INSERT INTO intelligence_investigations
             (id, org_id, created_by, chat_id, title, status, context, summary,
              confidence, current_step, started_at_micros, completed_at_micros,
              created_at_micros, updated_at_micros)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
        )
        .bind(&item.id.0)
        .bind(&item.org_id.0)
        .bind(&item.created_by.0)
        .bind(item.chat_id.as_ref().map(|id| &id.0))
        .bind(&item.title)
        .bind(enum_string(&item.status))
        .bind(Json(&item.context))
        .bind(&item.summary)
        .bind(item.confidence.as_ref().map(enum_string))
        .bind(&item.current_step)
        .bind(item.started_at.map(|value| value.0))
        .bind(item.completed_at.map(|value| value.0))
        .bind(item.created_at.0)
        .bind(item.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(item)
    }

    async fn update_investigation(&self, item: Investigation) -> Result<Investigation> {
        let result = sqlx::query(
            "UPDATE intelligence_investigations SET
              title=$3, status=$4, context=$5, summary=$6, confidence=$7, current_step=$8,
              started_at_micros=$9, completed_at_micros=$10, updated_at_micros=$11
             WHERE org_id=$1 AND id=$2",
        )
        .bind(&item.org_id.0)
        .bind(&item.id.0)
        .bind(&item.title)
        .bind(enum_string(&item.status))
        .bind(Json(&item.context))
        .bind(&item.summary)
        .bind(item.confidence.as_ref().map(enum_string))
        .bind(&item.current_step)
        .bind(item.started_at.map(|value| value.0))
        .bind(item.completed_at.map(|value| value.0))
        .bind(item.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found(format!(
                "investigation `{}` not found",
                item.id.0
            )));
        }
        Ok(item)
    }

    async fn append_step(&self, item: InvestigationStep) -> Result<InvestigationStep> {
        sqlx::query(
            "INSERT INTO intelligence_investigation_steps
             (id, investigation_id, org_id, position, title, status, tool_name, input,
              output_summary, conclusion_impact, error, started_at_micros, ended_at_micros,
              created_at_micros)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
        )
        .bind(&item.id.0)
        .bind(&item.investigation_id.0)
        .bind(&item.org_id.0)
        .bind(item.position)
        .bind(&item.title)
        .bind(enum_string(&item.status))
        .bind(&item.tool_name)
        .bind(Json(&item.input))
        .bind(&item.output_summary)
        .bind(&item.conclusion_impact)
        .bind(&item.error)
        .bind(item.started_at.map(|value| value.0))
        .bind(item.ended_at.map(|value| value.0))
        .bind(item.created_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(item)
    }

    async fn append_evidence(&self, item: InvestigationEvidence) -> Result<InvestigationEvidence> {
        sqlx::query(
            "INSERT INTO intelligence_investigation_evidence
             (id, investigation_id, step_id, org_id, kind, label, fact_status, source_ref,
              query, parameters, summary, created_at_micros)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(&item.id.0)
        .bind(&item.investigation_id.0)
        .bind(item.step_id.as_ref().map(|id| &id.0))
        .bind(&item.org_id.0)
        .bind(&item.kind)
        .bind(&item.label)
        .bind(enum_string(&item.fact_status))
        .bind(Json(&item.source_ref))
        .bind(&item.query)
        .bind(Json(&item.parameters))
        .bind(&item.summary)
        .bind(item.created_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(item)
    }

    async fn upsert_hypothesis(
        &self,
        item: InvestigationHypothesis,
    ) -> Result<InvestigationHypothesis> {
        sqlx::query(
            "INSERT INTO intelligence_investigation_hypotheses
             (id, investigation_id, org_id, statement, confidence, status, evidence_ids,
              created_at_micros, updated_at_micros)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
             ON CONFLICT (id) DO UPDATE SET statement=EXCLUDED.statement,
              confidence=EXCLUDED.confidence, status=EXCLUDED.status,
              evidence_ids=EXCLUDED.evidence_ids, updated_at_micros=EXCLUDED.updated_at_micros
             WHERE intelligence_investigation_hypotheses.org_id = EXCLUDED.org_id",
        )
        .bind(&item.id.0)
        .bind(&item.investigation_id.0)
        .bind(&item.org_id.0)
        .bind(&item.statement)
        .bind(enum_string(&item.confidence))
        .bind(enum_string(&item.status))
        .bind(Json(&item.evidence_ids))
        .bind(item.created_at.0)
        .bind(item.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(item)
    }

    async fn list_automations(&self, org_id: &Id) -> Result<Vec<Automation>> {
        let rows = sqlx::query(
            "SELECT * FROM intelligence_automations
             WHERE org_id=$1 ORDER BY updated_at_micros DESC",
        )
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(automation_row).collect()
    }

    async fn get_automation(&self, org_id: &Id, id: &Id) -> Result<Automation> {
        sqlx::query("SELECT * FROM intelligence_automations WHERE org_id=$1 AND id=$2")
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found(format!("automation `{}` not found", id.0)))
            .and_then(automation_row)
    }

    async fn create_automation(&self, item: Automation) -> Result<Automation> {
        sqlx::query(
            "INSERT INTO intelligence_automations
             (id,org_id,name,description,enabled,trigger,input_context,steps,allowed_tools,
              approval_policy,output_actions,failure_policy,notification,created_by,
              created_at_micros,updated_at_micros)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
        )
        .bind(&item.id.0)
        .bind(&item.org_id.0)
        .bind(&item.name)
        .bind(&item.description)
        .bind(item.enabled)
        .bind(Json(&item.trigger))
        .bind(Json(&item.input_context))
        .bind(Json(&item.steps))
        .bind(Json(&item.allowed_tools))
        .bind(Json(&item.approval_policy))
        .bind(Json(&item.output_actions))
        .bind(Json(&item.failure_policy))
        .bind(Json(&item.notification))
        .bind(&item.created_by.0)
        .bind(item.created_at.0)
        .bind(item.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(item)
    }

    async fn update_automation(&self, item: Automation) -> Result<Automation> {
        let result = sqlx::query(
            "UPDATE intelligence_automations SET
              name=$3,description=$4,enabled=$5,trigger=$6,input_context=$7,steps=$8,
              allowed_tools=$9,approval_policy=$10,output_actions=$11,failure_policy=$12,
              notification=$13,updated_at_micros=$14
             WHERE org_id=$1 AND id=$2",
        )
        .bind(&item.org_id.0)
        .bind(&item.id.0)
        .bind(&item.name)
        .bind(&item.description)
        .bind(item.enabled)
        .bind(Json(&item.trigger))
        .bind(Json(&item.input_context))
        .bind(Json(&item.steps))
        .bind(Json(&item.allowed_tools))
        .bind(Json(&item.approval_policy))
        .bind(Json(&item.output_actions))
        .bind(Json(&item.failure_policy))
        .bind(Json(&item.notification))
        .bind(item.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found(format!(
                "automation `{}` not found",
                item.id.0
            )));
        }
        Ok(item)
    }

    async fn list_approvals(&self, org_id: &Id) -> Result<Vec<ApprovalRequest>> {
        let rows = sqlx::query(
            "SELECT * FROM intelligence_approval_requests
             WHERE org_id=$1 ORDER BY created_at_micros DESC",
        )
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(approval_row).collect()
    }

    async fn get_approval(&self, org_id: &Id, id: &Id) -> Result<ApprovalRequest> {
        sqlx::query("SELECT * FROM intelligence_approval_requests WHERE org_id=$1 AND id=$2")
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found(format!("approval `{}` not found", id.0)))
            .and_then(approval_row)
    }

    async fn create_approval(&self, item: ApprovalRequest) -> Result<ApprovalRequest> {
        sqlx::query(
            "INSERT INTO intelligence_approval_requests
             (id,org_id,investigation_id,action,target,parameters,reason,impact,risk,status,
              requested_by,required_approvals,reviews,expires_at_micros,decided_at_micros,
              created_at_micros,updated_at_micros)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)",
        )
        .bind(&item.id.0)
        .bind(&item.org_id.0)
        .bind(item.investigation_id.as_ref().map(|id| &id.0))
        .bind(&item.action)
        .bind(&item.target)
        .bind(Json(&item.parameters))
        .bind(&item.reason)
        .bind(&item.impact)
        .bind(enum_string(&item.risk))
        .bind(enum_string(&item.status))
        .bind(&item.requested_by.0)
        .bind(item.required_approvals)
        .bind(Json(&item.reviews))
        .bind(item.expires_at.map(|value| value.0))
        .bind(item.decided_at.map(|value| value.0))
        .bind(item.created_at.0)
        .bind(item.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(item)
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "intelligence_approval_requests")
    )]
    async fn review_approval(
        &self,
        org_id: &Id,
        id: &Id,
        reviewer: &Id,
        approve: bool,
        comment: &str,
        now: TimestampMicros,
    ) -> Result<ApprovalRequest> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let row = sqlx::query(
            "SELECT * FROM intelligence_approval_requests
             WHERE org_id=$1 AND id=$2 FOR UPDATE",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("approval `{}` not found", id.0)))?;
        let mut item = approval_row(row)?;
        if item.status != ApprovalStatus::Pending {
            return Err(Error::conflict(format!(
                "approval is already {}",
                enum_string(&item.status)
            )));
        }
        if item.expires_at.is_some_and(|expires| expires.0 <= now.0) {
            item.status = ApprovalStatus::Expired;
            item.decided_at = Some(now);
        } else {
            let reviews = item
                .reviews
                .as_array_mut()
                .ok_or_else(|| Error::internal("approval reviews must be an array"))?;
            if reviews
                .iter()
                .any(|review| review["reviewer_id"].as_str() == Some(&reviewer.0))
            {
                return Err(Error::conflict(
                    "reviewer has already reviewed this request",
                ));
            }
            reviews.push(json!({
                "reviewer_id": reviewer.0,
                "decision": if approve { "approved" } else { "rejected" },
                "comment": comment,
                "reviewed_at_micros": now.0,
            }));
            if !approve {
                item.status = ApprovalStatus::Rejected;
                item.decided_at = Some(now);
            } else {
                let approved = reviews
                    .iter()
                    .filter(|review| review["decision"] == "approved")
                    .count() as i32;
                if approved >= item.required_approvals {
                    item.status = ApprovalStatus::Approved;
                    item.decided_at = Some(now);
                }
            }
        }
        item.updated_at = now;
        sqlx::query(
            "UPDATE intelligence_approval_requests
             SET status=$3,reviews=$4,decided_at_micros=$5,updated_at_micros=$6
             WHERE org_id=$1 AND id=$2",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(enum_string(&item.status))
        .bind(Json(&item.reviews))
        .bind(item.decided_at.map(|value| value.0))
        .bind(now.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok(item)
    }

    async fn mark_approval_executed(
        &self,
        org_id: &Id,
        id: &Id,
        now: TimestampMicros,
    ) -> Result<ApprovalRequest> {
        let result = sqlx::query(
            "UPDATE intelligence_approval_requests
             SET status='executed',updated_at_micros=$3
             WHERE org_id=$1 AND id=$2 AND status='approved'",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::conflict("approval is not executable"));
        }
        self.get_approval(org_id, id).await
    }

    async fn list_executions(&self, org_id: &Id) -> Result<Vec<Execution>> {
        let rows = sqlx::query(
            "SELECT * FROM intelligence_executions
             WHERE org_id=$1 ORDER BY created_at_micros DESC",
        )
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(execution_row).collect()
    }

    async fn get_execution(&self, org_id: &Id, id: &Id) -> Result<Execution> {
        sqlx::query("SELECT * FROM intelligence_executions WHERE org_id=$1 AND id=$2")
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found(format!("execution `{}` not found", id.0)))
            .and_then(execution_row)
    }

    async fn find_execution_by_key(&self, org_id: &Id, key: &str) -> Result<Option<Execution>> {
        sqlx::query("SELECT * FROM intelligence_executions WHERE org_id=$1 AND idempotency_key=$2")
            .bind(&org_id.0)
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .map(execution_row)
            .transpose()
    }

    async fn create_execution(&self, item: Execution) -> Result<Execution> {
        let result = sqlx::query(
            "INSERT INTO intelligence_executions
             (id,org_id,approval_request_id,investigation_id,action,target,parameters,
              idempotency_key,requested_by,approved_by,status,output_summary,error,verification,
              started_at_micros,finished_at_micros,created_at_micros,updated_at_micros)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)
             ON CONFLICT DO NOTHING",
        )
        .bind(&item.id.0)
        .bind(&item.org_id.0)
        .bind(&item.approval_request_id.0)
        .bind(item.investigation_id.as_ref().map(|id| &id.0))
        .bind(&item.action)
        .bind(&item.target)
        .bind(Json(&item.parameters))
        .bind(&item.idempotency_key)
        .bind(&item.requested_by.0)
        .bind(Json(&item.approved_by))
        .bind(enum_string(&item.status))
        .bind(&item.output_summary)
        .bind(&item.error)
        .bind(Json(&item.verification))
        .bind(item.started_at.map(|value| value.0))
        .bind(item.finished_at.map(|value| value.0))
        .bind(item.created_at.0)
        .bind(item.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if result.rows_affected() == 1 {
            return Ok(item);
        }
        if let Some(existing) = self
            .find_execution_by_key(&item.org_id, &item.idempotency_key)
            .await?
        {
            return Ok(existing);
        }
        sqlx::query(
            "SELECT * FROM intelligence_executions
             WHERE org_id=$1 AND approval_request_id=$2",
        )
        .bind(&item.org_id.0)
        .bind(&item.approval_request_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::conflict("execution already exists"))
        .and_then(execution_row)
    }

    async fn update_execution(&self, item: Execution) -> Result<Execution> {
        let result = sqlx::query(
            "UPDATE intelligence_executions SET
              status=$3,output_summary=$4,error=$5,verification=$6,started_at_micros=$7,
              finished_at_micros=$8,updated_at_micros=$9
             WHERE org_id=$1 AND id=$2",
        )
        .bind(&item.org_id.0)
        .bind(&item.id.0)
        .bind(enum_string(&item.status))
        .bind(&item.output_summary)
        .bind(&item.error)
        .bind(Json(&item.verification))
        .bind(item.started_at.map(|value| value.0))
        .bind(item.finished_at.map(|value| value.0))
        .bind(item.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found(format!(
                "execution `{}` not found",
                item.id.0
            )));
        }
        Ok(item)
    }

    async fn record_tool_call(&self, item: ToolCallRecord) -> Result<ToolCallRecord> {
        sqlx::query(
            "INSERT INTO intelligence_tool_calls
             (id,org_id,chat_id,investigation_id,step_id,tool_name,risk,input,
              output_summary,status,error,duration_ms,called_by,call_source,profile_id,
              approval_id,policy_decision,audit_id,created_at_micros)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)",
        )
        .bind(&item.id.0)
        .bind(&item.org_id.0)
        .bind(item.chat_id.as_ref().map(|id| &id.0))
        .bind(item.investigation_id.as_ref().map(|id| &id.0))
        .bind(item.step_id.as_ref().map(|id| &id.0))
        .bind(&item.tool_name)
        .bind(enum_string(&item.risk))
        .bind(Json(&item.input))
        .bind(&item.output_summary)
        .bind(&item.status)
        .bind(&item.error)
        .bind(item.duration_ms)
        .bind(&item.called_by.0)
        .bind(&item.call_source)
        .bind(item.profile_id.as_ref().map(|id| &id.0))
        .bind(item.approval_id.as_ref().map(|id| &id.0))
        .bind(Json(&item.policy_decision))
        .bind(item.audit_id.as_ref().map(|id| &id.0))
        .bind(item.created_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(item)
    }

    async fn list_tool_calls(
        &self,
        org_id: &Id,
        tool_name: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ToolCallRecord>> {
        let limit = i64::try_from(limit.clamp(1, 500)).unwrap_or(500);
        let rows = if let Some(tool_name) = tool_name {
            sqlx::query(
                "SELECT * FROM intelligence_tool_calls
                 WHERE org_id=$1 AND tool_name=$2
                 ORDER BY created_at_micros DESC LIMIT $3",
            )
            .bind(&org_id.0)
            .bind(tool_name)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
        } else {
            sqlx::query(
                "SELECT * FROM intelligence_tool_calls
                 WHERE org_id=$1 ORDER BY created_at_micros DESC LIMIT $2",
            )
            .bind(&org_id.0)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
        };
        rows.into_iter().map(tool_call_row).collect()
    }

    async fn list_profiles(&self, org_id: &Id) -> Result<Vec<AgentProfile>> {
        let rows = sqlx::query(
            "SELECT * FROM intelligence_agent_profiles
             WHERE org_id=$1 ORDER BY is_default DESC, updated_at_micros DESC",
        )
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(profile_row).collect()
    }

    async fn get_profile(&self, org_id: &Id, id: &Id) -> Result<AgentProfile> {
        sqlx::query("SELECT * FROM intelligence_agent_profiles WHERE org_id=$1 AND id=$2")
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found(format!("agent profile `{}` not found", id.0)))
            .and_then(profile_row)
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "intelligence_agent_profiles")
    )]
    async fn create_profile(&self, item: AgentProfile) -> Result<AgentProfile> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        if item.is_default {
            sqlx::query(
                "UPDATE intelligence_agent_profiles SET is_default=FALSE
                 WHERE org_id=$1 AND is_default=TRUE",
            )
            .bind(&item.org_id.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }
        sqlx::query(
            "INSERT INTO intelligence_agent_profiles
             (id,org_id,name,description,model_provider_id,model,allowed_tools,data_scope,
              risk_policy,network_access,max_context_tokens,max_investigation_secs,max_tool_calls,
              is_default,enabled,created_by,created_at_micros,updated_at_micros)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)",
        )
        .bind(&item.id.0)
        .bind(&item.org_id.0)
        .bind(&item.name)
        .bind(&item.description)
        .bind(item.model_provider_id.as_ref().map(|id| &id.0))
        .bind(&item.model)
        .bind(Json(&item.allowed_tools))
        .bind(Json(&item.data_scope))
        .bind(Json(&item.risk_policy))
        .bind(enum_string(&item.network_access))
        .bind(item.max_context_tokens)
        .bind(item.max_investigation_secs)
        .bind(item.max_tool_calls)
        .bind(item.is_default)
        .bind(item.enabled)
        .bind(&item.created_by.0)
        .bind(item.created_at.0)
        .bind(item.updated_at.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok(item)
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "intelligence_agent_profiles")
    )]
    async fn update_profile(&self, item: AgentProfile) -> Result<AgentProfile> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        if item.is_default {
            sqlx::query(
                "UPDATE intelligence_agent_profiles SET is_default=FALSE
                 WHERE org_id=$1 AND id<>$2 AND is_default=TRUE",
            )
            .bind(&item.org_id.0)
            .bind(&item.id.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }
        let result = sqlx::query(
            "UPDATE intelligence_agent_profiles SET
              name=$3,description=$4,model_provider_id=$5,model=$6,allowed_tools=$7,
              data_scope=$8,risk_policy=$9,network_access=$10,max_context_tokens=$11,
              max_investigation_secs=$12,max_tool_calls=$13,is_default=$14,enabled=$15,
              updated_at_micros=$16
             WHERE org_id=$1 AND id=$2",
        )
        .bind(&item.org_id.0)
        .bind(&item.id.0)
        .bind(&item.name)
        .bind(&item.description)
        .bind(item.model_provider_id.as_ref().map(|id| &id.0))
        .bind(&item.model)
        .bind(Json(&item.allowed_tools))
        .bind(Json(&item.data_scope))
        .bind(Json(&item.risk_policy))
        .bind(enum_string(&item.network_access))
        .bind(item.max_context_tokens)
        .bind(item.max_investigation_secs)
        .bind(item.max_tool_calls)
        .bind(item.is_default)
        .bind(item.enabled)
        .bind(item.updated_at.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found(format!(
                "agent profile `{}` not found",
                item.id.0
            )));
        }
        tx.commit().await.map_err(sqlx_err)?;
        Ok(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intelligence::model::{
        ConfidenceLevel, FactStatus, HypothesisStatus, InvestigationStatus, RiskLevel, StepStatus,
    };

    #[test]
    fn enum_round_trip_uses_database_wire_values() {
        assert_eq!(enum_string(&RiskLevel::L2), "l2");
        assert_eq!(
            parse_enum::<InvestigationStatus>("waiting_for_approval".into(), "status").unwrap(),
            InvestigationStatus::WaitingForApproval
        );
        assert_eq!(
            parse_enum::<StepStatus>("succeeded".into(), "status").unwrap(),
            StepStatus::Succeeded
        );
        assert_eq!(
            parse_enum::<ConfidenceLevel>("high".into(), "confidence").unwrap(),
            ConfidenceLevel::High
        );
        assert_eq!(
            parse_enum::<FactStatus>("verified".into(), "fact status").unwrap(),
            FactStatus::Verified
        );
        assert_eq!(
            parse_enum::<HypothesisStatus>("supported".into(), "hypothesis").unwrap(),
            HypothesisStatus::Supported
        );
        assert_eq!(
            parse_enum::<NetworkAccess>("allowed".into(), "network access").unwrap(),
            NetworkAccess::Allowed
        );
    }
}
