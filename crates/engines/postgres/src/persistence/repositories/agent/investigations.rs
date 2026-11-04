// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 调查与它的步骤、证据、假设。
//!
//! 步骤、证据、假设存在调查这一行的 JSONB 数组里；追加或改写时锁住这一行，读出数组，改完写回。

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use sqlx::{Row, postgres::PgRow, types::Json};

use super::{super::sqlx_err, PgAgentRepository, enum_string, parse_enum};
use crate::{
    agent::model::{
        ConfidenceLevel, FactStatus, HypothesisStatus, Investigation, InvestigationDetail,
        InvestigationEvidence, InvestigationHypothesis, InvestigationStep, StepStatus,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const INVESTIGATION_COLS: &str = "id, org_id, created_by, chat_id, title, status, context, \
    summary, confidence, current_step, started_at, completed_at, created_at, updated_at";

/// `agent_investigations.steps` 里的一步。
#[derive(Serialize, Deserialize)]
struct StoredStep {
    id: Id,
    position: i32,
    title: String,
    status: StepStatus,
    tool_name: Option<String>,
    input: Value,
    output_summary: Option<String>,
    conclusion_impact: Option<String>,
    error: Option<String>,
    started_at: Option<TimestampMicros>,
    ended_at: Option<TimestampMicros>,
    created_at: TimestampMicros,
}

impl StoredStep {
    fn from_step(item: &InvestigationStep) -> Self {
        Self {
            id: item.id.clone(),
            position: item.position,
            title: item.title.clone(),
            status: item.status,
            tool_name: item.tool_name.clone(),
            input: item.input.clone(),
            output_summary: item.output_summary.clone(),
            conclusion_impact: item.conclusion_impact.clone(),
            error: item.error.clone(),
            started_at: item.started_at,
            ended_at: item.ended_at,
            created_at: item.created_at,
        }
    }

    fn into_step(self, investigation: &Investigation) -> InvestigationStep {
        InvestigationStep {
            id: self.id,
            investigation_id: investigation.id.clone(),
            org_id: investigation.org_id.clone(),
            position: self.position,
            title: self.title,
            status: self.status,
            tool_name: self.tool_name,
            input: self.input,
            output_summary: self.output_summary,
            conclusion_impact: self.conclusion_impact,
            error: self.error,
            started_at: self.started_at,
            ended_at: self.ended_at,
            created_at: self.created_at,
        }
    }
}

/// `agent_investigations.evidence` 里的一条证据。
#[derive(Serialize, Deserialize)]
struct StoredEvidence {
    id: Id,
    step_id: Option<Id>,
    kind: String,
    label: String,
    fact_status: FactStatus,
    source_ref: Value,
    query: Option<String>,
    parameters: Value,
    summary: String,
    created_at: TimestampMicros,
}

impl StoredEvidence {
    fn from_evidence(item: &InvestigationEvidence) -> Self {
        Self {
            id: item.id.clone(),
            step_id: item.step_id.clone(),
            kind: item.kind.clone(),
            label: item.label.clone(),
            fact_status: item.fact_status,
            source_ref: item.source_ref.clone(),
            query: item.query.clone(),
            parameters: item.parameters.clone(),
            summary: item.summary.clone(),
            created_at: item.created_at,
        }
    }

    fn into_evidence(self, investigation: &Investigation) -> InvestigationEvidence {
        InvestigationEvidence {
            id: self.id,
            investigation_id: investigation.id.clone(),
            step_id: self.step_id,
            org_id: investigation.org_id.clone(),
            kind: self.kind,
            label: self.label,
            fact_status: self.fact_status,
            source_ref: self.source_ref,
            query: self.query,
            parameters: self.parameters,
            summary: self.summary,
            created_at: self.created_at,
        }
    }
}

/// `agent_investigations.hypotheses` 里的一个假设。
#[derive(Serialize, Deserialize)]
struct StoredHypothesis {
    id: Id,
    statement: String,
    confidence: ConfidenceLevel,
    status: HypothesisStatus,
    evidence_ids: Vec<Id>,
    created_at: TimestampMicros,
    updated_at: TimestampMicros,
}

impl StoredHypothesis {
    fn from_hypothesis(item: &InvestigationHypothesis) -> Self {
        Self {
            id: item.id.clone(),
            statement: item.statement.clone(),
            confidence: item.confidence,
            status: item.status,
            evidence_ids: item.evidence_ids.clone(),
            created_at: item.created_at,
            updated_at: item.updated_at,
        }
    }

    fn into_hypothesis(self, investigation: &Investigation) -> InvestigationHypothesis {
        InvestigationHypothesis {
            id: self.id,
            investigation_id: investigation.id.clone(),
            org_id: investigation.org_id.clone(),
            statement: self.statement,
            confidence: self.confidence,
            status: self.status,
            evidence_ids: self.evidence_ids,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

fn investigation_row(row: &PgRow) -> Result<Investigation> {
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
        started_at: row.try_get("started_at").map_err(sqlx_err)?,
        completed_at: row.try_get("completed_at").map_err(sqlx_err)?,
        created_at: row.try_get("created_at").map_err(sqlx_err)?,
        updated_at: row.try_get("updated_at").map_err(sqlx_err)?,
    })
}

fn items<T: DeserializeOwned>(row: &PgRow, column: &str) -> Result<Vec<T>> {
    Ok(row.try_get::<Json<Vec<T>>, _>(column).map_err(sqlx_err)?.0)
}

fn not_found(id: &Id) -> Error {
    Error::not_found(format!("investigation `{}` not found", id.0))
}

impl PgAgentRepository {
    pub(super) async fn investigations_in(&self, org_id: &Id) -> Result<Vec<Investigation>> {
        let rows = sqlx::query(&format!(
            "SELECT {INVESTIGATION_COLS} FROM agent_investigations
             WHERE org_id = $1 ORDER BY updated_at DESC"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter().map(investigation_row).collect()
    }

    pub(super) async fn investigation_detail(
        &self,
        org_id: &Id,
        id: &Id,
    ) -> Result<InvestigationDetail> {
        let row = sqlx::query(&format!(
            "SELECT {INVESTIGATION_COLS}, steps, evidence, hypotheses
             FROM agent_investigations WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| not_found(id))?;

        let investigation = investigation_row(&row)?;

        let mut steps: Vec<InvestigationStep> = items::<StoredStep>(&row, "steps")?
            .into_iter()
            .map(|item| item.into_step(&investigation))
            .collect();

        let mut evidence: Vec<InvestigationEvidence> = items::<StoredEvidence>(&row, "evidence")?
            .into_iter()
            .map(|item| item.into_evidence(&investigation))
            .collect();

        let mut hypotheses: Vec<InvestigationHypothesis> =
            items::<StoredHypothesis>(&row, "hypotheses")?
                .into_iter()
                .map(|item| item.into_hypothesis(&investigation))
                .collect();

        steps.sort_by_key(|item| item.position);
        evidence.sort_by_key(|item| item.created_at);
        hypotheses.sort_by_key(|item| std::cmp::Reverse(item.updated_at));

        Ok(InvestigationDetail {
            investigation,
            steps,
            evidence,
            hypotheses,
        })
    }

    pub(super) async fn insert_investigation(&self, item: Investigation) -> Result<Investigation> {
        sqlx::query(
            "INSERT INTO agent_investigations
             (id, org_id, created_by, chat_id, title, status, context, summary,
              confidence, current_step, started_at, completed_at,
              created_at, updated_at)
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
        .bind(item.started_at)
        .bind(item.completed_at)
        .bind(item.created_at)
        .bind(item.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(item)
    }

    pub(super) async fn save_investigation(&self, item: Investigation) -> Result<Investigation> {
        let result = sqlx::query(
            "UPDATE agent_investigations SET
              title=$3, status=$4, context=$5, summary=$6, confidence=$7, current_step=$8,
              started_at=$9, completed_at=$10, updated_at=$11
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
        .bind(item.started_at)
        .bind(item.completed_at)
        .bind(item.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if result.rows_affected() == 0 {
            return Err(not_found(&item.id));
        }

        Ok(item)
    }

    pub(super) async fn push_step(&self, item: InvestigationStep) -> Result<InvestigationStep> {
        self.change_items(
            &item.org_id,
            &item.investigation_id,
            "steps",
            |steps: &mut Vec<StoredStep>| {
                if steps.iter().any(|step| step.position == item.position) {
                    return Err(Error::conflict(format!(
                        "investigation `{}` already has a step at position {}",
                        item.investigation_id.0, item.position
                    )));
                }

                steps.push(StoredStep::from_step(&item));
                Ok(())
            },
        )
        .await?;

        Ok(item)
    }

    pub(super) async fn push_evidence(
        &self,
        item: InvestigationEvidence,
    ) -> Result<InvestigationEvidence> {
        self.change_items(
            &item.org_id,
            &item.investigation_id,
            "evidence",
            |evidence: &mut Vec<StoredEvidence>| {
                evidence.push(StoredEvidence::from_evidence(&item));
                Ok(())
            },
        )
        .await?;

        Ok(item)
    }

    /// 同一 id 的假设改写陈述、置信度、状态、证据与更新时间，保留创建时间。
    pub(super) async fn put_hypothesis(
        &self,
        item: InvestigationHypothesis,
    ) -> Result<InvestigationHypothesis> {
        self.change_items(
            &item.org_id,
            &item.investigation_id,
            "hypotheses",
            |hypotheses: &mut Vec<StoredHypothesis>| {
                let mut stored = StoredHypothesis::from_hypothesis(&item);

                match hypotheses
                    .iter_mut()
                    .find(|hypothesis| hypothesis.id == item.id)
                {
                    Some(existing) => {
                        stored.created_at = existing.created_at;
                        *existing = stored;
                    }
                    None => hypotheses.push(stored),
                }

                Ok(())
            },
        )
        .await?;

        Ok(item)
    }

    /// 锁住调查这一行，读出 `column` 数组，交给 `change` 改，再写回。
    async fn change_items<T, F>(&self, org_id: &Id, id: &Id, column: &str, change: F) -> Result<()>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce(&mut Vec<T>) -> Result<()>,
    {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let row = sqlx::query(&format!(
            "SELECT {column} FROM agent_investigations
             WHERE org_id = $1 AND id = $2 FOR UPDATE"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| not_found(id))?;

        let mut values = items::<T>(&row, column)?;
        change(&mut values)?;

        sqlx::query(&format!(
            "UPDATE agent_investigations SET {column} = $3 WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .bind(Json(&values))
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }
}
