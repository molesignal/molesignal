// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 调查、审批、执行与 Agent Profile 的 Postgres 实现。
//!
//! 调查的 SQL 在 [`investigations`]，Agent Profile 的在 [`profiles`]，自动化在 [`automations`]。

use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    agent::model::{
        AgentProfile, AgentRepository, ApprovalRequest, ApprovalStatus, Execution, Investigation,
        InvestigationDetail, InvestigationEvidence, InvestigationHypothesis, InvestigationStep,
        ToolCallRecord,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub mod automations;
pub mod chat_archives;
pub mod chats;
pub mod currencies;
pub mod feedback;
pub mod im;
pub mod inbound_mcp;
mod investigations;
pub mod knowledge;
pub mod memory;
pub mod model_providers;
pub mod network;
mod profiles;
pub mod prompts;
pub mod runs;
pub mod scoring;
pub mod skills;
pub mod tool_control;
pub mod toolsets;
pub mod usage;

pub struct PgAgentRepository {
    pool: PgPool,
}

impl PgAgentRepository {
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
        expires_at: row
            .try_get::<Option<TimestampMicros>, _>("expires_at")
            .map_err(sqlx_err)?,
        decided_at: row
            .try_get::<Option<TimestampMicros>, _>("decided_at")
            .map_err(sqlx_err)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
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
        started_at: row
            .try_get::<Option<TimestampMicros>, _>("started_at")
            .map_err(sqlx_err)?,
        finished_at: row
            .try_get::<Option<TimestampMicros>, _>("finished_at")
            .map_err(sqlx_err)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
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
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl AgentRepository for PgAgentRepository {
    async fn list_investigations(&self, org_id: &Id) -> Result<Vec<Investigation>> {
        self.investigations_in(org_id).await
    }

    async fn get_investigation(&self, org_id: &Id, id: &Id) -> Result<InvestigationDetail> {
        self.investigation_detail(org_id, id).await
    }

    async fn create_investigation(&self, item: Investigation) -> Result<Investigation> {
        self.insert_investigation(item).await
    }

    async fn update_investigation(&self, item: Investigation) -> Result<Investigation> {
        self.save_investigation(item).await
    }

    async fn append_step(&self, item: InvestigationStep) -> Result<InvestigationStep> {
        self.push_step(item).await
    }

    async fn append_evidence(&self, item: InvestigationEvidence) -> Result<InvestigationEvidence> {
        self.push_evidence(item).await
    }

    async fn upsert_hypothesis(
        &self,
        item: InvestigationHypothesis,
    ) -> Result<InvestigationHypothesis> {
        self.put_hypothesis(item).await
    }

    async fn list_approvals(&self, org_id: &Id) -> Result<Vec<ApprovalRequest>> {
        let rows = sqlx::query(
            "SELECT * FROM agent_approval_requests
             WHERE org_id=$1 ORDER BY created_at DESC",
        )
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(approval_row).collect()
    }

    async fn get_approval(&self, org_id: &Id, id: &Id) -> Result<ApprovalRequest> {
        sqlx::query("SELECT * FROM agent_approval_requests WHERE org_id=$1 AND id=$2")
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
            "INSERT INTO agent_approval_requests
             (id,org_id,investigation_id,action,target,parameters,reason,impact,risk,status,
              requested_by,required_approvals,reviews,expires_at,decided_at,
              created_at,updated_at)
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
        .bind(item.expires_at)
        .bind(item.decided_at)
        .bind(item.created_at)
        .bind(item.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(item)
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "agent_approval_requests")
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
            "SELECT * FROM agent_approval_requests
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
                .any(|review| review["reviewerId"].as_str() == Some(&reviewer.0))
            {
                return Err(Error::conflict(
                    "reviewer has already reviewed this request",
                ));
            }

            reviews.push(json!({
                "reviewerId": reviewer.0,
                "decision": if approve { "approved" } else { "rejected" },
                "comment": comment,
                "reviewedAt": now.0,
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
            "UPDATE agent_approval_requests
             SET status=$3,reviews=$4,decided_at=$5,updated_at=$6
             WHERE org_id=$1 AND id=$2",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(enum_string(&item.status))
        .bind(Json(&item.reviews))
        .bind(item.decided_at)
        .bind(now)
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
            "UPDATE agent_approval_requests
             SET status='executed',updated_at=$3
             WHERE org_id=$1 AND id=$2 AND status='approved'",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(now)
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
            "SELECT * FROM agent_executions
             WHERE org_id=$1 ORDER BY created_at DESC",
        )
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(execution_row).collect()
    }

    async fn get_execution(&self, org_id: &Id, id: &Id) -> Result<Execution> {
        sqlx::query("SELECT * FROM agent_executions WHERE org_id=$1 AND id=$2")
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found(format!("execution `{}` not found", id.0)))
            .and_then(execution_row)
    }

    async fn find_execution_by_key(&self, org_id: &Id, key: &str) -> Result<Option<Execution>> {
        sqlx::query("SELECT * FROM agent_executions WHERE org_id=$1 AND idempotency_key=$2")
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
            "INSERT INTO agent_executions
             (id,org_id,approval_request_id,investigation_id,action,target,parameters,
              idempotency_key,requested_by,approved_by,status,output_summary,error,verification,
              started_at,finished_at,created_at,updated_at)
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
        .bind(item.started_at)
        .bind(item.finished_at)
        .bind(item.created_at)
        .bind(item.updated_at)
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
            "SELECT * FROM agent_executions
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
            "UPDATE agent_executions SET
              status=$3,output_summary=$4,error=$5,verification=$6,started_at=$7,
              finished_at=$8,updated_at=$9
             WHERE org_id=$1 AND id=$2",
        )
        .bind(&item.org_id.0)
        .bind(&item.id.0)
        .bind(enum_string(&item.status))
        .bind(&item.output_summary)
        .bind(&item.error)
        .bind(Json(&item.verification))
        .bind(item.started_at)
        .bind(item.finished_at)
        .bind(item.updated_at)
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
            "INSERT INTO agent_tool_calls
             (id,org_id,chat_id,investigation_id,step_id,tool_name,risk,input,
              output_summary,status,error,duration_ms,called_by,call_source,profile_id,
              approval_id,policy_decision,audit_id,created_at)
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
        .bind(item.created_at)
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
                "SELECT * FROM agent_tool_calls
                 WHERE org_id=$1 AND tool_name=$2
                 ORDER BY created_at DESC LIMIT $3",
            )
            .bind(&org_id.0)
            .bind(tool_name)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
        } else {
            sqlx::query(
                "SELECT * FROM agent_tool_calls
                 WHERE org_id=$1 ORDER BY created_at DESC LIMIT $2",
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
        profiles::list(&self.pool, org_id).await
    }

    async fn get_profile(&self, org_id: &Id, id: &Id) -> Result<AgentProfile> {
        profiles::get(&self.pool, org_id, id).await
    }

    async fn create_profile(&self, item: AgentProfile) -> Result<AgentProfile> {
        profiles::create(&self.pool, item).await
    }

    async fn update_profile(&self, item: AgentProfile) -> Result<AgentProfile> {
        profiles::update(&self.pool, item).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::model::{
        ConfidenceLevel, FactStatus, HypothesisStatus, InvestigationStatus, NetworkAccess,
        RiskLevel, StepStatus,
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
